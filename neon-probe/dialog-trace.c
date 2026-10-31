/* dialog-trace.dll - find who shows the startup "game file damaged" box.
 *
 * Hooks MessageBoxW/A, CreateDialogParamW/A, DialogBoxParamW/A and
 * TaskDialogIndirect in every module's imports (normal and delay), logs the
 * caller's return address and the text, then calls through. No code bytes are
 * patched in the process except import slots. Writes C:\Neon\logs\dialog.<pid>.log.
 *
 * Build: cl /nologo /LD /MT /O2 /D_WIN32_WINNT=0x0600 dialog-trace.c /link psapi.lib
 */
#include <windows.h>
#include <psapi.h>
#include <stdio.h>
#include <string.h>
#include <intrin.h>

typedef int (WINAPI *MessageBoxW_fn)(HWND, LPCWSTR, LPCWSTR, UINT);
typedef int (WINAPI *MessageBoxA_fn)(HWND, LPCSTR, LPCSTR, UINT);
typedef INT_PTR (WINAPI *DlgParamW_fn)(HINSTANCE, LPCWSTR, HWND, DLGPROC, LPARAM);
typedef INT_PTR (WINAPI *DlgParamA_fn)(HINSTANCE, LPCSTR, HWND, DLGPROC, LPARAM);
typedef HRESULT (WINAPI *TaskDialogIndirect_fn)(const void *, int *, int *, BOOL *);

static FILE *g_log;
static MessageBoxW_fn o_mbw;
static MessageBoxA_fn o_mba;
static DlgParamW_fn o_cdpw, o_dbpw;
static DlgParamA_fn o_cdpa, o_dbpa;
static TaskDialogIndirect_fn o_tdi;
static CRITICAL_SECTION g_cs;

static void lg(const char *fmt, ...) {
    if (!g_log) return;
    EnterCriticalSection(&g_cs);
    fprintf(g_log, "{\"t\":%llu,\"m\":\"", (unsigned long long)GetTickCount64());
    va_list ap; va_start(ap, fmt); vfprintf(g_log, fmt, ap); va_end(ap);
    fprintf(g_log, "\"}\n");
    fflush(g_log);
    LeaveCriticalSection(&g_cs);
}

static const char *mod_of(void *addr) {
    static char name[MAX_PATH];
    HMODULE m = NULL;
    if (GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                           GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, (LPCSTR)addr, &m) && m) {
        GetModuleFileNameA(m, name, sizeof name);
        char *b = strrchr(name, '\\');
        return b ? b + 1 : name;
    }
    return "?";
}

static void wide2utf8(const wchar_t *w, char *out, size_t n) {
    out[0] = 0;
    if (w) WideCharToMultiByte(CP_UTF8, 0, w, -1, out, (int)n, NULL, NULL);
}

static int WINAPI h_mbw(HWND h, LPCWSTR t, LPCWSTR c, UINT f) {
    char tt[2048], cc[512];
    wide2utf8(t, tt, sizeof tt); wide2utf8(c, cc, sizeof cc);
    lg("MessageBoxW caller=%p mod=%s text=%s caption=%s", _ReturnAddress(),
       mod_of(_ReturnAddress()), tt, cc);
    return o_mbw(h, t, c, f);
}

static int WINAPI h_mba(HWND h, LPCSTR t, LPCSTR c, UINT f) {
    lg("MessageBoxA caller=%p mod=%s text=%s caption=%s", _ReturnAddress(),
       mod_of(_ReturnAddress()), t ? t : "", c ? c : "");
    return o_mba(h, t, c, f);
}

static INT_PTR WINAPI h_cdpw(HINSTANCE i, LPCWSTR n, HWND p, DLGPROC d, LPARAM l) {
    char nn[512]; wide2utf8(n, nn, sizeof nn);
    lg("CreateDialogParamW caller=%p mod=%s name=%s", _ReturnAddress(),
       mod_of(_ReturnAddress()), nn);
    return o_cdpw(i, n, p, d, l);
}

static INT_PTR WINAPI h_dbpw(HINSTANCE i, LPCWSTR n, HWND p, DLGPROC d, LPARAM l) {
    char nn[512]; wide2utf8(n, nn, sizeof nn);
    lg("DialogBoxParamW caller=%p mod=%s name=%s", _ReturnAddress(),
       mod_of(_ReturnAddress()), nn);
    return o_dbpw(i, n, p, d, l);
}

static INT_PTR WINAPI h_cdpa(HINSTANCE i, LPCSTR n, HWND p, DLGPROC d, LPARAM l) {
    lg("CreateDialogParamA caller=%p mod=%s name=%s", _ReturnAddress(),
       mod_of(_ReturnAddress()), n ? n : "");
    return o_cdpa(i, n, p, d, l);
}

static INT_PTR WINAPI h_dbpa(HINSTANCE i, LPCSTR n, HWND p, DLGPROC d, LPARAM l) {
    lg("DialogBoxParamA caller=%p mod=%s name=%s", _ReturnAddress(),
       mod_of(_ReturnAddress()), n ? n : "");
    return o_dbpa(i, n, p, d, l);
}

