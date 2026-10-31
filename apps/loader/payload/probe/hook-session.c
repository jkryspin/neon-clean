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
#include <bcrypt.h>
#pragma comment(lib, "bcrypt.lib")

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

typedef BOOL (WINAPI *CreateProcessAsUserW_fn)(
    HANDLE, LPCWSTR, LPWSTR, LPSECURITY_ATTRIBUTES, LPSECURITY_ATTRIBUTES, BOOL,
    DWORD, LPVOID, LPCWSTR, LPSTARTUPINFOW, LPPROCESS_INFORMATION);
typedef BOOL (WINAPI *CreateProcessAsUserA_fn)(
    HANDLE, LPCSTR, LPSTR, LPSECURITY_ATTRIBUTES, LPSECURITY_ATTRIBUTES, BOOL,
    DWORD, LPVOID, LPCSTR, LPSTARTUPINFOA, LPPROCESS_INFORMATION);
typedef BOOL (WINAPI *ShellExecuteExW_fn)(SHELLEXECUTEINFOW *);
typedef BOOL (WINAPI *ShellExecuteExA_fn)(SHELLEXECUTEINFOA *);

static CreateProcessAsUserW_fn g_orig_bg_w = NULL;
static CreateProcessAsUserA_fn g_orig_bg_a = NULL;
static ShellExecuteExW_fn g_orig_se_w = NULL;
static ShellExecuteExA_fn g_orig_se_a = NULL;

/* Addresses (as imported) that mean "this slot is CreateProcessW/A". */
static ULONG_PTR g_w_addrs[4];
static ULONG_PTR g_a_addrs[4];
static int g_w_count = 0, g_a_count = 0;
static BOOL g_installed = FALSE;

/* Slots we patched, with their original values, so unload can restore them. */
#define MAX_HOOKED 256
static ULONG_PTR *g_hooked_slot[MAX_HOOKED];
static ULONG_PTR  g_hooked_orig[MAX_HOOKED];
static int g_hooked_n = 0;

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
    /* The launcher passes the token as: MapleStory.exe "" -nxl NP12:us:0:<n>:<hex>
     * Take the whole non-space argument after "-nxl". */
    const char *nxl = strstr(narrow, "-nxl ");
    if (nxl) {
        nxl += 5;
        size_t j = 0;
        while (nxl[j] && nxl[j] != ' ' && j < out_size - 1) { out[j] = nxl[j]; j++; }
        out[j] = '\0';
        if (j > 0) return;
    }

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

/* ══ Launch state: mocks + client patch, applied inside CreateProcess ══
 *
 * The launcher verifies its file manifest and repairs MapleStory.exe back to
 * the clean build, so patching it before the launcher starts is a race. This
 * runs after that repair and before the image is mapped: put the mock DLLs
 * back, then apply the five anchored bytes and a valid PE checksum.
 */

#define APP_DIR   L"C:\\Nexon\\Library\\maplestorycw\\appdata"
#define EXE_PATH  APP_DIR L"\\MapleStory.exe"
#define EXE_SIZE  77813232u
#define CHK_OFF   0x1f8u

static const BYTE A_INIT[9]  = { 0x48, 0x83, 0xec, 0x28, 0xb9, 0x17, 0x00, 0x00, 0x00 };
static const BYTE A_COMP[10] = { 0x48, 0x83, 0xec, 0x28, 0xe8, 0, 0, 0, 0, 0x48 };
static const DWORD A_INIT_OFF[3] = { 0x2fa9600u, 0x2fc92a0u, 0x2fd9ba4u };
static const DWORD A_COMP_OFF = 0x2fd9c3cu;
static const DWORD RELAY_OFF  = 0x338fee0u;

