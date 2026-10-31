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
 * Return convention: the client calls ordinal 9 from its fail-fast helpers and
 * tests the result (`test eax,eax; je ok; int 29h`), so NONZERO means failure
 * and success is 0. Every export returns 0.
 *
 * See docs/anticheat-mock.md. The real grap64.dll is backed up as
 * grap64.dll.orig by scripts/mock-anticheat.sh.
 *
 * Build: cl /nologo /LD /MT /O2 /W3 mock-grap64.c /link /DEF:mock-grap64.def
 *          /OUT:mock-grap64-win.dll
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
long long grap_ord1(void) { return 0; }
long long grap_ord2(void) { return 0; }
long long grap_ord3(void) { return 0; }
long long grap_ord4(void) { return 0; }
long long grap_ord5(void) { return 0; }
long long grap_ord6(void) { return 0; }
long long grap_ord7(void) { return 0; }
long long grap_ord8(void) { return 0; }
long long grap_ord9(void) { logmsg("grap ordinal 9 called"); return 0; }

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);
        char path[MAX_PATH];
        CreateDirectoryW(L"C:\\Neon", NULL);
        CreateDirectoryW(L"C:\\Neon\\logs", NULL);
        snprintf(path, sizeof path, "C:\\Neon\\logs\\mock-grap64.%lu.log",
                 (unsigned long)GetCurrentProcessId());
        g_log = fopen(path, "a");
        logmsg("mock grap64 attached");
    } else if (reason == DLL_PROCESS_DETACH) {
        if (g_log) { logmsg("mock grap64 detached"); fclose(g_log); g_log = NULL; }
    }
    return TRUE;
}