static HRESULT WINAPI h_tdi(const void *cfg, int *btn, int *rad, BOOL *ver) {
    lg("TaskDialogIndirect caller=%p mod=%s", _ReturnAddress(), mod_of(_ReturnAddress()));
    return o_tdi(cfg, btn, rad, ver);
}

typedef int (WINAPI *Indirect_fn)(const void *);
typedef int (WINAPI *Timeout_fn)(HWND, LPCWSTR, LPCWSTR, UINT, WORD, DWORD);
static Indirect_fn o_mbi, o_smm;
static Timeout_fn o_mbt;

static int WINAPI h_mbi(const void *p) {
    /* MSGBOXPARAMSW: cbSize, hwndOwner, hInstance, lpszText, lpszCaption, ... */
    char tt[2048] = "", cc[512] = "";
    if (p) {
        const void **a = (const void **)p;
        wide2utf8((const wchar_t *)a[3], tt, sizeof tt);
        wide2utf8((const wchar_t *)a[4], cc, sizeof cc);
    }
    lg("MessageBoxIndirectW caller=%p mod=%s text=%s caption=%s",
       _ReturnAddress(), mod_of(_ReturnAddress()), tt, cc);
    return o_mbi(p);
}

static int WINAPI h_mbt(HWND h, LPCWSTR t, LPCWSTR c, UINT f, WORD w, DWORD ms) {
    char tt[2048], cc[512];
    wide2utf8(t, tt, sizeof tt); wide2utf8(c, cc, sizeof cc);
    lg("MessageBoxTimeoutW caller=%p mod=%s text=%s caption=%s", _ReturnAddress(),
       mod_of(_ReturnAddress()), tt, cc);
    return o_mbt(h, t, c, f, w, ms);
}

static int WINAPI h_smm(const void *p) {
    lg("SoftModalMessageBox caller=%p mod=%s", _ReturnAddress(), mod_of(_ReturnAddress()));
    return o_smm(p);
}

typedef HWND (WINAPI *CWE_fn)(DWORD, LPCWSTR, LPCWSTR, DWORD, int, int, int, int,
                              HWND, HMENU, HINSTANCE, LPVOID);
static CWE_fn o_cwe;

static HWND WINAPI h_cwe(DWORD ex, LPCWSTR cls, LPCWSTR name, DWORD st, int x, int y,
                         int w, int h, HWND par, HMENU menu, HINSTANCE inst, LPVOID p) {
    if (cls && !wcscmp(cls, L"#32770")) {
        char nn[512]; wide2utf8(name, nn, sizeof nn);
        lg("CreateWindowExW(#32770) caller=%p mod=%s name=%s", _ReturnAddress(),
           mod_of(_ReturnAddress()), nn);
    }
    return o_cwe(ex, cls, name, st, x, y, w, h, par, menu, inst, p);
}

static void patch_slot(void *fn, void *hook) {
    DWORD old;
    if (!VirtualProtect(fn, sizeof(void *), PAGE_READWRITE, &old)) return;
    *(void **)fn = hook;
    VirtualProtect(fn, sizeof(void *), old, &old);
}

static ULONG_PTR g_addrs[16];
static int g_naddr;
static void *g_hooks[16];
static int g_nhook;

static void *hook_for(ULONG_PTR p) {
    for (int i = 0; i < g_naddr; i++) if (g_addrs[i] == p) return g_hooks[i];
    return NULL;
}

static void add_target(const char *dll, const char *fn, void *hook) {
    HMODULE m = GetModuleHandleA(dll);
    if (!m) m = LoadLibraryA(dll);
    if (!m) return;
    void *p = GetProcAddress(m, fn);
    if (p && g_naddr < 16) { g_addrs[g_naddr] = (ULONG_PTR)p; g_hooks[g_nhook++] = hook; g_naddr++; }
}

