/* neon-probe: mock nmcogame64.dll - disable the NGS nmco layer before injecting.
 *
 * MapleStory.exe statically imports exactly one symbol from nmcogame64.dll,
 * NMCO_MemoryFree (see docs/anticheat-mock.md). The real DLL forwards every
 * export into the NGS runtime via a callback the runtime registers; under Wine
 * the runtime is half-loaded, so the real NMCO_MemoryFree already degrades to a
 * no-op when nothing registered the callback. This stub makes that explicit and
 * keeps the game from touching the NGS service at all.
 *
 * The real nmcogame64.dll is backed up as nmcogame64.dll.orig by
 * scripts/mock-anticheat.sh.
 *
 * Build: x86_64-w64-mingw32-gcc -Wall -O2 -shared -o nmcogame64.dll \
 *          mock-nmcogame64.c mock-nmcogame64.def
 *
 * All stubs are inert; NMCO_MemoryFree is a deliberate no-op (leak, not a wrong
 * free) which matches the degraded real behaviour. Everything is logged so we
 * can see which entry points the game actually calls.
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

/* ── NMCO_* stubs (signatures are deliberately permissive) ── */
void NMCO_MemoryFree(void *p) {
    (void)p;
    logmsg("NMCO_MemoryFree called");
}

long long NMCO_CallNMFunction(void *a, void *b, void *c, void *d, void *e) {
    (void)a; (void)b; (void)c; (void)d; (void)e;
    logmsg("NMCO_CallNMFunction called");
    return 0;
}

void NMCO_SetLocaleAndRegion(int a, int b) { (void)a; (void)b; logmsg("NMCO_SetLocaleAndRegion called"); }
void NMCO_SetPatchOption(int a) { (void)a; logmsg("NMCO_SetPatchOption called"); }
void NMCO_SetVersionFileUrlA(const char *a) { (void)a; logmsg("NMCO_SetVersionFileUrlA called"); }
void NMCO_SetVersionFileUrlW(const wchar_t *a) { (void)a; logmsg("NMCO_SetVersionFileUrlW called"); }

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);
        char path[MAX_PATH];
        snprintf(path, sizeof path, "Z:\\tmp\\neon-mock-nmcogame64.%lu.log",
                 (unsigned long)GetCurrentProcessId());
        g_log = fopen(path, "a");
        logmsg("mock nmcogame64 attached");
    } else if (reason == DLL_PROCESS_DETACH) {
        if (g_log) { logmsg("mock nmcogame64 detached"); fclose(g_log); g_log = NULL; }
    }
    return TRUE;
}
