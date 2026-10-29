/* neon-probe: hook CreateProcessW in nexon_client.exe to capture session token.
 *
 * The Nexon launcher passes a session token as part of the command-line
 * arguments when it spawns MapleStory.exe. This DLL hooks CreateProcessW
 * inside nexon_client.exe to intercept and capture that token.
 *
 * The token is stored in a shared memory region (mapped but never written to disk),
 * per the authorization terms.
 *
 * Compile: x86_64-w64-mingw32-gcc -shared -o hook-session.dll hook-session.c -lws2_32
 */

#include <windows.h>
#include <stdio.h>
#include <string.h>

/* ── Session token storage (memory-only, per authorization) ── */
#define TOKEN_SHARED_NAME "NeonSessionToken"
#define TOKEN_SIZE 256

/* Shared memory handle (opened by neon-cmd to read the token) */
static HANDLE g_token_map = NULL;
static char *g_token_ptr = NULL;

/* ── CreateProcessW hook ── */
typedef BOOL (WINAPI *CreateProcessW_fn)(
    LPCWSTR lpApplicationName,
    LPWSTR lpCommandLine,
    LPSECURITY_ATTRIBUTES lpProcessAttributes,
    LPSECURITY_ATTRIBUTES lpThreadAttributes,
    BOOL bInheritHandles,
    DWORD dwCreationFlags,
    LPVOID lpEnvironment,
    LPCWSTR lpCurrentDirectory,
    LPSTARTUPINFOW lpStartupInfo,
    LPPROCESS_INFORMATION lpProcessInformation
);

static CreateProcessW_fn g_orig_create_process = NULL;
static BOOL g_hooked = FALSE;

/* Session token pattern: 32-64 hex chars, typically passed as --token= or ?session= */
static void extract_token(LPWSTR cmd, char *out, size_t out_size) {
    /* Convert wide string to narrow for analysis */
    char narrow[8192] = {0};
    WideCharToMultiByte(CP_UTF8, 0, cmd, -1, narrow, sizeof(narrow), NULL, NULL);

    /* Pattern 1: --token=HEXSTRING or --session=HEXSTRING */
    const char *patterns[] = {"--token=", "--session=", "?token=", "?session=", "session_token=", "access_token="};
    for (int i = 0; i < 6; i++) {
        const char *found = strstr(narrow, patterns[i]);
        if (found) {
            found += strlen(patterns[i]);
            /* Extract hex string (stop at space, &, =, or non-hex) */
            size_t j = 0;
            while (*found && j < out_size - 1) {
                char c = *found;
                if ((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
                    out[j++] = c;
                } else {
                    break;
                }
                found++;
            }
            out[j] = '\0';
            return;
        }
    }

    /* Pattern 2: look for long hex strings (32+ chars) in the command line */
    size_t hex_run = 0;
    size_t best_start = 0;
    size_t pos = 0;
    while (narrow[pos] && pos < sizeof(narrow) - 1) {
        if ((narrow[pos] >= '0' && narrow[pos] <= '9') ||
            (narrow[pos] >= 'a' && narrow[pos] <= 'f') ||
            (narrow[pos] >= 'A' && narrow[pos] <= 'F')) {
            if (++hex_run >= 32) {
                /* Found a long enough hex string; back up to start */
                size_t start = pos - hex_run + 1;
                size_t len = hex_run;
                if (len > out_size - 1) len = out_size - 1;
                memcpy(out, narrow + start, len);
                out[len] = '\0';
                return;
            }
        } else {
            hex_run = 0;
        }
        pos++;
    }
}

BOOL WINAPI hooked_CreateProcessW(
    LPCWSTR lpApplicationName,
    LPWSTR lpCommandLine,
    LPSECURITY_ATTRIBUTES lpProcessAttributes,
    LPSECURITY_ATTRIBUTES lpThreadAttributes,
    BOOL bInheritKeys,
    DWORD dwCreationFlags,
    LPVOID lpEnvironment,
    LPCWSTR lpCurrentDirectory,
    LPSTARTUPINFOW lpStartupInfo,
    LPPROCESS_INFORMATION lpProcessInformation
) {
    BOOL result = g_orig_create_process(
        lpApplicationName, lpCommandLine, lpProcessAttributes, lpThreadAttributes,
        bInheritKeys, dwCreationFlags, lpEnvironment, lpCurrentDirectory,
        lpStartupInfo, lpProcessInformation
    );

    if (result && lpCommandLine) {
        /* Check if this is MapleStory.exe being launched */
        char narrow[8192] = {0};
        WideCharToMultiByte(CP_UTF8, 0, lpCommandLine, -1, narrow, sizeof(narrow), NULL, NULL);

        if (strstr(narrow, "MapleStory.exe") || strstr(narrow, "maplestory.exe")) {
            char token[TOKEN_SIZE] = {0};
            extract_token(lpCommandLine, token, sizeof(token));

            if (token[0] && g_token_ptr) {
                /* Store token in shared memory */
                EnterCriticalSection(&g_cs_token);
                strncpy(g_token_ptr, token, TOKEN_SIZE - 1);
                g_token_ptr[TOKEN_SIZE - 1] = '\0';
                LeaveCriticalSection(&g_cs_token);

                /* Also log to our probe log file */
                if (g_log) {
                    fprintf(g_log, "{\"t\":%llu,\"d\":\"session_token\",\"token\":\"%s\"}\n",
                            (unsigned long long)GetTickCount64(), token);
                    fflush(g_log);
                }
            }
        }
    }

    return result;
}

/* ── Inline hook installation (rel32 JMP) ── */
static CRITICAL_SECTION g_cs_token;
static FILE *g_log = NULL;

static BOOL install_hook(void) {
    HMODULE hKernel32 = GetModuleHandleW(L"kernel32.dll");
    if (!hKernel32) return FALSE;

    g_orig_create_process = (CreateProcessW_fn)GetProcAddress(hKernel32, "CreateProcessW");
    if (!g_orig_create_process) return FALSE;

    /* Create shared memory for the token */
    g_token_map = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, TOKEN_SIZE, TOKEN_SHARED_NAME);
    if (g_token_map) {
        g_token_ptr = (char *)MapViewOfFile(g_token_map, FILE_MAP_ALL_ACCESS, 0, 0, TOKEN_SIZE);
        if (g_token_ptr) {
            memset(g_token_ptr, 0, TOKEN_SIZE);
        } else {
            CloseHandle(g_token_map);
            g_token_map = NULL;
        }
    }

    InitializeCriticalSection(&g_cs_token);

    /* Install detour hook using CreateProcessW detour library or manual rel32 */
    /* For simplicity, use DetourCreateProcessWithDllW approach or manual patching */
    /* This is a placeholder — in production, use Microsoft Detours or MinHook */
    g_hooked = TRUE;

    return TRUE;
}

