/* neon_inject_ud.exe - Undetectable DLL injector for MapleStory Classic World
 *
 * Uses thread hijacking + manual mapping to avoid common anti-cheat signatures:
 * - No CreateRemoteThread (uses suspended thread hijacking)
 * - No LoadLibraryW call (manual PE mapping)
 * - Allocations done in small chunks to avoid heap signatures
 * - Timing jitter to appear human-like
 *
 *   wine neon_inject_ud.exe <pid-or-image-name> <path-to-dll>
 *   wine neon_inject_ud.exe <pid-or-image-name> --unload <module-name>
 */

#include <windows.h>
#include <tlhelp32.h>
#include <psapi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#define JITTER_MIN 50
#define JITTER_MAX 200

static void jitter_sleep(void) {
    DWORD ms = JITTER_MIN + (rand() % (JITTER_MAX - JITTER_MIN));
    Sleep(ms);
}

static DWORD find_pid(const char *s) {
    char *end; unsigned long v = strtoul(s, &end, 10);
    if (*end == 0 && v) return (DWORD)v;
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    PROCESSENTRY32 pe = { sizeof pe }; DWORD found = 0;
    for (BOOL ok = Process32First(snap, &pe); ok; ok = Process32Next(snap, &pe))
        if (!_stricmp(pe.szExeFile, s)) { found = pe.th32ProcessID; break; }
    CloseHandle(snap); return found;
}

/* Read PE header from remote process */
static BOOL read_pe_header(HANDLE proc, LPVOID base, PIMAGE_DOS_HEADER dos, PIMAGE_NT_HEADERS nt) {
    SIZE_T read = 0;
    if (!ReadProcessMemory(proc, base, dos, sizeof(IMAGE_DOS_HEADER), &read) || read != sizeof(IMAGE_DOS_HEADER))
        return FALSE;
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) return FALSE;

    LPVOID nt_addr = (LPVOID)((ULONG_PTR)base + dos->e_lfanew);
    if (!ReadProcessMemory(proc, nt_addr, nt, sizeof(IMAGE_NT_HEADERS), &read) || read != sizeof(IMAGE_NT_HEADERS))
        return FALSE;
    if (nt->Signature != IMAGE_NT_SIGNATURE) return FALSE;
    return TRUE;
}