static int same_file_hash(const wchar_t *a, const wchar_t *b) {
    BCRYPT_ALG_HANDLE alg = NULL;
    BCRYPT_HASH_HANDLE ha = NULL, hb = NULL;
    BYTE da[32], db[32], obj[64];
    DWORD got = 0, cb = 0;
    HANDLE f = INVALID_HANDLE_VALUE;
    int out = 0, i, ok;

    if (!BCryptOpenAlgorithmProvider(&alg, BCRYPT_SHA256_ALGORITHM, NULL, 0)) {
        for (i = 0; i < 2 && !out; i++) {
            const wchar_t *p = i ? b : a;
            BCRYPT_HASH_HANDLE *h = i ? &hb : &ha;
            BYTE *dst = i ? db : da;
            BYTE buf[65536];
            DWORD n = 0;
            f = CreateFileW(p, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
                            NULL, OPEN_EXISTING, 0, NULL);
            if (f == INVALID_HANDLE_VALUE) break;
            if (BCryptGetProperty(alg, BCRYPT_OBJECT_LENGTH, obj, sizeof obj, &cb, 0) ||
                BCryptCreateHash(alg, h, obj, cb, NULL, 0, 0)) { CloseHandle(f); break; }
            ok = 1;
            while (ok && ReadFile(f, buf, sizeof buf, &n, NULL) && n)
                ok = !BCryptHashData(*h, buf, n, 0);
            CloseHandle(f);
            if (ok) { BCryptFinishHash(*h, dst, 32, 0); out++; }
        }
    }
    if (ha) BCryptDestroyHash(ha);
    if (hb) BCryptDestroyHash(hb);
    if (alg) BCryptCloseAlgorithmProvider(alg, 0);
    return out == 2 && !memcmp(da, db, 32);
}

static void ensure_file(const wchar_t *name, const char *dir) {
    wchar_t dst[MAX_PATH], src[MAX_PATH], orig[MAX_PATH];
    swprintf(dst, MAX_PATH, L"%s\\%s", APP_DIR, name);
    swprintf(src, MAX_PATH, L"%hs\\mock-%s-win.dll", dir, name);
    swprintf(orig, MAX_PATH, L"%s.orig", dst);

    if (GetFileAttributesW(src) == INVALID_FILE_ATTRIBUTES) {
        logmsg("mock missing: %ls", src);
        return;
    }
    if (GetFileAttributesW(dst) != INVALID_FILE_ATTRIBUTES &&
        same_file_hash(dst, src)) {
        logmsg("%ls already mocked", name);
        return;
    }
    if (GetFileAttributesW(orig) == INVALID_FILE_ATTRIBUTES) {
        if (!CopyFileW(dst, orig, FALSE)) logmsg("backup %ls failed %lu", name, GetLastError());
        else logmsg("saved %ls.orig", name);
    }
    if (CopyFileW(src, dst, FALSE)) logmsg("mocked %ls", name);
    else logmsg("mock %ls failed %lu", name, GetLastError());
}

static DWORD pe_checksum(const BYTE *b, DWORD n) {
    ULONGLONG sum = 0;
    DWORD i;
    for (i = 0; i + 1 < n; i += 2) {
        if (i == CHK_OFF || i == CHK_OFF + 2) continue;
        sum += *(const WORD *)(b + i);
        sum = (sum & 0xffff) + (sum >> 16);
    }
    if (i < n) { sum += b[i]; sum = (sum & 0xffff) + (sum >> 16); }
    sum = (sum & 0xffff) + (sum >> 16);
    return (DWORD)(sum + n);
}

static int client_patch_site(BYTE *d, DWORD off, const BYTE *anchor, DWORD len) {
    if (off + len > EXE_SIZE) return 0;
    if (d[off] == 0xc3 && !memcmp(d + off + 1, anchor + 1, len - 1)) return 0;
    if (memcmp(d + off, anchor, len)) return 1;
    d[off] = 0xc3;
    logmsg("patched ret @ %#lx", off);
    return 0;
}