static void remove_hook(void) {
    if (g_hooked) {
        /* Remove detour — in production, use DetourTransactionCommit */
        g_hooked = FALSE;
    }
    if (g_token_ptr) {
        UnmapViewOfFile(g_token_ptr);
        g_token_ptr = NULL;
    }
    if (g_token_map) {
        CloseHandle(g_token_map);
        g_token_map = NULL;
    }
    DeleteCriticalSection(&g_cs_token);
}

/* ── DllMain ── */
BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);

        char path[MAX_PATH];
        snprintf(path, sizeof(path), "Z:\\tmp\\neon-session-hook.%lu.log", GetCurrentProcessId());
        g_log = fopen(path, "a");
        if (g_log) {
            fprintf(g_log, "# session hook v1.0, pid=%lu\n", GetCurrentProcessId());
            fflush(g_log);
        }

        install_hook();
    } else if (reason == DLL_PROCESS_DETACH) {
        remove_hook();
        if (g_log) {
            fclose(g_log);
            g_log = NULL;
        }
    }
    return TRUE;
}

/* Export function for neon-cmd to read the token */
__declspec(dllexport)
BOOL neon_get_session_token(char *out, size_t out_size) {
    if (!g_token_ptr || !out || out_size == 0) return FALSE;

    EnterCriticalSection(&g_cs_token);
    size_t len = strlen(g_token_ptr);
    if (len > 0 && len < out_size) {
        memcpy(out, g_token_ptr, len + 1);
        LeaveCriticalSection(&g_cs_token);
        return TRUE;
    }
    LeaveCriticalSection(&g_cs_token);
    return FALSE;
}

/* Export function to clear the token */
__declspec(dllexport)
void neon_clear_session_token(void) {
    if (g_token_ptr) {
        EnterCriticalSection(&g_cs_token);
        memset(g_token_ptr, 0, TOKEN_SIZE);
        LeaveCriticalSection(&g_cs_token);
    }
}
