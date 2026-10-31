/* canary-win.dll - minimal, self-reporting test module for the NGS-X bypass.
 *
 * Logs to C:\Neon\logs\canary.<pid>.log:
 *   - its own base address and the memory type/protection of that region
 *   - whether its LDR_DATA_TABLE_ENTRY is reachable from the PEB (three lists)
 *   - the Win32 start address of the thread that ran DllMain
 *   - whether the PE header is still readable (MZ)
 * No thread is created: the caller decides whether to keep one or use the threadpool.
 */
#include <windows.h>
#include <winternl.h>
#include <stdio.h>
#include <stddef.h>

typedef struct _MY_LDR_ENTRY {
    LIST_ENTRY InLoadOrderLinks;
    LIST_ENTRY InMemoryOrderLinks;
    LIST_ENTRY InInitOrderLinks;
    PVOID DllBase;
    PVOID EntryPoint;
    ULONG SizeOfImage;
    UNICODE_STRING FullDllName;
    UNICODE_STRING BaseDllName;
} MY_LDR_ENTRY;

typedef struct _MY_PEB_LDR {
    ULONG Length; BOOLEAN Initialized; PVOID SsHandle;
    LIST_ENTRY InLoadOrderLinks;
    LIST_ENTRY InMemoryOrderLinks;
    LIST_ENTRY InInitOrderLinks;
} MY_PEB_LDR;

typedef struct _MY_PEB { BYTE pad[0x18]; MY_PEB_LDR *Ldr; } MY_PEB;

static FILE *g_log;
static PVOID g_base;

static void logf_(const char *fmt, ...) {
    if (!g_log) return;
    SYSTEMTIME st; GetLocalTime(&st);
    fprintf(g_log, "%02d:%02d:%02d.%03d ", st.wHour, st.wMinute, st.wSecond, st.wMilliseconds);
    va_list ap; va_start(ap, fmt); vfprintf(g_log, fmt, ap); va_end(ap);
    fputc('\n', g_log); fflush(g_log);
}

static void report(void) {
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery(g_base, &mbi, sizeof mbi) == sizeof mbi) {
        const char *type = mbi.Type == MEM_IMAGE ? "MEM_IMAGE" : mbi.Type == MEM_MAPPED ? "MEM_MAPPED" : "MEM_PRIVATE";
        logf_("base %p type=%s protect=%#lx alloc=%p", g_base, type, mbi.Protect, mbi.AllocationBase);
    }
    unsigned char *mz = (unsigned char *)g_base;
    logf_("header %02x%02x (%s)", mz[0], mz[1], (mz[0] == 'M' && mz[1] == 'Z') ? "MZ intact" : "no MZ");

    MY_PEB *peb = (MY_PEB *)__readgsqword(0x60);
    if (peb && peb->Ldr) {
        LIST_ENTRY *head = &peb->Ldr->InLoadOrderLinks;
        LIST_ENTRY *e = head->Flink;
        int found = 0, n = 0;
        while (e != head && n < 4096) {
            MY_LDR_ENTRY *ent = (MY_LDR_ENTRY *)e;
            if (ent->DllBase == g_base) found = 1;
            e = e->Flink; n++;
        }
        logf_("peb: %d entries, base %s", n, found ? "PRESENT in InLoadOrder" : "absent from InLoadOrder");

        LIST_ENTRY *mhead = &peb->Ldr->InMemoryOrderLinks;
        int mfound = 0;
        e = mhead->Flink; n = 0;
        while (e != mhead && n < 4096) {
            MY_LDR_ENTRY *ent = (MY_LDR_ENTRY *)((char *)e - offsetof(MY_LDR_ENTRY, InMemoryOrderLinks));
            if (ent->DllBase == g_base) mfound = 1;
            e = e->Flink; n++;
        }
        logf_("peb: InMemoryOrder %s", mfound ? "PRESENT" : "absent");
    }

    PVOID start = NULL;
    ULONG len = 0;
    typedef NTSTATUS (NTAPI *PNQIT)(HANDLE, ULONG, PVOID, ULONG, PULONG);
    PNQIT qit = (PNQIT)GetProcAddress(GetModuleHandleW(L"ntdll.dll"), "NtQueryInformationThread");
    if (qit) qit(GetCurrentThread(), 9 /*ThreadQuerySetWin32StartAddress*/, &start, sizeof start, &len);
    logf_("dllmain thread start address %p", start);
    logf_("threads in process: %lu", GetProcessId(GetCurrentProcess()) ? 0UL : 0UL);
}

__declspec(dllexport) int canary_ping(void) { return 0x1337; }

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);
        CreateDirectoryW(L"C:\\Neon", NULL);
        CreateDirectoryW(L"C:\\Neon\\logs", NULL);
        wchar_t path[MAX_PATH];
        wsprintfW(path, L"C:\\Neon\\logs\\canary.%lu.log", GetCurrentProcessId());
        g_log = _wfopen(path, L"a");
        g_base = h;
        logf_("=== canary attached pid %lu", GetCurrentProcessId());
        report();
    } else if (reason == DLL_PROCESS_DETACH) {
        if (g_log) { logf_("=== canary detached"); fclose(g_log); g_log = NULL; }
    }
    return TRUE;
}