static void patch_client(void) {
    HANDLE mu = CreateMutexW(NULL, FALSE, L"Local\\NeonPatch");
    if (mu) WaitForSingleObject(mu, 15000);

    HANDLE f = INVALID_HANDLE_VALUE;
    BYTE *d = NULL;
    DWORD n = 0, got = 0, i;
    int bad = 0, changed = 0, tries;

    for (tries = 0; tries < 60; tries++) {
        f = CreateFileW(EXE_PATH, GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ,
                        NULL, OPEN_EXISTING, 0, NULL);
        if (f != INVALID_HANDLE_VALUE) break;
        Sleep(250);
    }
    if (f == INVALID_HANDLE_VALUE) { logmsg("open exe failed %lu", GetLastError()); goto out; }

    n = GetFileSize(f, NULL);
    if (n != EXE_SIZE) { logmsg("exe size %lu, expected %u", n, EXE_SIZE); goto out; }
    d = (BYTE *)VirtualAlloc(NULL, n, MEM_COMMIT, PAGE_READWRITE);
    if (!d) { logmsg("alloc failed"); goto out; }
    if (!ReadFile(f, d, n, &got, NULL) || got != n) { logmsg("read failed"); goto out; }

    for (i = 0; i < 3; i++) bad |= client_patch_site(d, A_INIT_OFF[i], A_INIT, sizeof A_INIT);
    bad |= client_patch_site(d, A_COMP_OFF, A_COMP, sizeof A_COMP);

    if (!memcmp(d + RELAY_OFF, "127.0.0.1", 9)) {
        logmsg("relay already local");
    } else if (!memcmp(d + RELAY_OFF, "10.9.2.131", 10)) {
        memcpy(d + RELAY_OFF, "127.0.0.1", 9);
        logmsg("relay -> 127.0.0.1");
        changed = 1;
    } else {
        logmsg("relay anchor mismatch");
        bad = 1;
    }

    /* Any of the three sites changing implies a change; recompute always when valid. */
    changed |= !memcmp(d + A_INIT_OFF[0], "\xc3", 1) &&
               !(memcmp(d + A_INIT_OFF[0], A_INIT, sizeof A_INIT) == 0);
    if (bad) { logmsg("anchor check FAILED, writing anyway"); }

    *(DWORD *)(d + CHK_OFF) = 0;
    *(DWORD *)(d + CHK_OFF) = pe_checksum(d, n);

    SetFilePointer(f, 0, NULL, FILE_BEGIN);
    changed = 1;
    if (WriteFile(f, d, n, &got, NULL) && got == n) logmsg("client written, checksum %#lx",
                                                          *(DWORD *)(d + CHK_OFF));
    else logmsg("write failed %lu", GetLastError());

out:
    if (d) VirtualFree(d, 0, MEM_RELEASE);
    if (f != INVALID_HANDLE_VALUE) CloseHandle(f);
    if (mu) { ReleaseMutex(mu); CloseHandle(mu); }
}

static void ensure_launch_state(const char *app, const char *cmd) {
    char buf[17408];
    buf[0] = '\0';
    if (app) { strncat(buf, app, sizeof buf - 1); strncat(buf, " ", sizeof buf - strlen(buf) - 1); }
    if (cmd) strncat(buf, cmd, sizeof buf - strlen(buf) - 1);
    if (!contains_ci(buf, "maplestory.exe")) {
        logmsg("not the game: %.80hs", buf);
        return;
    }

    char dir[MAX_PATH] = "C:\\Users\\John\\projects\\neon-clean\\neon-probe";
    GetEnvironmentVariableA("NEON_PROBE_DIR", dir, sizeof dir);
    logmsg("launch state begin");
    ensure_file(L"grap64.dll", dir);
    ensure_file(L"nmcogame64.dll", dir);
    patch_client();
    logmsg("launch state done");
}

/* ── Hooks ── */
static BOOL WINAPI hooked_CreateProcessW(
    LPCWSTR app, LPWSTR cmd, LPSECURITY_ATTRIBUTES pa, LPSECURITY_ATTRIBUTES ta,
    BOOL inherit, DWORD flags, LPVOID env, LPCWSTR dir,
    LPSTARTUPINFOW si, LPPROCESS_INFORMATION pi) {

    char appn[4096] = {0}, cmdn[16384] = {0};
    if (app) WideCharToMultiByte(CP_UTF8, 0, app, -1, appn, sizeof appn, NULL, NULL);
    if (cmd) WideCharToMultiByte(CP_UTF8, 0, cmd, -1, cmdn, sizeof cmdn, NULL, NULL);
    logmsg("CreateProcessW app=%.300hs cmd=%.600hs", appn, cmdn);

    ensure_launch_state(appn, cmdn);
    BOOL r = g_orig_w(app, cmd, pa, ta, inherit, flags, env, dir, si, pi);
    if (r && pi) capture_game_launch(appn, cmdn, pi->dwProcessId);
    return r;
}

