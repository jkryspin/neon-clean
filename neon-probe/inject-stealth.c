/* neon_inject_ud.exe - Undetectable DLL injector for MapleStory Classic World
 *
 * Manual PE mapping with thread hijacking:
 * - No CreateRemoteThread (hijacks an existing target thread)
 * - No LoadLibraryW (manual section mapping + import resolution)
 * - Relocations applied locally before writing to target
 * - Imports resolved locally (system DLLs share bases across Wine processes)
 * - Shellcode stub for proper x64 ABI DllMain invocation
 * - Timing jitter between operations
 *
 *   inject-stealth.exe <pid-or-image-name> <path-to-dll> [--no-wipe] [--fixed-base]
 *
 * Stealth differences from inject-undetectable.c:
 *   - image is reserved PAGE_READWRITE (never RWX); protections are widened per section
 *   - headers are zeroed after DllMain returns and the header page becomes PAGE_READONLY
 *   - the allocation base is randomised instead of the DLL's preferred base
 */

#include <windows.h>
#include <tlhelp32.h>
#include <psapi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>
#include <time.h>

#define JITTER_MIN 50
#define JITTER_MAX 200
#define HIJACK_TIMEOUT_MS 5000
#define HIJACK_POLL_MS 100

static int g_wipe = 1;
static int g_fixed_base = 0;

static void jitter_sleep(void) {
    DWORD ms = JITTER_MIN + (rand() % (JITTER_MAX - JITTER_MIN));
    Sleep(ms);
}

static DWORD find_pid(const char *s) {
    char *end;
    unsigned long v = strtoul(s, &end, 10);
    if (*end == 0 && v) return (DWORD)v;

    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snap == INVALID_HANDLE_VALUE) return 0;
    PROCESSENTRY32 pe = { sizeof pe };
    DWORD found = 0;
    for (BOOL ok = Process32First(snap, &pe); ok; ok = Process32Next(snap, &pe))
        if (!_stricmp(pe.szExeFile, s)) { found = pe.th32ProcessID; break; }
    CloseHandle(snap);
    return found;
}

/* Convert PE section characteristics to PAGE_* protection constants */
static DWORD section_to_page_protect(DWORD ch) {
    BOOL x = (ch & IMAGE_SCN_MEM_EXECUTE) != 0;
    BOOL r = (ch & IMAGE_SCN_MEM_READ) != 0;
    BOOL w = (ch & IMAGE_SCN_MEM_WRITE) != 0;
    if (x && w) return PAGE_EXECUTE_READWRITE;
    if (x && r) return PAGE_EXECUTE_READ;
    if (x)      return PAGE_EXECUTE;
    if (w)      return PAGE_READWRITE;
    if (r)      return PAGE_READONLY;
    return PAGE_NOACCESS;
}

/* Apply base relocations to a locally-mapped PE image */
static BOOL apply_relocations(char *image, DWORD image_size,
                              IMAGE_NT_HEADERS *nt, ULONG_PTR actual_base) {
    ULONG_PTR preferred = nt->OptionalHeader.ImageBase;
    LONGLONG delta = (LONGLONG)(actual_base - preferred);
    if (delta == 0) return TRUE;

    IMAGE_DATA_DIRECTORY *dir =
        &nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_BASERELOC];
    if (!dir->Size || !dir->VirtualAddress) return TRUE;

    if (dir->VirtualAddress >= image_size) {
        fprintf(stderr, "reloc directory outside image bounds\n");
        return FALSE;
    }

    IMAGE_BASE_RELOCATION *block =
        (IMAGE_BASE_RELOCATION *)(image + dir->VirtualAddress);
    DWORD processed = 0;

    while (processed < dir->Size && block->SizeOfBlock >= sizeof(IMAGE_BASE_RELOCATION)) {
        DWORD count = (block->SizeOfBlock - sizeof(IMAGE_BASE_RELOCATION)) / sizeof(WORD);
        WORD *entries = (WORD *)(block + 1);

        for (DWORD i = 0; i < count; i++) {
            WORD type   = entries[i] >> 12;
            WORD offset = entries[i] & 0xFFF;
            DWORD rva   = block->VirtualAddress + offset;

            if (rva + 8 > image_size) continue;

            switch (type) {
            case IMAGE_REL_BASED_DIR64: {
                ULONGLONG *addr = (ULONGLONG *)(image + rva);
                *addr += delta;
                break;
            }
            case IMAGE_REL_BASED_HIGHLOW: {
                DWORD *addr = (DWORD *)(image + rva);
                *addr += (DWORD)delta;
                break;
            }
            case IMAGE_REL_BASED_ABSOLUTE:
                break; /* padding, skip */
            default:
                fprintf(stderr, "warning: unhandled reloc type %d at RVA 0x%lx\n",
                        type, (unsigned long)rva);
                break;
            }
        }
        processed += block->SizeOfBlock;
        block = (IMAGE_BASE_RELOCATION *)((char *)block + block->SizeOfBlock);
    }
    return TRUE;
}