static void hook_module(HMODULE mod, const char *name) {
    BYTE *b = (BYTE *)mod;
    if (!b || b[0] != 'M' || b[1] != 'Z') return;
    IMAGE_DOS_HEADER *dos = (IMAGE_DOS_HEADER *)b;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(b + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) return;

    IMAGE_DATA_DIRECTORY *dir = &nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (dir->VirtualAddress) {
        IMAGE_IMPORT_DESCRIPTOR *id = (IMAGE_IMPORT_DESCRIPTOR *)(b + dir->VirtualAddress);
        for (int k = 0; k < 4096 && id->Name; k++, id++) {
            const char *dll = (const char *)(b + id->Name);
            if (_stricmp(dll, "user32.dll") && _stricmp(dll, "comctl32.dll") &&
                _stricmp(dll, "shell32.dll")) continue;
            IMAGE_THUNK_DATA *iat = (IMAGE_THUNK_DATA *)(b + id->FirstThunk);
            for (int i = 0; i < 20000 && iat->u1.Function; i++, iat++) {
                void *want = hook_for(iat->u1.Function);
                if (want) patch_slot(iat, want);
            }
        }
    }

    dir = &nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT];
    if (!dir->VirtualAddress) return;
    IMAGE_DELAYLOAD_DESCRIPTOR *dd = (IMAGE_DELAYLOAD_DESCRIPTOR *)(b + dir->VirtualAddress);
    for (; dd->DllNameRVA; dd++) {
        ULONG_PTR vb = dd->Attributes.RvaBased ? (ULONG_PTR)b : 0;
        const char *dll = (const char *)(vb + dd->DllNameRVA);
        if (_stricmp(dll, "user32.dll") && _stricmp(dll, "comctl32.dll") &&
            _stricmp(dll, "shell32.dll")) continue;
        if (!dd->ImportNameTableRVA) continue;
        IMAGE_THUNK_DATA *t = (IMAGE_THUNK_DATA *)(vb + dd->ImportNameTableRVA);
        IMAGE_THUNK_DATA *iat = (IMAGE_THUNK_DATA *)(vb + dd->ImportAddressTableRVA);
        for (int i = 0; i < 20000 && t->u1.AddressOfData; i++, t++, iat++) {
            if (IMAGE_SNAP_BY_ORDINAL(t->u1.Ordinal)) continue;
            IMAGE_IMPORT_BY_NAME *nm = (IMAGE_IMPORT_BY_NAME *)(vb + t->u1.AddressOfData);
            void *want = NULL;
            if (!_stricmp((char *)nm->Name, "MessageBoxW")) want = h_mbw;
            else if (!_stricmp((char *)nm->Name, "MessageBoxA")) want = h_mba;
            else if (!_stricmp((char *)nm->Name, "CreateDialogParamW")) want = h_cdpw;
            else if (!_stricmp((char *)nm->Name, "DialogBoxParamW")) want = h_dbpw;
            else if (!_stricmp((char *)nm->Name, "TaskDialogIndirect")) want = h_tdi;
            if (want && (void *)iat->u1.Function != want) patch_slot(iat, want);
        }
    }
}

static DWORD WINAPI install(LPVOID p) {
    (void)p;
    HMODULE u = LoadLibraryA("user32.dll");
    HMODULE c = LoadLibraryA("comctl32.dll");
    if (u) {
        o_mbw = (MessageBoxW_fn)GetProcAddress(u, "MessageBoxW");
        o_mba = (MessageBoxA_fn)GetProcAddress(u, "MessageBoxA");
        o_cdpw = (DlgParamW_fn)GetProcAddress(u, "CreateDialogParamW");
        o_dbpw = (DlgParamW_fn)GetProcAddress(u, "DialogBoxParamW");
        o_cdpa = (DlgParamA_fn)GetProcAddress(u, "CreateDialogParamA");
        o_dbpa = (DlgParamA_fn)GetProcAddress(u, "DialogBoxParamA");
        o_mbi = (Indirect_fn)GetProcAddress(u, "MessageBoxIndirectW");
        o_mbt = (Timeout_fn)GetProcAddress(u, "MessageBoxTimeoutW");
        o_smm = (Indirect_fn)GetProcAddress(u, "SoftModalMessageBox");
        o_cwe = (CWE_fn)GetProcAddress(u, "CreateWindowExW");
    }
    if (c) o_tdi = (TaskDialogIndirect_fn)GetProcAddress(c, "TaskDialogIndirect");

    add_target("user32.dll", "MessageBoxW", h_mbw);
    add_target("user32.dll", "MessageBoxA", h_mba);
    add_target("user32.dll", "CreateDialogParamW", h_cdpw);
    add_target("user32.dll", "DialogBoxParamW", h_dbpw);
    add_target("user32.dll", "CreateDialogParamA", h_cdpa);
    add_target("user32.dll", "DialogBoxParamA", h_dbpa);
    add_target("comctl32.dll", "TaskDialogIndirect", h_tdi);
    add_target("user32.dll", "MessageBoxIndirectW", h_mbi);
    add_target("user32.dll", "MessageBoxTimeoutW", h_mbt);
    add_target("user32.dll", "SoftModalMessageBox", h_smm);
    add_target("user32.dll", "CreateWindowExW", h_cwe);

    HMODULE mods[1024]; DWORD need = 0;
    if (!EnumProcessModules(GetCurrentProcess(), mods, sizeof mods, &need)) return 0;
    for (DWORD i = 0; i < need / sizeof(HMODULE); i++) {
        char nm[MAX_PATH] = {0};
        GetModuleBaseNameA(GetCurrentProcess(), mods[i], nm, sizeof nm);
        lg("scan %lu %s", i, nm);
        hook_module(mods[i], nm);
    }
    lg("install done");
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID r) {
    (void)r;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);
        CreateDirectoryW(L"C:\\Neon", NULL);
        CreateDirectoryW(L"C:\\Neon\\logs", NULL);
        char path[MAX_PATH];
        snprintf(path, sizeof path, "C:\\Neon\\logs\\dialog.%lu.log", GetCurrentProcessId());
        g_log = fopen(path, "a");
        InitializeCriticalSection(&g_cs);
        lg("attach pid=%lu", GetCurrentProcessId());
        CreateThread(NULL, 0, install, NULL, 0, NULL);
    }
    return TRUE;
}