static BOOL WINAPI hooked_CreateProcessA(
    LPCSTR app, LPSTR cmd, LPSECURITY_ATTRIBUTES pa, LPSECURITY_ATTRIBUTES ta,
    BOOL inherit, DWORD flags, LPVOID env, LPCSTR dir,
    LPSTARTUPINFOA si, LPPROCESS_INFORMATION pi) {

    logmsg("CreateProcessA app=%.300hs", app ? app : "");
    ensure_launch_state(app, cmd);
    BOOL r = g_orig_a(app, cmd, pa, ta, inherit, flags, env, dir, si, pi);
    if (r && pi) capture_game_launch(app, cmd, pi->dwProcessId);
    return r;
}

static BOOL WINAPI hooked_CreateProcessAsUserW(
    HANDLE tok, LPCWSTR app, LPWSTR cmd, LPSECURITY_ATTRIBUTES pa,
    LPSECURITY_ATTRIBUTES ta, BOOL inherit, DWORD flags, LPVOID env,
    LPCWSTR dir, LPSTARTUPINFOW si, LPPROCESS_INFORMATION pi) {

    char appn[4096] = {0}, cmdn[16384] = {0};
    if (app) WideCharToMultiByte(CP_UTF8, 0, app, -1, appn, sizeof appn, NULL, NULL);
    if (cmd) WideCharToMultiByte(CP_UTF8, 0, cmd, -1, cmdn, sizeof cmdn, NULL, NULL);
    logmsg("CreateProcessAsUserW app=%.300hs cmd=%.600hs", appn, cmdn);

    ensure_launch_state(appn, cmdn);
    if (!g_orig_bg_w) g_orig_bg_w = (CreateProcessAsUserW_fn)GetProcAddress(
        GetModuleHandleA("advapi32.dll"), "CreateProcessAsUserW");
    BOOL r = g_orig_bg_w(tok, app, cmd, pa, ta, inherit, flags, env, dir, si, pi);
    if (r && pi) capture_game_launch(appn, cmdn, pi->dwProcessId);
    return r;
}

static BOOL WINAPI hooked_CreateProcessAsUserA(
    HANDLE tok, LPCSTR app, LPSTR cmd, LPSECURITY_ATTRIBUTES pa,
    LPSECURITY_ATTRIBUTES ta, BOOL inherit, DWORD flags, LPVOID env,
    LPCSTR dir, LPSTARTUPINFOA si, LPPROCESS_INFORMATION pi) {

    logmsg("CreateProcessAsUserA app=%.300hs", app ? app : "");
    ensure_launch_state(app, cmd);
    if (!g_orig_bg_a) g_orig_bg_a = (CreateProcessAsUserA_fn)GetProcAddress(
        GetModuleHandleA("advapi32.dll"), "CreateProcessAsUserA");
    BOOL r = g_orig_bg_a(tok, app, cmd, pa, ta, inherit, flags, env, dir, si, pi);
    if (r && pi) capture_game_launch(app, cmd, pi->dwProcessId);
    return r;
}

static BOOL WINAPI hooked_ShellExecuteExW(SHELLEXECUTEINFOW *sei) {
    char f[4096] = {0}, p[16384] = {0};
    if (sei && sei->lpFile) WideCharToMultiByte(CP_UTF8, 0, sei->lpFile, -1, f, sizeof f, NULL, NULL);
    if (sei && sei->lpParameters) WideCharToMultiByte(CP_UTF8, 0, sei->lpParameters, -1, p, sizeof p, NULL, NULL);
    logmsg("ShellExecuteExW file=%.300hs params=%.600hs", f, p);
    ensure_launch_state(f, p);
    if (!g_orig_se_w) g_orig_se_w = (ShellExecuteExW_fn)GetProcAddress(
        GetModuleHandleA("shell32.dll"), "ShellExecuteExW");
    BOOL r = g_orig_se_w(sei);
    if (r && sei && sei->hProcess) capture_game_launch(f, p, GetProcessId(sei->hProcess));
    return r;
}

static BOOL WINAPI hooked_ShellExecuteExA(SHELLEXECUTEINFOA *sei) {
    logmsg("ShellExecuteExA file=%.300hs", sei && sei->lpFile ? sei->lpFile : "");
    ensure_launch_state(sei ? sei->lpFile : NULL, sei ? sei->lpParameters : NULL);
    if (!g_orig_se_a) g_orig_se_a = (ShellExecuteExA_fn)GetProcAddress(
        GetModuleHandleA("shell32.dll"), "ShellExecuteExA");
    BOOL r = g_orig_se_a(sei);
    if (r && sei && sei->hProcess) capture_game_launch(sei->lpFile, sei->lpParameters,
                                                       GetProcessId(sei->hProcess));
    return r;
}