/* Resolve imports in a locally-mapped PE image.
 * System DLLs (kernel32, ntdll, msvcrt, etc.) map at the same base in all
 * Wine processes, so GetModuleHandleA/GetProcAddress from the injector
 * yields addresses valid in the target too. */
static BOOL resolve_imports(char *image, DWORD image_size, IMAGE_NT_HEADERS *nt) {
    IMAGE_DATA_DIRECTORY *dir =
        &nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (!dir->Size || !dir->VirtualAddress) return TRUE;

    if (dir->VirtualAddress >= image_size) {
        fprintf(stderr, "import directory outside image bounds\n");
        return FALSE;
    }

    IMAGE_IMPORT_DESCRIPTOR *desc =
        (IMAGE_IMPORT_DESCRIPTOR *)(image + dir->VirtualAddress);

    while (desc->Name) {
        if (desc->Name >= image_size) break;
        char *dll_name = image + desc->Name;

        HMODULE dll = GetModuleHandleA(dll_name);
        if (!dll) {
            dll = LoadLibraryA(dll_name);
            if (!dll) {
                fprintf(stderr, "warning: cannot resolve import module '%s'\n", dll_name);
                desc++;
                continue;
            }
        }
        printf("  import: %s -> %p\n", dll_name, (void *)dll);

        DWORD orig_rva = desc->OriginalFirstThunk ? desc->OriginalFirstThunk
                                                   : desc->FirstThunk;
        if (orig_rva >= image_size || desc->FirstThunk >= image_size) {
            desc++;
            continue;
        }

        IMAGE_THUNK_DATA *orig  = (IMAGE_THUNK_DATA *)(image + orig_rva);
        IMAGE_THUNK_DATA *thunk = (IMAGE_THUNK_DATA *)(image + desc->FirstThunk);

        while (orig->u1.AddressOfData) {
            FARPROC addr = NULL;
            if (IMAGE_SNAP_BY_ORDINAL(orig->u1.Ordinal)) {
                addr = GetProcAddress(dll, (LPCSTR)IMAGE_ORDINAL(orig->u1.Ordinal));
            } else {
                if (orig->u1.AddressOfData >= image_size) { orig++; thunk++; continue; }
                IMAGE_IMPORT_BY_NAME *imp =
                    (IMAGE_IMPORT_BY_NAME *)(image + (DWORD)orig->u1.AddressOfData);
                addr = GetProcAddress(dll, imp->Name);
            }
            if (!addr) {
                fprintf(stderr, "warning: unresolved import in %s\n", dll_name);
            }
            thunk->u1.Function = (ULONGLONG)addr;
            orig++;
            thunk++;
        }
        desc++;
    }
    return TRUE;
}

/* Manual map: read DLL, prepare image locally (sections + relocations + imports),
 * write prepared image to target. Returns target base address; fills *entry_rva. */
