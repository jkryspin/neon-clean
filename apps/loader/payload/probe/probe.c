/* neon_probe.dll - an OBSERVER loaded into a client under Wine to learn which code handles which packet.
 *
 * DESIGN RULES (each exists because of something that already cost a session, see docs/decode-plan.md):
 *   - It NEVER writes to the target's code. No inline hooks, no int3, no patched jumps. Inline hooks are the
 *     single most integrity-check-detectable thing an injected module can do.
 *   - It never sets or clears debug registers (the client already occupies all four).
 *   - It never calls ptrace-using debugging APIs (no DebugActiveProcess, no WaitForDebugEvent).
 *   - Everything it does is reversible: unloading restores the exact page protections it changed.
 *   - It is an OBSERVER for protocol research. It does not hide itself from anything and has no
 *     anti-detection logic; that is deliberate, not an omission.
 *
 * STAGE 1 (this file): prove the module loads, then report what it can see. Nothing else.
 * Output goes to Z:\tmp\neon-probe.<pid>.log, i.e. /tmp on the Linux side (Wine maps / to Z:).
 */
#include <windows.h>
#include <stdio.h>

static FILE *g_log;
static volatile LONG g_stop;

static void logf_(const char *fmt, ...) {
    if (!g_log) return;
    SYSTEMTIME st; GetLocalTime(&st);
    fprintf(g_log, "%02d:%02d:%02d.%03d ", st.wHour, st.wMinute, st.wSecond, st.wMilliseconds);
    va_list ap; va_start(ap, fmt); vfprintf(g_log, fmt, ap); va_end(ap);
    fputc('\n', g_log); fflush(g_log);
}

static DWORD WINAPI report_thread(LPVOID unused) {
    (void)unused;
    HMODULE exe = GetModuleHandleW(NULL);
    logf_("probe loaded in pid %lu, main module base %p", GetCurrentProcessId(), (void *)exe);

    /* Enumerate the committed regions of the main image so the log shows we can see the same layout the
     * /proc/PID/maps analysis found (plain r-xp image vs the protector's rwxp region). Read-only. */
    MEMORY_BASIC_INFORMATION mbi; unsigned char *p = (unsigned char *)0x140000000ULL; int shown = 0;
    while (p < (unsigned char *)0x146000000ULL && VirtualQuery(p, &mbi, sizeof mbi) == sizeof mbi) {
        if (mbi.State == MEM_COMMIT && shown < 24) {
            DWORD pr = mbi.Protect & 0xFF;
            const char *name = pr == PAGE_EXECUTE_READ ? "r-x" : pr == PAGE_EXECUTE_READWRITE ? "RWX(protector)" :
                               pr == PAGE_READONLY ? "r--" : pr == PAGE_READWRITE ? "rw-" : "other";
            logf_("  region %p size %#zx %s", mbi.BaseAddress, mbi.RegionSize, name); shown++;
        }
        p = (unsigned char *)mbi.BaseAddress + mbi.RegionSize;
    }
    while (!InterlockedCompareExchange(&g_stop, 0, 0)) Sleep(500);
    logf_("probe stopping cleanly");
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);
        char path[MAX_PATH];
        snprintf(path, sizeof path, "Z:\\tmp\\neon-probe.%lu.log", GetCurrentProcessId());
        g_log = fopen(path, "a");
        /* Do the real work on our OWN thread: DllMain runs under the loader lock, so doing anything non-trivial
         * here can deadlock the host. */
        HANDLE t = CreateThread(NULL, 0, report_thread, NULL, 0, NULL);
        if (t) CloseHandle(t);
    } else if (reason == DLL_PROCESS_DETACH) {
        InterlockedExchange(&g_stop, 1);
    }
    return TRUE;
}