/* Patch CreateProcessAsUserW/A slots in a module's ADVAPI32 delay-import table.
 * The launcher spawns the game with CreateProcessAsUserW, which bypasses the
 * normal kernel32 imports entirely. ResolveDelayLoadedAPI never overwrites a
 * non-null slot, so patching it here is stable. */
static void hook_delay_module(HMODULE mod, const char *name) {
    BYTE *b = (BYTE *)mod;
    if (!b || b[0] != 'M' || b[1] != 'Z') return;
    IMAGE_DOS_HEADER *dos = (IMAGE_DOS_HEADER *)b;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(b + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) return;
    IMAGE_DATA_DIRECTORY *dir =
        &nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT];
    if (!dir->VirtualAddress || !dir->Size) return;

    IMAGE_DELAYLOAD_DESCRIPTOR *dd = (IMAGE_DELAYLOAD_DESCRIPTOR *)(b + dir->VirtualAddress);
    int patched = 0;
    for (; dd->DllNameRVA; dd++) {
        ULONG_PTR vbase = dd->Attributes.RvaBased ? (ULONG_PTR)b : 0;
        const char *dll = (const char *)(vbase + dd->DllNameRVA);
        if (_stricmp(dll, "advapi32.dll") && _stricmp(dll, "shell32.dll")) continue;
        IMAGE_THUNK_DATA *nit = (IMAGE_THUNK_DATA *)(vbase + dd->ImportNameTableRVA);
        IMAGE_THUNK_DATA *iat = (IMAGE_THUNK_DATA *)(vbase + dd->ImportAddressTableRVA);
        for (; nit->u1.AddressOfData; nit++, iat++) {
            IMAGE_IMPORT_BY_NAME *imp = (IMAGE_IMPORT_BY_NAME *)(vbase + nit->u1.AddressOfData);
            void *want = NULL;
            if (!_stricmp((char *)imp->Name, "CreateProcessAsUserW")) want = hooked_CreateProcessAsUserW;
            else if (!_stricmp((char *)imp->Name, "CreateProcessAsUserA")) want = hooked_CreateProcessAsUserA;
            else if (!_stricmp((char *)imp->Name, "ShellExecuteExW")) want = hooked_ShellExecuteExW;
            else if (!_stricmp((char *)imp->Name, "ShellExecuteExA")) want = hooked_ShellExecuteExA;
            if (!want) continue;
            DWORD old;
            if (!VirtualProtect(iat, sizeof *iat, PAGE_READWRITE, &old)) continue;
            if ((void *)iat->u1.Function == want) { VirtualProtect(iat, sizeof *iat, old, &old); continue; }
            if (g_hooked_n < MAX_HOOKED) {
                g_hooked_slot[g_hooked_n] = (ULONG_PTR *)iat;
                g_hooked_orig[g_hooked_n] = iat->u1.Function;
                g_hooked_n++;
            }
            iat->u1.Function = (ULONG_PTR)want;
            VirtualProtect(iat, sizeof *iat, old, &old);
            patched++;
        }
    }
    if (patched) logmsg("delay-hooked %d slot(s) in %s", patched, name ? name : "?");
}

/* ── IAT patching ── */
static void hook_module(HMODULE mod, const char *name, int is_kernel) {
    BYTE *b = (BYTE *)mod;
    if (!b || b[0] != 'M' || b[1] != 'Z') return;
    IMAGE_DOS_HEADER *dos = (IMAGE_DOS_HEADER *)b;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(b + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) return;

    IMAGE_DATA_DIRECTORY imp =
        nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (!imp.VirtualAddress) return;

    /* kernel32/kernelbase implement CreateProcessW by forwarding through their
     * own IAT entry. If we patched that entry, calling the original (the
     * forwarder) would bounce straight back into the hook -> infinite recursion.
     * Leave those two modules' own slots alone and trampoline to the real body. */
    if (is_kernel) return;

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
                    if (g_hooked_n < MAX_HOOKED) {
                        g_hooked_slot[g_hooked_n] = slot;
                        g_hooked_orig[g_hooked_n] = *slot;
                        g_hooked_n++;
                    }
                    *slot = (ULONG_PTR)want;
                    VirtualProtect(slot, sizeof(void *), old, &old);
                    patched++;
                }
            }
        }
    }
    if (patched) logmsg("iat-hooked %d slot(s) in %s", patched, name ? name : "?");
}

