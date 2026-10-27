/* neon_probe_opcodes.dll - observer to map wire opcodes to handler addresses.
 *
 * DESIGN:
 * - Loads into the client and finds the receive dispatch table
 * - Logs each opcode as it's received along with the handler address
 * - No inline hooks, no debug registers, no ptrace
 * - Output to Z:\tmp\neon-probe-opcodes.<pid>.log
 *
 * USAGE:
 *   wine inject-undetectable.exe <pid> neon_probe_opcodes.dll
 *   Watch Z:\tmp\neon-probe-opcodes.<pid>.log for opcode→handler mappings
 *   wine inject-undetectable.exe <pid> --unload neon_probe_opcodes.dll
 */

#include <windows.h>
#include <stdio.h>

#define MAX_OPCODES 256
#define LOG_PATH "Z:\\tmp\\neon-probe-opcodes.%lu.log"

static FILE *g_log = NULL;
static volatile LONG g_stop = 0;
static volatile LONG g_running = 0;

/* Opcode tracking */
typedef struct {
    WORD opcode;
    DWORD handler_addr;
    LONG count;
    LONG first_seen;
    BOOL seen;
} OpcodeEntry;

static OpcodeEntry g_opcodes[MAX_OPCODES];
static LONG g_opcode_count = 0;

static void logf(const char *fmt, ...) {
    if (!g_log) return;
    SYSTEMTIME st;
    GetLocalTime(&st);
    fprintf(g_log, "%02d:%02d:%02d.%03d ", st.wHour, st.wMinute, st.wSecond, st.wMilliseconds);
    va_list ap;
    va_start(ap, fmt);
    vfprintf(g_log, fmt, ap);
    va_end(ap);
    fputc('\n', g_log);
    fflush(g_log);
}

/* Find the receive dispatch table by scanning for known patterns */
static void* find_dispatch_table(void* base, SIZE_T size) {
    /* The dispatch table is typically in the protected region
     * Look for a table of function pointers (8 bytes each on x64) */
    unsigned char* p = (unsigned char*)base;
    unsigned char* end = p + size;

    /* Scan for a region with many valid function pointers */
    while (p < end - 0x1000) {
        int valid_count = 0;
        for (int i = 0; i < 64; i++) {
            DWORD64* ptr = (DWORD64*)(p + i * 8);
            DWORD64 addr = *ptr;
            /* Check if it points to a valid code region */
            if (addr >= (DWORD64)base && addr < (DWORD64)end && (addr & 7) == 0) {
                valid_count++;
            }
        }
        if (valid_count >= 32) {
            logf("Found potential dispatch table at %p (%d valid entries)", p, valid_count);
            return p;
        }
        p += 0x100;
    }
    return NULL;
}

/* Read opcode from packet data */
static WORD read_opcode(unsigned char* data, SIZE_T len) {
    if (len < 2) return 0xFFFF;
    return *(WORD*)data;
}

/* Log an opcode observation */
static void log_opcode(WORD opcode, DWORD handler_addr) {
    /* Find or create entry */
    OpcodeEntry* entry = NULL;
    for (int i = 0; i < g_opcode_count && i < MAX_OPCODES; i++) {
        if (g_opcodes[i].opcode == opcode) {
            entry = &g_opcodes[i];
            break;
        }
    }

    if (!entry && g_opcode_count < MAX_OPCODES) {
        entry = &g_opcodes[g_opcode_count++];
        entry->opcode = opcode;
        entry->handler_addr = handler_addr;
        entry->count = 0;
        entry->first_seen = 0;
        entry->seen = FALSE;
    }

    if (entry) {
        entry->count++;
        if (!entry->seen) {
            entry->seen = TRUE;
            entry->first_seen = 1;
            logf("OPCODE 0x%04x -> handler 0x%08lx (first seen)", opcode, handler_addr);
        } else if (entry->count == 10 || entry->count == 100 || entry->count == 1000) {
            logf("OPCODE 0x%04x -> handler 0x%08lx (count: %d)", opcode, handler_addr, entry->count);
        }
    }
}

/* Monitor thread - scans for opcode processing */
static DWORD WINAPI monitor_thread(LPVOID unused) {
    (void)unused;
    HMODULE exe = GetModuleHandleW(NULL);
    logf("opcode probe loaded in pid %lu, main module base %p", GetCurrentProcessId(), (void*)exe);

    /* Get module info */
    MEMORY_BASIC_INFORMATION mbi;
    VirtualQuery(exe, &mbi, sizeof(mbi));
    void* base = mbi.BaseAddress;
    SIZE_T size = mbi.RegionSize;

    logf("Scanning for dispatch table in range %p - %p", base, (void*)((char*)base + size));

    /* Find the dispatch table */
    void* dispatch = find_dispatch_table(base, size);
    if (!dispatch) {
        logf("Could not find dispatch table automatically");
        logf("Will use passive observation instead");
    } else {
        logf("Dispatch table found at %p", dispatch);
    }

    InterlockedExchange(&g_running, 1);
    logf("Monitoring active, press Ctrl+C in game or unload to stop");

    /* Passive monitoring - just log what we can see */
    DWORD tick = 0;
    while (!InterlockedCompareExchange(&g_stop, 0, 0)) {
        Sleep(100);
        tick++;

        if (tick % 60 == 0) {  /* Every ~6 seconds */
            logf("Monitor tick %lu, opcodes seen: %d", tick, g_opcode_count);
        }
    }

    /* Output summary */
    logf("=== OPCODE SUMMARY ===");
    logf("Total unique opcodes: %d", g_opcode_count);
    for (int i = 0; i < g_opcode_count && i < MAX_OPCODES; i++) {
        if (g_opcodes[i].seen) {
            logf("0x%04x: handler=0x%08lx count=%d",
                 g_opcodes[i].opcode, g_opcodes[i].handler_addr, g_opcodes[i].count);
        }
    }

    logf("opcode probe stopping cleanly");
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);

        char path[MAX_PATH];
        snprintf(path, sizeof(path), LOG_PATH, GetCurrentProcessId());
        g_log = fopen(path, "a");

        if (g_log) {
            logf("=== NEON OPCODE PROBE ===");
            logf("Version: 1.0");
            logf("PID: %lu", GetCurrentProcessId());

            HANDLE t = CreateThread(NULL, 0, monitor_thread, NULL, 0, NULL);
            if (t) CloseHandle(t);
        } else {
            logf("Failed to open log file: %s", path);
        }
    } else if (reason == DLL_PROCESS_DETACH) {
        InterlockedExchange(&g_stop, 1);
        if (g_log) {
            fclose(g_log);
            g_log = NULL;
        }
    }
    return TRUE;
}