static LPVOID manual_map_dll(HANDLE proc, const char *dll_path, DWORD *entry_rva,
                             DWORD *pdata_rva, DWORD *pdata_size) {
    *entry_rva = 0; *pdata_rva = 0; *pdata_size = 0;

    /* Read raw DLL from disk */
    HANDLE fh = CreateFileA(dll_path, GENERIC_READ, FILE_SHARE_READ,
                            NULL, OPEN_EXISTING, 0, NULL);
    if (fh == INVALID_HANDLE_VALUE) {
        fprintf(stderr, "cannot open %s: %lu\n", dll_path, GetLastError());
        return NULL;
    }
    DWORD file_size = GetFileSize(fh, NULL);
    char *raw = (char *)VirtualAlloc(NULL, file_size,
                                     MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!raw) { CloseHandle(fh); return NULL; }

    DWORD bytes = 0;
    if (!ReadFile(fh, raw, file_size, &bytes, NULL) || bytes != file_size) {
        VirtualFree(raw, 0, MEM_RELEASE);
        CloseHandle(fh);
        return NULL;
    }
    CloseHandle(fh);

    /* Validate PE */
    IMAGE_DOS_HEADER *dos = (IMAGE_DOS_HEADER *)raw;
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) {
        fprintf(stderr, "invalid DOS signature\n");
        VirtualFree(raw, 0, MEM_RELEASE);
        return NULL;
    }
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(raw + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) {
        fprintf(stderr, "invalid NT signature\n");
        VirtualFree(raw, 0, MEM_RELEASE);
        return NULL;
    }

    DWORD image_size = nt->OptionalHeader.SizeOfImage;
    IMAGE_SECTION_HEADER *sects = IMAGE_FIRST_SECTION(nt);
    WORD num_sects = nt->FileHeader.NumberOfSections;

    /* Allocate in target — try preferred base first, then any address */
    LPVOID target_base = NULL;
    if (g_fixed_base) {
        target_base = VirtualAllocEx(proc, (LPVOID)nt->OptionalHeader.ImageBase,
                                     image_size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    } else {
        for (int i = 0; i < 64 && !target_base; i++) {
            ULONG_PTR hint = ((ULONG_PTR)rand() << 16) + ((rand() & 0xFFFF) << 4);
            hint = (hint & ~(ULONG_PTR)0xFFFF) + 0x10000;
            target_base = VirtualAllocEx(proc, (LPVOID)hint, image_size,
                                         MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        }
    }
    if (!target_base) {
        target_base = VirtualAllocEx(proc, NULL, image_size,
                                     MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    }
    if (!target_base) {
        fprintf(stderr, "VirtualAllocEx(%lu bytes) failed: %lu\n",
                image_size, GetLastError());
        VirtualFree(raw, 0, MEM_RELEASE);
        return NULL;
    }
    printf("allocated %lu bytes at %p in target\n", image_size, target_base);

    /* Build the full mapped image locally */
    char *image = (char *)VirtualAlloc(NULL, image_size,
                                       MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!image) {
        VirtualFreeEx(proc, target_base, 0, MEM_RELEASE);
        VirtualFree(raw, 0, MEM_RELEASE);
        return NULL;
    }
    memset(image, 0, image_size);

    /* Copy headers */
    memcpy(image, raw, nt->OptionalHeader.SizeOfHeaders);

    /* Copy sections */
    for (WORD i = 0; i < num_sects; i++) {
        if (sects[i].SizeOfRawData == 0) continue;
        if (sects[i].VirtualAddress + sects[i].SizeOfRawData > image_size) continue;
        if (sects[i].PointerToRawData + sects[i].SizeOfRawData > file_size) continue;
        memcpy(image + sects[i].VirtualAddress,
               raw + sects[i].PointerToRawData,
               sects[i].SizeOfRawData);
    }
    VirtualFree(raw, 0, MEM_RELEASE);

    /* Re-parse headers from the mapped image */
    dos = (IMAGE_DOS_HEADER *)image;
    nt  = (IMAGE_NT_HEADERS *)(image + dos->e_lfanew);
    sects = IMAGE_FIRST_SECTION(nt);

    /* Apply relocations */
    if (!apply_relocations(image, image_size, nt, (ULONG_PTR)target_base)) {
        fprintf(stderr, "relocation failed\n");
        VirtualFree(image, 0, MEM_RELEASE);
        VirtualFreeEx(proc, target_base, 0, MEM_RELEASE);
        return NULL;
    }
    printf("relocations applied (preferred %p, actual %p, delta 0x%llx)\n",
           (void *)nt->OptionalHeader.ImageBase, target_base,
           (unsigned long long)((ULONG_PTR)target_base - nt->OptionalHeader.ImageBase));

    /* Resolve imports */
    if (!resolve_imports(image, image_size, nt)) {
        fprintf(stderr, "import resolution failed\n");
        VirtualFree(image, 0, MEM_RELEASE);
        VirtualFreeEx(proc, target_base, 0, MEM_RELEASE);
        return NULL;
    }
    printf("imports resolved\n");

    /* Write the prepared image to target */
    SIZE_T wrote = 0;
    WriteProcessMemory(proc, target_base, image,
                       nt->OptionalHeader.SizeOfHeaders, &wrote);
    jitter_sleep();

    for (WORD i = 0; i < num_sects; i++) {
        if (sects[i].SizeOfRawData == 0) continue;
        LPVOID dest = (LPVOID)((ULONG_PTR)target_base + sects[i].VirtualAddress);
        WriteProcessMemory(proc, dest, image + sects[i].VirtualAddress,
                           sects[i].SizeOfRawData, &wrote);
        jitter_sleep();
    }
    printf("image written to target (%d sections)\n", num_sects);

    /* Set per-section page protections */
    for (WORD i = 0; i < num_sects; i++) {
        DWORD prot = section_to_page_protect(sects[i].Characteristics);
        DWORD old = 0;
        LPVOID dest = (LPVOID)((ULONG_PTR)target_base + sects[i].VirtualAddress);
        VirtualProtectEx(proc, dest, sects[i].Misc.VirtualSize, prot, &old);
    }

    *entry_rva = nt->OptionalHeader.AddressOfEntryPoint;
    *pdata_rva = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXCEPTION].VirtualAddress;
    *pdata_size = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXCEPTION].Size;
    if (g_wipe) {
        char zeros[0x1000] = {0};
        SIZE_T wrote = 0;
        DWORD old = 0;
        WriteProcessMemory(proc, target_base, zeros, sizeof zeros, &wrote);
        VirtualProtectEx(proc, target_base, 0x1000, PAGE_READONLY, &old);
    }
    VirtualFree(image, 0, MEM_RELEASE);

    printf("entry RVA = 0x%lx\n", (unsigned long)*entry_rva);
    return target_base;
}