static void unhook_all(void) {
    for (int i = 0; i < g_hooked_n; i++) {
        DWORD old;
        if (VirtualProtect(g_hooked_slot[i], sizeof(void *), PAGE_READWRITE, &old)) {
            *g_hooked_slot[i] = g_hooked_orig[i];
            VirtualProtect(g_hooked_slot[i], sizeof(void *), old, &old);
        }
    }
    logmsg("restored %d slot(s)", g_hooked_n);
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
    /* kernelbase is already loaded on modern Wine; GetModuleHandle avoids a
     * LoadLibrary call while we may still be inside the loader lock. */
    HMODULE kbase = GetModuleHandleA("kernelbase.dll");
    add_addr(g_w_addrs, &g_w_count, k32, "CreateProcessW");
    add_addr(g_w_addrs, &g_w_count, kbase, "CreateProcessW");
    add_addr(g_a_addrs, &g_a_count, k32, "CreateProcessA");
    add_addr(g_a_addrs, &g_a_count, kbase, "CreateProcessA");
    /* Trampoline to the real implementation, not kernel32's forwarder. */
    g_orig_w = (CreateProcessW_fn)GetProcAddress(kbase ? kbase : k32, "CreateProcessW");
    g_orig_a = (CreateProcessA_fn)GetProcAddress(kbase ? kbase : k32, "CreateProcessA");

    HMODULE adv = LoadLibraryA("advapi32.dll");
    HMODULE sh = LoadLibraryA("shell32.dll");
    if (adv) {
        g_orig_bg_w = (CreateProcessAsUserW_fn)GetProcAddress(adv, "CreateProcessAsUserW");
        g_orig_bg_a = (CreateProcessAsUserA_fn)GetProcAddress(adv, "CreateProcessAsUserA");
    }
    if (sh) {
        g_orig_se_w = (ShellExecuteExW_fn)GetProcAddress(sh, "ShellExecuteExW");
        g_orig_se_a = (ShellExecuteExA_fn)GetProcAddress(sh, "ShellExecuteExA");
    }

    HMODULE mods[1024];
    DWORD need = 0;
    if (!EnumProcessModules(GetCurrentProcess(), mods, sizeof mods, &need)) {
        logmsg("EnumProcessModules failed %lu", GetLastError());
        return;
    }
    for (DWORD i = 0; i < need / sizeof(HMODULE); i++) {
        char nm[MAX_PATH] = {0};
        GetModuleBaseNameA(GetCurrentProcess(), mods[i], nm, sizeof nm);
        int is_kernel = !_stricmp(nm, "kernel32.dll") || !_stricmp(nm, "kernelbase.dll");
        hook_module(mods[i], nm, is_kernel);
        hook_delay_module(mods[i], nm);
    }
    logmsg("install complete (w=%d a=%d)", g_w_count, g_a_count);
}

/* Run the install on its own thread: EnumProcessModules/psapi can load DLLs,
 * which is unsafe from inside DllMain's loader lock. */
static DWORD WINAPI install_thread(LPVOID p) {
    (void)p;
    install_hooks();
    return 0;
}

/* ── DllMain ── */
BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);
        char path[MAX_PATH];
        CreateDirectoryW(L"C:\\Neon", NULL);
        CreateDirectoryW(L"C:\\Neon\\logs", NULL);
        snprintf(path, sizeof path, "C:\\Neon\\logs\\neon-session-hook.%lu.log",
                 (unsigned long)GetCurrentProcessId());
        g_log = fopen(path, "a");
        logmsg("hook-session attached pid=%lu", (unsigned long)GetCurrentProcessId());

        g_token_map = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE,
                                         0, TOKEN_SIZE, TOKEN_SHARED_NAME);
        if (g_token_map)
            g_token_ptr = (char *)MapViewOfFile(g_token_map, FILE_MAP_ALL_ACCESS, 0, 0, TOKEN_SIZE);
        InitializeCriticalSection(&g_cs);

        CreateThread(NULL, 0, install_thread, NULL, 0, NULL);
    } else if (reason == DLL_PROCESS_DETACH) {
        if (g_installed) unhook_all();
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
