/* neon-probe: real CreateProcessW detour in nexon_client.exe to capture the
 * one-time session token the Nexon launcher passes when it spawns MapleStory.exe.
 *
 * The launcher (nexon_client.exe / nexon_x64.dll / nexon_runtime.exe) builds the
 * game command line and calls CreateProcessW from kernel32. We IAT-hook every
 * module in the injected process that imports CreateProcessW (or CreateProcessA)
 * and copy the token out of the game's command line. No code bytes are patched
 * and no debugger is attached - only the launcher's import slots are rewritten,
 * which is exactly the "hook into nexon_client" step.
 *
 * Inject into the launcher process(es) while the launcher is up but before PLAY
 * is clicked:
 *   ./inject.sh nexon_client.exe hook-session.dll
 * (scripts/mock-anticheat.sh does this for every nexon_* process.)
 *
 * The token is kept in named shared memory (NeonSessionToken) and never written
 * to disk, per the authorization terms. neon-cmd reads it with
 * neon_get_session_token(). A copy is also appended to
 * Z:\tmp\neon-session-hook.<pid>.log for debugging.
 *
 * Build: x86_64-w64-mingw32-gcc -Wall -O2 -D_WIN32_WINNT=0x0600 -shared \
 *          -o hook-session.dll hook-session.c -lpsapi -lkernel32
 */
#include <windows.h>
#include <psapi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ── Session token storage (memory-only, per authorization) ── */
#define TOKEN_SHARED_NAME "NeonSessionToken"
#define TOKEN_SIZE 512

static HANDLE g_token_map = NULL;
static char  *g_token_ptr = NULL;
static CRITICAL_SECTION g_cs;
static FILE  *g_log = NULL;

/* ── Real entry points we trampoline to ── */
typedef BOOL (WINAPI *CreateProcessW_fn)(
    LPCWSTR, LPWSTR, LPSECURITY_ATTRIBUTES, LPSECURITY_ATTRIBUTES, BOOL,
    DWORD, LPVOID, LPCWSTR, LPSTARTUPINFOW, LPPROCESS_INFORMATION);
typedef BOOL (WINAPI *CreateProcessA_fn)(
    LPCSTR, LPSTR, LPSECURITY_ATTRIBUTES, LPSECURITY_ATTRIBUTES, BOOL,
    DWORD, LPVOID, LPCSTR, LPSTARTUPINFOA, LPPROCESS_INFORMATION);

static CreateProcessW_fn g_orig_w = NULL;
static CreateProcessA_fn g_orig_a = NULL;

/* Addresses (as imported) that mean "this slot is CreateProcessW/A". */
static ULONG_PTR g_w_addrs[4];
static ULONG_PTR g_a_addrs[4];
static int g_w_count = 0, g_a_count = 0;
static BOOL g_installed = FALSE;

static void logmsg(const char *fmt, ...) {
    if (!g_log) return;
    va_list ap;
    va_start(ap, fmt);
    fprintf(g_log, "{\"t\":%llu,\"m\":\"",
            (unsigned long long)GetTickCount64());
    vfprintf(g_log, fmt, ap);
    va_end(ap);
    fprintf(g_log, "\"}\n");
    fflush(g_log);
}

/* ── Case-insensitive substring (narrow) ── */
static int contains_ci(const char *hay, const char *needle) {
    size_t n = strlen(needle);
    for (; *hay; hay++)
        if (!_strnicmp(hay, needle, n)) return 1;
    return 0;
}