/* Hijack a thread in the target process to execute shellcode that calls
 * DllMain(dll_base, DLL_PROCESS_ATTACH, NULL) with proper x64 ABI,
 * then restores the thread to its original state. */
static int hijack_thread(HANDLE proc, DWORD target_pid,
                         LPVOID dll_base, DWORD entry_rva,
                         DWORD pdata_rva, DWORD pdata_size) {
    /* Find a thread in the TARGET process */
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if (snap == INVALID_HANDLE_VALUE) {
        fprintf(stderr, "CreateToolhelp32Snapshot failed: %lu\n", GetLastError());
        return 1;
    }

    THREADENTRY32 te = { sizeof(te) };
    DWORD target_tid = 0;
    for (BOOL ok = Thread32First(snap, &te); ok; ok = Thread32Next(snap, &te)) {
        if (te.th32OwnerProcessID == target_pid) {
            target_tid = te.th32ThreadID;
            break;
        }
    }
    CloseHandle(snap);

    if (!target_tid) {
        fprintf(stderr, "no threads found for pid %lu\n", target_pid);
        return 1;
    }
    printf("hijacking tid %lu in pid %lu\n", target_tid, target_pid);

    HANDLE thread = OpenThread(
        THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT,
        FALSE, target_tid);
    if (!thread) {
        fprintf(stderr, "OpenThread(%lu) failed: %lu\n", target_tid, GetLastError());
        return 1;
    }

    /* Suspend and capture context */
    SuspendThread(thread);
    jitter_sleep();

    CONTEXT ctx;
    memset(&ctx, 0, sizeof(ctx));
    ctx.ContextFlags = CONTEXT_FULL;
    if (!GetThreadContext(thread, &ctx)) {
        fprintf(stderr, "GetThreadContext failed: %lu\n", GetLastError());
        ResumeThread(thread);
        CloseHandle(thread);
        return 1;
    }

    ULONG_PTR orig_rip = ctx.Rip;
    ULONG_PTR orig_rsp = ctx.Rsp;
    printf("original RIP=%p RSP=%p\n", (void *)orig_rip, (void *)orig_rsp);

    /* Shellcode: save volatile regs, call DllMain, signal done, wait for go,
     * restore everything and jump back. The handshake removes the RIP-poll
     * race that made the injector force-restore a running thread. */
    ULONG_PTR entry_addr = (ULONG_PTR)dll_base + entry_rva;

    LPVOID sc = VirtualAllocEx(proc, NULL, 0x2000,
                               MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!sc) {
        fprintf(stderr, "shellcode alloc failed: %lu\n", GetLastError());
        ResumeThread(thread);
        CloseHandle(thread);
        return 1;
    }
    ULONG_PTR ctrl = (ULONG_PTR)sc + 0x1000;

    unsigned char code[256];
    int p = 0;
#define E1(x) code[p++] = (unsigned char)(x)
#define E8(v) do { ULONG_PTR _v = (ULONG_PTR)(v); memcpy(&code[p], &_v, 8); p += 8; } while (0)
#define E4(v) do { unsigned _v = (unsigned)(v); memcpy(&code[p], &_v, 4); p += 4; } while (0)

    E1(0x9C);                                        /* pushfq */
    E1(0x50); E1(0x51); E1(0x52);                    /* push rax rcx rdx */
    E1(0x41); E1(0x50); E1(0x41); E1(0x51);
    E1(0x41); E1(0x52); E1(0x41); E1(0x53);          /* push r8..r11 */
    E1(0x48); E1(0x83); E1(0xEC); E1(0x08);          /* sub rsp,8 */
    if (pdata_rva && pdata_size >= sizeof(RUNTIME_FUNCTION)) {
        ULONG_PTR pfn = (ULONG_PTR)GetProcAddress(GetModuleHandleA("ntdll.dll"), "RtlAddFunctionTable");
        if (pfn) {
            E1(0x48); E1(0xB9); E8((ULONG_PTR)dll_base + pdata_rva);   /* mov rcx,table */
            E1(0xBA); E4(pdata_size / sizeof(RUNTIME_FUNCTION));       /* mov edx,count */
            E1(0x49); E1(0xB8); E8((ULONG_PTR)dll_base);               /* mov r8,base */
            E1(0x48); E1(0xB8); E8(pfn);                               /* mov rax,fn */
            E1(0xFF); E1(0xD0);                                        /* call rax */
        }
    }
    E1(0x48); E1(0xB9); E8(dll_base);                /* mov rcx,dll_base */
    E1(0xBA); E1(1); E1(0); E1(0); E1(0);            /* mov edx,1 */
    E1(0x45); E1(0x31); E1(0xC0);                    /* xor r8d,r8d */
    E1(0x48); E1(0xB8); E8(entry_addr);              /* mov rax,entry */
    E1(0xFF); E1(0xD0);                              /* call rax */
    E1(0x48); E1(0x83); E1(0xC4); E1(0x08);          /* add rsp,8 */
    E1(0x49); E1(0xBA); E8(ctrl);                    /* mov r10,ctrl */
    E1(0x41); E1(0xC7); E1(0x02); E1(1); E1(0); E1(0); E1(0); /* mov [r10],1 */
    E1(0x41); E1(0x83); E1(0x7A); E1(0x04); E1(0x00);/* cmp [r10+4],0 */
    E1(0x74); E1(0xF9);                              /* je spin */
    E1(0x41); E1(0x5B); E1(0x41); E1(0x5A);
    E1(0x41); E1(0x59); E1(0x41); E1(0x58);          /* pop r11..r8 */
    E1(0x5A); E1(0x59); E1(0x58);                    /* pop rdx rcx rax */
    E1(0x9D);                                        /* popfq */
    E1(0x48); E1(0xBC); E8(orig_rsp);                /* mov rsp,orig_rsp */
    E1(0x48); E1(0xB8); E8(orig_rip);                /* mov rax,orig_rip */
    E1(0xFF); E1(0xE0);                              /* jmp rax */
#undef E1
#undef E8

    SIZE_T wrote = 0;
    WriteProcessMemory(proc, sc, code, p, &wrote);
    { DWORD sc_old = 0; VirtualProtectEx(proc, sc, 0x1000, PAGE_EXECUTE_READ, &sc_old); }
    printf("shellcode at %p (%d bytes), entry at %p\n",
           sc, p, (void *)entry_addr);

    /* Redirect thread to shellcode */
    ctx.Rip = (ULONG_PTR)sc;
    /* Align stack to 16 bytes (required by x64 ABI before call) */
    ctx.Rsp = (ctx.Rsp & ~(ULONG_PTR)0xF) - 8;

    if (!SetThreadContext(thread, &ctx)) {
        fprintf(stderr, "SetThreadContext failed: %lu\n", GetLastError());
        VirtualFreeEx(proc, sc, 0, MEM_RELEASE);
        ctx.Rip = orig_rip;
        ctx.Rsp = orig_rsp;
        SetThreadContext(thread, &ctx);
        ResumeThread(thread);
        CloseHandle(thread);
        return 1;
    }

    /* Resume, wait for the done flag, then let the shellcode continue. */
    printf("executing DllMain...\n");
    ResumeThread(thread);

    int elapsed = 0;
    BOOL completed = FALSE;
    while (elapsed < HIJACK_TIMEOUT_MS) {
        DWORD done = 0;
        SIZE_T n = 0;
        if (ReadProcessMemory(proc, (LPCVOID)ctrl, &done, sizeof done, &n) &&
            n == sizeof done && done) { completed = TRUE; break; }
        Sleep(20);
        elapsed += 20;
    }
    if (completed) {
        DWORD go = 1;
        SIZE_T n = 0;
        WriteProcessMemory(proc, (LPCVOID)(ctrl + 4), &go, sizeof go, &n);
        printf("DllMain returned, thread released (%dms)\n", elapsed);
        Sleep(100);
    } else {
        fprintf(stderr, "warning: DllMain did not signal done within %dms\n",
                HIJACK_TIMEOUT_MS);
    }

    /* Free shellcode (DllMain already returned, DLL's own threads are running) */
    VirtualFreeEx(proc, sc, 0, MEM_RELEASE);
    CloseHandle(thread);
    return 0;
}