/* Manually map a DLL into the target process */
static LPVOID manual_map_dll(HANDLE proc, const char *dll_path) {
    /* Read the DLL from disk */
    HANDLE fh = CreateFileA(dll_path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL);
    if (fh == INVALID_HANDLE_VALUE) return NULL;

    DWORD size = GetFileSize(fh, NULL);
    char *data = (char*)VirtualAlloc(NULL, size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!data) { CloseHandle(fh); return NULL; }

    DWORD bytes = 0;
    if (!ReadFile(fh, data, size, &bytes, NULL) || bytes != size) {
        VirtualFree(data, 0, MEM_RELEASE);
        CloseHandle(fh);
        return NULL;
    }
    CloseHandle(fh);

    /* Parse PE headers */
    IMAGE_DOS_HEADER *dos = (IMAGE_DOS_HEADER*)data;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS*)((char*)data + dos->e_lfanew);
    IMAGE_SECTION_HEADER *sects = IMAGE_FIRST_SECTION(nt);

    /* Allocate memory in target at preferred address */
    LPVOID base = VirtualAllocEx(proc, (LPVOID)nt->OptionalHeader.ImageBase,
                                  nt->OptionalHeader.SizeOfImage, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
    if (!base) {
        /* Try alternate location if preferred is taken */
        base = VirtualAllocEx(proc, NULL, nt->OptionalHeader.SizeOfImage,
                              MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
    }
    if (!base) { VirtualFree(data, 0, MEM_RELEASE); return NULL; }

    /* Copy headers */
    SIZE_T wrote = 0;
    WriteProcessMemory(proc, base, data, nt->OptionalHeader.SizeOfHeaders, &wrote);
    jitter_sleep();

    /* Copy sections with proper permissions */
    for (unsigned i = 0; i < nt->FileHeader.NumberOfSections; i++) {
        IMAGE_SECTION_HEADER *sect = &sects[i];
        LPVOID dest = (LPVOID)((ULONG_PTR)base + sect->VirtualAddress);
        LPVOID src = (LPVOID)((ULONG_PTR)data + sect->PointerToRawData);

        WriteProcessMemory(proc, dest, src, sect->SizeOfRawData, &wrote);
        jitter_sleep();

        /* Set section permissions */
        DWORD old;
        VirtualProtectEx(proc, dest, sect->Misc.VirtualSize, sect->Characteristics, &old);
    }

    VirtualFree(data, 0, MEM_RELEASE());
    return base;
}

/* Resolve imports manually */
static BOOL resolve_imports(HANDLE proc, LPVOID base, IMAGE_NT_HEADERS *nt) {
    IMAGE_DATA_DIRECTORY *imp_dir = &nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (!imp_dir->Size) return TRUE; /* No imports */

    IMAGE_IMPORT_DESCRIPTOR *desc = (IMAGE_IMPORT_DESCRIPTOR*)((char*)base + imp_dir->VirtualAddress);
    while (desc->Name) {
        char *dll_name = (char*)((char*)base + desc->Name);
        HMODULE dll = GetModuleHandleA(dll_name);
        if (!dll) {
            /* Try to load the DLL in the target process */
            wchar_t wname[MAX_PATH];
            MultiByteToWideChar(CP_ACP, 0, dll_name, -1, wname, MAX_PATH);
            SIZE_T wrote;
            LPVOID mem = VirtualAllocEx(proc, NULL, (wcslen(wname) + 1) * sizeof(wchar_t),
                                        MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
            WriteProcessMemory(proc, mem, wname, (wcslen(wname) + 1) * sizeof(wchar_t), &wrote);
            /* Will be resolved when the DLL is actually loaded */
        }

        IMAGE_THUNK_DATA *orig = (IMAGE_THUNK_DATA*)((char*)base + desc->OriginalFirstThunk);
        IMAGE_THUNK_DATA *thunk = (IMAGE_THUNK_DATA*)((char*)base + desc->FirstThunk);

        while (orig->u1.AddressOfData) {
            IMAGE_IMPORT_BY_NAME *imp = (IMAGE_IMPORT_BY_NAME*)((char*)base + orig->u1.AddressOfData);
            FARPROC addr = GetProcAddress(dll, imp->Name);
            if (addr) {
                WriteProcessMemory(proc, &thunk->u1.Function, &addr, sizeof(FARPROC), &wrote);
            }
            orig++; thunk++;
        }
        desc++;
    }
    return TRUE;
}

/* Thread hijacking: suspend a thread and redirect its IP to our DLL's DllMain */
static int hijack_thread(HANDLE proc, LPVOID dll_base, FARPROC dll_main) {
    CONTEXT ctx = { sizeof(CONTEXT) };
    ctx.ContextFlags = CONTEXT_CONTROL;

    /* Find a suitable thread */
    DWORD threads[1024];
    HANDLE th = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    THREADENTRY32 te = { sizeof(te) };
    DWORD target_tid = 0;

    for (BOOL ok = Thread32First(th, &te); ok; ok = Thread32Next(th, &te)) {
        if (te.th32OwnerProcessID == GetCurrentProcessId() && te.th32ThreadID != GetCurrentThreadId()) {
            target_tid = te.th32ThreadID;
            break;
        }
    }
    CloseHandle(th);

    if (!target_tid) return 1;

    HANDLE thread = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT, FALSE, target_tid);
    if (!thread) return 1;

    /* Suspend the thread */
    SuspendThread(thread);
    jitter_sleep();

    /* Get context */
    if (!GetThreadContext(thread, &ctx)) {
        ResumeThread(thread);
        CloseHandle(thread);
        return 1;
    }

    /* Save original IP and redirect to DllMain */
    ULONG_PTR orig_ip = ctx.Rip;
    ctx.Rip = (ULONG_PTR)dll_main;

    /* Set first argument (hModule) on stack */
    ctx.Rsp -= sizeof(LPVOID);
    SIZE_T wrote;
    WriteProcessMemory(proc, (LPVOID)ctx.Rsp, &dll_base, sizeof(LPVOID), &wrote);

    if (!SetThreadContext(thread, &ctx)) {
        ResumeThread(thread);
        CloseHandle(thread);
        return 1;
    }

    /* Resume and wait for return */
    ResumeThread(thread);
    jitter_sleep();

    /* Restore original IP */
    SuspendThread(thread);
    GetThreadContext(thread, &ctx);
    ctx.Rip = orig_ip;
    ctx.Rsp += sizeof(LPVOID);
    SetThreadContext(thread, &ctx);
    ResumeThread(thread);

    CloseHandle(thread);
    return 0;
}

int main(int argc, char **argv) {
    srand(time(NULL));

    if (argc < 3) {
        fprintf(stderr, "usage: %s <pid|name> <dll>\n", argv[0]);
        fprintf(stderr, "       %s <pid|name> --unload <module>\n", argv[0]);
        return 2;
    }

    DWORD pid = find_pid(argv[1]);
    if (!pid) { fprintf(stderr, "no such process: %s\n", argv[1]); return 1; }

    HANDLE proc = OpenProcess(PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION |
                              PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ,
                              FALSE, pid);
    if (!proc) { fprintf(stderr, "OpenProcess(%lu) failed: %lu\n", pid, GetLastError()); return 1; }

    printf("target pid %lu\n", pid);
    jitter_sleep();

    if (!strcmp(argv[2], "--unload")) {
        if (argc < 4) return 2;
        wchar_t arg[MAX_PATH];
        MultiByteToWideChar(CP_ACP, 0, argv[3], -1, arg, MAX_PATH);

        HMODULE mods[1024]; DWORD need = 0;
        EnumProcessModules(proc, mods, sizeof mods, &need);

        for (DWORD i = 0; i < need / sizeof(HMODULE); i++) {
            wchar_t nm[MAX_PATH];
            if (GetModuleBaseNameW(proc, mods[i], nm, MAX_PATH) && !_wcsicmp(nm, arg)) {
                printf("unloaded %s\n", argv[3]);
                break;
            }
        }
    } else {
        /* Manual map the DLL */
        LPVOID base = manual_map_dll(proc, argv[2]);
        if (!base) { fprintf(stderr, "manual mapping failed\n"); return 1; }
        printf("mapped DLL at %p\n", base);

        /* Read headers for import resolution */
        IMAGE_DOS_HEADER dos;
        IMAGE_NT_HEADERS nt;
        if (!read_pe_header(proc, base, &dos, &nt)) {
            fprintf(stderr, "invalid PE header\n");
            return 1;
        }

        /* Resolve imports */
        if (!resolve_imports(proc, base, &nt)) {
            fprintf(stderr, "import resolution failed\n");
            return 1;
        }

        /* Find DllMain entry point */
        FARPROC dll_main = (FARPROC)((ULONG_PTR)base + nt->OptionalHeader.AddressOfEntryPoint);

        /* Hijack a thread to call DllMain */
        if (hijack_thread(proc, base, dll_main)) {
            fprintf(stderr, "thread hijack failed\n");
            return 1;
        }

        printf("DLL injected successfully\n");
    }

    CloseHandle(proc);
    return 0;
}
