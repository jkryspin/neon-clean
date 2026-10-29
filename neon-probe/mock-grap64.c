/* neon-probe: mock grap64.dll - disable the GRAP anti-cheat before injecting.
 *
 * MapleStory.exe statically imports exactly one symbol from grap64.dll: ORDINAL 9.
 * The real grap64.dll is Themida-packed, exports 9 functions with NO names and
 * export base 1, and MapleStory.exe's own Themida stub validates its imports -
 * replacing grap64 with a mock that has a different export layout (base 9, one
 * named export) makes Themida abort with "An Error has occured while loading
 * imports. Wrong DLL present." So this mock reproduces the export shape: base 1,
 * nine NONAME ordinals, ordinal 9 being the entry point the game calls.
 *
 * See docs/anticheat-mock.md. The real grap64.dll is backed up as
 * grap64.dll.orig by scripts/mock-anticheat.sh.
 *
 * Build: make mock-grap64-win.dll   (x86_64-w64-mingw32-gcc + mock-grap64.def)
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

/* Nine ordinal-only exports (mock-grap64.def maps them to ordinals 1..9). */
long long grap_ord1(void) { return 1; }
long long grap_ord2(void) { return 1; }
long long grap_ord3(void) { return 1; }
long long grap_ord4(void) { return 1; }
long long grap_ord5(void) { return 1; }
long long grap_ord6(void) { return 1; }
long long grap_ord7(void) { return 1; }
long long grap_ord8(void) { return 1; }
long long grap_ord9(void) { logmsg("grap ordinal 9 called"); return 1; }

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