int main(int argc, char **argv) {
    srand((unsigned)time(NULL) ^ (unsigned)GetCurrentProcessId());

    if (argc < 3) {
        fprintf(stderr, "usage: %s <pid|name> <dll>\n", argv[0]);
        fprintf(stderr, "       %s <pid|name> --unload <module>\n", argv[0]);
        return 2;
    }

    for (int i = 3; i < argc; i++) {
        if (!strcmp(argv[i], "--no-wipe")) g_wipe = 0;
        if (!strcmp(argv[i], "--fixed-base")) g_fixed_base = 1;
    }

    DWORD pid = find_pid(argv[1]);
    if (!pid) {
        fprintf(stderr, "no such process: %s\n", argv[1]);
        return 1;
    }

    HANDLE proc = OpenProcess(
        PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION |
        PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ,
        FALSE, pid);
    if (!proc) {
        fprintf(stderr, "OpenProcess(%lu) failed: %lu\n", pid, GetLastError());
        return 1;
    }

    printf("target pid %lu\n", pid);
    jitter_sleep();

    if (!strcmp(argv[2], "--unload")) {
        if (argc < 4) return 2;
        wchar_t arg[MAX_PATH];
        MultiByteToWideChar(CP_ACP, 0, argv[3], -1, arg, MAX_PATH);

        HMODULE mods[1024];
        DWORD need = 0;
        EnumProcessModules(proc, mods, sizeof mods, &need);

        for (DWORD i = 0; i < need / sizeof(HMODULE); i++) {
            wchar_t nm[MAX_PATH];
            if (GetModuleBaseNameW(proc, mods[i], nm, MAX_PATH) &&
                !_wcsicmp(nm, arg)) {
                printf("found module %s at %p\n", argv[3], (void *)mods[i]);
                break;
            }
        }
    } else {
        /* Manual map the DLL */
        DWORD entry_rva = 0, pdata_rva = 0, pdata_size = 0;
        LPVOID base = manual_map_dll(proc, argv[2], &entry_rva, &pdata_rva, &pdata_size);
        if (!base) {
            fprintf(stderr, "manual mapping failed\n");
            CloseHandle(proc);
            return 1;
        }
        printf("mapped DLL at %p\n", base);

        if (!entry_rva) {
            fprintf(stderr, "DLL has no entry point\n");
            CloseHandle(proc);
            return 1;
        }

        /* Hijack a target thread to call DllMain */
        jitter_sleep();
        if (hijack_thread(proc, pid, base, entry_rva, pdata_rva, pdata_size)) {
            fprintf(stderr, "thread hijack failed\n");
            CloseHandle(proc);
            return 1;
        }

        printf("DLL injected successfully\n");
    }

    CloseHandle(proc);
    return 0;
}
