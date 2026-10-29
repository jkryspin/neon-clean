/* neon-probe: mock grap64.dll - disable the GRAP anti-cheat before injecting.
 *
 * MapleStory.exe statically imports exactly one symbol from grap64.dll: ORDINAL 9
 * (grap64.dll has no named exports; see docs/anticheat-mock.md). Dropping this
 * stub in as grap64.dll makes the game launch with no Themida/GRAP protector:
 * no virtualised region, no self-integrity checks, no protector watching our
 * injected pages. The game's own packet code is plain .text, so the hook/probe
 * work becomes "easy, same as 20 years ago".
 *
 * The real grap64.dll is backed up as grap64.dll.orig by scripts/mock-anticheat.sh.
 *
 * Build: make mock-grap64-win.dll   (x86_64-w64-mingw32-gcc + mock-grap64.def)
 * Compile standalone:
 *   x86_64-w64-mingw32-gcc -Wall -O2 -shared -o grap64.dll mock-grap64.c mock-grap64.def
 *
 * The ordinal-9 stub returns 1 (success). If the game is seen to dereference a
 * pointer from it, change the return type - the log will show it was called.
 */
#include <windows.h>
#include <stdio.h>

static FILE *g_log;

static void logmsg(const char *m) {
    if (g_log) {
        fprintf(g_log, "{\"t\":%llu,\"m\":\"%s\"}\n",
                (unsigned long long)GetTickCount64(), m);
        fflush(g_log);
    }
}

/* Exported as ordinal 9 by mock-grap64.def. */
long long grap_entry(void);
long long grap_entry(void) {
    logmsg("grap ordinal 9 called");
    return 1;
}

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);
        char path[MAX_PATH];
        snprintf(path, sizeof path, "Z:\\tmp\\neon-mock-grap64.%lu.log",
                 (unsigned long)GetCurrentProcessId());
        g_log = fopen(path, "a");
        logmsg("mock grap64 attached");
    } else if (reason == DLL_PROCESS_DETACH) {
        if (g_log) { logmsg("mock grap64 detached"); fclose(g_log); g_log = NULL; }
    }
    return TRUE;
}