/* ── Token extraction: named params, then a long-hex fallback ── */
static void extract_token(const char *narrow, char *out, size_t out_size) {
    static const char *patterns[] = {
        "--token=", "-token=", "token=", "--session=", "-session=", "session=",
        "?token=", "&token=", "session_token=", "access_token=", "nxToken="
    };
    for (size_t i = 0; i < sizeof patterns / sizeof patterns[0]; i++) {
        const char *found = strstr(narrow, patterns[i]);
        if (found) {
            found += strlen(patterns[i]);
            size_t j = 0;
            while (*found && j < out_size - 1) {
                char c = *found;
                if ((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') ||
                    (c >= 'A' && c <= 'F') || c == '-' || c == '_') {
                    out[j++] = c;
                } else break;
                found++;
            }
            out[j] = '\0';
            if (j >= 16) return;
        }
    }
    /* Fallback: first run of 32+ hex chars. */
    size_t run = 0, pos = 0;
    while (narrow[pos]) {
        char c = narrow[pos];
        int hex = (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
        if (hex) {
            if (++run >= 32) {
                size_t start = pos - run + 1;
                size_t len = run;
                if (len > out_size - 1) len = out_size - 1;
                memcpy(out, narrow + start, len);
                out[len] = '\0';
                return;
            }
        } else run = 0;
        pos++;
    }
}

static void store_token(const char *token) {
    if (!token[0] || !g_token_ptr) return;
    EnterCriticalSection(&g_cs);
    strncpy(g_token_ptr, token, TOKEN_SIZE - 1);
    g_token_ptr[TOKEN_SIZE - 1] = '\0';
    LeaveCriticalSection(&g_cs);
    logmsg("session token captured len=%zu", strlen(token));
}

static void capture_game_launch(const char *app, const char *cmd, DWORD pid) {
    char buf[16384];
    buf[0] = '\0';
    if (app && app[0]) { strncat(buf, app, sizeof buf - 1); strncat(buf, " ", sizeof buf - strlen(buf) - 1); }
    if (cmd && cmd[0]) strncat(buf, cmd, sizeof buf - strlen(buf) - 1);

    if (contains_ci(buf, "MapleStory.exe") || contains_ci(buf, "maplestory.exe")) {
        logmsg("spawned game pid=%lu cmd=%s", (unsigned long)pid, buf);
        char token[TOKEN_SIZE] = {0};
        extract_token(buf, token, sizeof token);
        if (token[0]) store_token(token);
        else logmsg("game launched but no token matched");
    }
}

/* ── Hooks ── */
static BOOL WINAPI hooked_CreateProcessW(
    LPCWSTR app, LPWSTR cmd, LPSECURITY_ATTRIBUTES pa, LPSECURITY_ATTRIBUTES ta,
    BOOL inherit, DWORD flags, LPVOID env, LPCWSTR dir,
    LPSTARTUPINFOW si, LPPROCESS_INFORMATION pi) {

    char appn[4096] = {0}, cmdn[16384] = {0};
    if (app) WideCharToMultiByte(CP_UTF8, 0, app, -1, appn, sizeof appn, NULL, NULL);
    if (cmd) WideCharToMultiByte(CP_UTF8, 0, cmd, -1, cmdn, sizeof cmdn, NULL, NULL);

    BOOL r = g_orig_w(app, cmd, pa, ta, inherit, flags, env, dir, si, pi);
    if (r && pi) capture_game_launch(appn, cmdn, pi->dwProcessId);
    return r;
}

static BOOL WINAPI hooked_CreateProcessA(
    LPCSTR app, LPSTR cmd, LPSECURITY_ATTRIBUTES pa, LPSECURITY_ATTRIBUTES ta,
    BOOL inherit, DWORD flags, LPVOID env, LPCSTR dir,
    LPSTARTUPINFOA si, LPPROCESS_INFORMATION pi) {

    BOOL r = g_orig_a(app, cmd, pa, ta, inherit, flags, env, dir, si, pi);
    if (r && pi) capture_game_launch(app, cmd, pi->dwProcessId);
    return r;
}

/* ── IAT patching ── */
static void hook_module(HMODULE mod, const char *name) {
    BYTE *b = (BYTE *)mod;
    if (!b || b[0] != 'M' || b[1] != 'Z') return;
    IMAGE_DOS_HEADER *dos = (IMAGE_DOS_HEADER *)b;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(b + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) return;

    IMAGE_DATA_DIRECTORY imp =
        nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (!imp.VirtualAddress) return;

    IMAGE_IMPORT_DESCRIPTOR *d = (IMAGE_IMPORT_DESCRIPTOR *)(b + imp.VirtualAddress);
    int patched = 0;
    for (; d->Name; d++) {
        IMAGE_THUNK_DATA *t = (IMAGE_THUNK_DATA *)(b + d->FirstThunk);
        for (; t->u1.Function; t++) {
            ULONG_PTR *slot = (ULONG_PTR *)&t->u1.Function;
            void *want = NULL;
            for (int i = 0; i < g_w_count; i++) if (*slot == g_w_addrs[i]) { want = hooked_CreateProcessW; break; }
            if (!want) for (int i = 0; i < g_a_count; i++) if (*slot == g_a_addrs[i]) { want = hooked_CreateProcessA; break; }
            if (want) {
                DWORD old;
                if (VirtualProtect(slot, sizeof(void *), PAGE_READWRITE, &old)) {
                    *slot = (ULONG_PTR)want;
                    VirtualProtect(slot, sizeof(void *), old, &old);
                    patched++;
                }
            }
        }
    }
    if (patched) logmsg("iat-hooked %d slot(s) in %s", patched, name ? name : "?");
}

static void add_addr(ULONG_PTR *arr, int *count, HMODULE mod, const char *fn) {
    if (!mod) return;
    FARPROC p = GetProcAddress(mod, fn);
    if (p && *count < 4) arr[(*count)++] = (ULONG_PTR)p;
}

static void install_hooks(void) {
    if (g_installed) return;
    g_installed = TRUE;

    HMODULE k32 = GetModuleHandleA("kernel32.dll");
    HMODULE kbase = LoadLibraryA("kernelbase.dll");
    add_addr(g_w_addrs, &g_w_count, k32, "CreateProcessW");
    add_addr(g_w_addrs, &g_w_count, kbase, "CreateProcessW");
    add_addr(g_a_addrs, &g_a_count, k32, "CreateProcessA");
    add_addr(g_a_addrs, &g_a_count, kbase, "CreateProcessA");
    g_orig_w = (CreateProcessW_fn)GetProcAddress(k32, "CreateProcessW");
    g_orig_a = (CreateProcessA_fn)GetProcAddress(k32, "CreateProcessA");

    HMODULE mods[1024];
    DWORD need = 0;
    if (!EnumProcessModules(GetCurrentProcess(), mods, sizeof mods, &need)) {
        logmsg("EnumProcessModules failed %lu", GetLastError());
        return;
    }
    for (DWORD i = 0; i < need / sizeof(HMODULE); i++) {
        char nm[MAX_PATH] = {0};
        GetModuleBaseNameA(GetCurrentProcess(), mods[i], nm, sizeof nm);
        hook_module(mods[i], nm);
    }
    logmsg("install complete (w=%d a=%d)", g_w_count, g_a_count);
}

/* ── DllMain ── */
BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);
        char path[MAX_PATH];
        snprintf(path, sizeof path, "Z:\\tmp\\neon-session-hook.%lu.log",
                 (unsigned long)GetCurrentProcessId());
        g_log = fopen(path, "a");
        logmsg("hook-session attached pid=%lu", (unsigned long)GetCurrentProcessId());

        g_token_map = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE,
                                         0, TOKEN_SIZE, TOKEN_SHARED_NAME);
        if (g_token_map)
            g_token_ptr = (char *)MapViewOfFile(g_token_map, FILE_MAP_ALL_ACCESS, 0, 0, TOKEN_SIZE);
        InitializeCriticalSection(&g_cs);

        install_hooks();
    } else if (reason == DLL_PROCESS_DETACH) {
        if (g_token_ptr) { UnmapViewOfFile(g_token_ptr); g_token_ptr = NULL; }
        if (g_token_map) { CloseHandle(g_token_map); g_token_map = NULL; }
        if (g_log) { logmsg("hook-session detached"); fclose(g_log); g_log = NULL; }
    }
    return TRUE;
}

/* ── Exports for neon-cmd ── */
__declspec(dllexport)
BOOL neon_get_session_token(char *out, size_t out_size) {
    if (!g_token_ptr || !out || out_size == 0) return FALSE;
    EnterCriticalSection(&g_cs);
    size_t len = strlen(g_token_ptr);
    if (len > 0 && len < out_size) {
        memcpy(out, g_token_ptr, len + 1);
        LeaveCriticalSection(&g_cs);
        return TRUE;
    }
    LeaveCriticalSection(&g_cs);
    return FALSE;
}

__declspec(dllexport)
void neon_clear_session_token(void) {
    if (g_token_ptr) {
        EnterCriticalSection(&g_cs);
        memset(g_token_ptr, 0, TOKEN_SIZE);
        LeaveCriticalSection(&g_cs);
    }
}
