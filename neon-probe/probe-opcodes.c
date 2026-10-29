/* neon-probe: in-process wire opcode mapping via inline detour hook.
 *
 * v4.1 — proper inline hook with nearby springboard.
 *
 * Previous approaches that failed:
 *   v2.0 PAGE_GUARD: Wine doesn't fire guard faults for code execution.
 *   v3.0 INT3+VEH: Wine/GRAP swallows EXCEPTION_BREAKPOINT before VEH.
 *   v4.0 inline hook: trampoline too far from target for rel32 CALL fixup.
 *
 * This version:
 *   1. Allocates a code page within ±2GB of the target function
 *   2. Builds a springboard (abs JMP to hook) + trampoline (5 saved bytes
 *      + rel JMP back) on that nearby page
 *   3. Patches only 5 bytes at the target (rel32 JMP to springboard)
 *   4. First instruction is exactly 5 bytes (mov [rsp+8],rcx) — clean cut
 *
 * OUTPUT: Z:\tmp\neon-probe-opcodes.<pid>.jsonl
 */

#include <windows.h>
#include <psapi.h>
#include <stdio.h>
#include <time.h>

/* ── Addresses from Classic World v1.0.1 ── */
#define PKT_SEND_FUNC     0x141660360ULL

/* Packet object layout (from outraw.py / outbuild.py) */
#define PKT_PAYLOAD_OFF   8
#define PKT_LENGTH_OFF    0x428

/* ── Configuration ── */
#define LOG_PATH          "Z:\\tmp\\neon-probe-opcodes.%lu.jsonl"
#define STOLEN_BYTES      5   /* mov [rsp+8], rcx = 48 89 4c 24 08 */

/* ── Global state ── */
static FILE *g_log = NULL;
static volatile LONG g_out_count = 0;
static BYTE g_saved_prologue[STOLEN_BYTES];
static BOOL g_hooked = FALSE;
static LPVOID g_nearby_page = NULL;

/* typedef for calling the original function via trampoline */
typedef void (__attribute__((ms_abi)) *pkt_send_fn)(void *pkt_obj);
static pkt_send_fn g_trampoline_fn = NULL;

/* ── Logging ── */
static void probe_log(const char *fmt, ...) {
    if (!g_log) return;
    va_list ap;
    va_start(ap, fmt);
    vfprintf(g_log, fmt, ap);
    va_end(ap);
    fputc('\n', g_log);
    fflush(g_log);
}

/* ── Safe memory read ── */
static BOOL safe_read(DWORD64 addr, void *buf, SIZE_T len) {
    SIZE_T got = 0;
    return ReadProcessMemory(GetCurrentProcess(), (LPCVOID)addr, buf, len, &got) && got == len;
}

/* ── Our hook function ── */
__attribute__((ms_abi))
static void hook_pkt_send(void *pkt_obj) {
    DWORD64 rcx = (DWORD64)pkt_obj;
    WORD internal_op = 0;
    DWORD pkt_len = 0;
    unsigned char head[16];

    /* Always log entry for diagnostics (first 5 calls) */
    LONG count = InterlockedCompareExchange(&g_out_count, 0, 0);
    if (count < 5 && g_log) {
        fprintf(g_log, "{\"t\":%llu,\"d\":\"hook_entry\",\"rcx\":\"0x%llx\",\"n\":%ld}\n",
                (unsigned long long)GetTickCount64(),
                (unsigned long long)rcx,
                (long)count);
        fflush(g_log);
    }

    if (safe_read(rcx + PKT_PAYLOAD_OFF, &internal_op, 2) &&
        safe_read(rcx + PKT_LENGTH_OFF, &pkt_len, 4) &&
        pkt_len >= 2 && pkt_len < 0x10000) {

        SIZE_T head_len = (pkt_len < 16) ? pkt_len : 16;
        if (safe_read(rcx + PKT_PAYLOAD_OFF, head, head_len) && g_log) {
            DWORD64 tick = GetTickCount64();
            int i;

            fprintf(g_log, "{\"t\":%llu,\"d\":\"out\",\"op\":\"0x%04x\",\"len\":%lu,\"head\":\"",
                    (unsigned long long)tick, internal_op, (unsigned long)pkt_len);
            for (i = 0; i < (int)head_len; i++)
                fprintf(g_log, "%02x", head[i]);
            fprintf(g_log, "\"}\n");
            fflush(g_log);
            InterlockedIncrement(&g_out_count);
        }
    }

    /* Call original function via trampoline */
    if (g_trampoline_fn)
        g_trampoline_fn(pkt_obj);
}

/* ── Build 14-byte absolute JMP ── */
static void build_abs_jmp(BYTE *buf, DWORD64 target) {
    buf[0] = 0xFF;
    buf[1] = 0x25;
    buf[2] = 0x00;
    buf[3] = 0x00;
    buf[4] = 0x00;
    buf[5] = 0x00;
    *(DWORD64 *)(buf + 6) = target;
}

/* ── Allocate a page within ±2GB of target ── */
static LPVOID alloc_near(DWORD64 target) {
    DWORD64 lo = (target > 0x70000000ULL) ? (target - 0x70000000ULL) : 0x10000ULL;
    DWORD64 hi = target + 0x70000000ULL;
    DWORD64 addr;

    /* Try addresses in 64KB increments (VirtualAlloc granularity) */
    for (addr = lo; addr < hi; addr += 0x10000) {
        LPVOID p = VirtualAlloc((LPVOID)addr, 0x1000,
                                MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE);
        if (p)
            return p;
    }
    return NULL;
}

/* ── Install the inline hook ── */
static BOOL install_hook(void) {
    DWORD old_prot;
    BYTE func_bytes[32];

    /* Strip any stale PAGE_GUARD from earlier probes */
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery((LPCVOID)PKT_SEND_FUNC, &mbi, sizeof(mbi))) {
        probe_log("# target page: base=0x%llx, protect=0x%lx",
                  (unsigned long long)(DWORD64)mbi.BaseAddress,
                  (unsigned long)mbi.Protect);
        if (mbi.Protect & PAGE_GUARD) {
            DWORD clean = mbi.Protect & ~PAGE_GUARD;
            DWORD tmp;
            VirtualProtect(mbi.BaseAddress, mbi.RegionSize, clean, &tmp);
            probe_log("# stripped PAGE_GUARD");
        }
    }

    /* Restore any stale INT3 from v3.0 */
    BYTE test_byte = 0;
    if (safe_read(PKT_SEND_FUNC, &test_byte, 1) && test_byte == 0xCC) {
        probe_log("# restoring stale INT3 → 0x48");
        VirtualProtect((LPVOID)PKT_SEND_FUNC, 1, PAGE_EXECUTE_READWRITE, &old_prot);
        *(volatile BYTE *)PKT_SEND_FUNC = 0x48;
        FlushInstructionCache(GetCurrentProcess(), (LPCVOID)PKT_SEND_FUNC, 1);
        VirtualProtect((LPVOID)PKT_SEND_FUNC, 1, old_prot, &old_prot);
    }

    /* Undo v4.0's 14-byte hook if present (check for FF 25) */
    BYTE jmp_check[2] = {0};
    safe_read(PKT_SEND_FUNC, jmp_check, 2);
    if (jmp_check[0] == 0xFF && jmp_check[1] == 0x25) {
        probe_log("# undoing v4.0 abs JMP, restoring prologue");
        /* Read saved prologue from v4.0's trampoline at 0x7387040 */
        BYTE old_saved[16];
        if (safe_read(0x7387040ULL, old_saved, 16)) {
            VirtualProtect((LPVOID)PKT_SEND_FUNC, 16, PAGE_EXECUTE_READWRITE, &old_prot);
            memcpy((void *)PKT_SEND_FUNC, old_saved, 16);
            FlushInstructionCache(GetCurrentProcess(), (LPCVOID)PKT_SEND_FUNC, 16);
            VirtualProtect((LPVOID)PKT_SEND_FUNC, 16, old_prot, &old_prot);
        }
    }

    /* Read the first 32 bytes of the function */
    if (!safe_read(PKT_SEND_FUNC, func_bytes, 32)) {
        probe_log("# ERROR: cannot read function at 0x%llx", (unsigned long long)PKT_SEND_FUNC);
        return FALSE;
    }

    /* Log for diagnostics */
    {
        char hex[97];
        int i;
        for (i = 0; i < 32; i++)
            snprintf(hex + i * 3, 4, "%02x ", func_bytes[i]);
        hex[96] = '\0';
        probe_log("# func bytes: %s", hex);
    }

    /* Verify expected prologue: 48 89 4c 24 08 (mov [rsp+8], rcx) */
    if (func_bytes[0] != 0x48 || func_bytes[1] != 0x89 ||
        func_bytes[2] != 0x4C || func_bytes[3] != 0x24 || func_bytes[4] != 0x08) {
        probe_log("# WARNING: unexpected prologue (expected 48 89 4c 24 08), got %02x %02x %02x %02x %02x",
                  func_bytes[0], func_bytes[1], func_bytes[2], func_bytes[3], func_bytes[4]);
        /* Continue anyway — might be a different but still 5+ byte instruction */
    }

    /* Save the prologue */
    memcpy(g_saved_prologue, func_bytes, STOLEN_BYTES);

    /* Allocate nearby page for springboard + trampoline */
    g_nearby_page = alloc_near(PKT_SEND_FUNC);
    if (!g_nearby_page) {
        probe_log("# ERROR: cannot allocate page near target");
        return FALSE;
    }

    DWORD64 page_addr = (DWORD64)g_nearby_page;
    probe_log("# nearby page at 0x%llx (delta from target: %lld)",
              (unsigned long long)page_addr,
              (long long)(page_addr - PKT_SEND_FUNC));

    /*
     * Layout on nearby page:
     *   +0:  springboard — 14-byte abs JMP to hook_pkt_send
     *   +16: trampoline  — 5 saved bytes + 5-byte rel JMP to PKT_SEND_FUNC+5
     */
    BYTE *springboard = (BYTE *)g_nearby_page;
    BYTE *trampoline  = springboard + 16;

    /* Build springboard: abs JMP to hook_pkt_send */
    build_abs_jmp(springboard, (DWORD64)hook_pkt_send);

    /* Build trampoline: saved 5 bytes + rel32 JMP back */
    memcpy(trampoline, g_saved_prologue, STOLEN_BYTES);

    /* Compute rel32 for JMP back to PKT_SEND_FUNC + STOLEN_BYTES */
    DWORD64 jmp_from = (DWORD64)(trampoline + STOLEN_BYTES + 5); /* next instruction after JMP */
    DWORD64 jmp_to   = PKT_SEND_FUNC + STOLEN_BYTES;
    LONG64 delta = (LONG64)(jmp_to - jmp_from);

    if (delta > 0x7FFFFFFFLL || delta < -0x80000000LL) {
        probe_log("# ERROR: trampoline too far for rel32 JMP (delta=%lld)", (long long)delta);
        VirtualFree(g_nearby_page, 0, MEM_RELEASE);
        g_nearby_page = NULL;
        return FALSE;
    }

    trampoline[STOLEN_BYTES] = 0xE9; /* JMP rel32 */
    *(LONG *)(trampoline + STOLEN_BYTES + 1) = (LONG)delta;

    FlushInstructionCache(GetCurrentProcess(), g_nearby_page, 32);

    g_trampoline_fn = (pkt_send_fn)trampoline;
    probe_log("# springboard at 0x%llx, trampoline at 0x%llx",
              (unsigned long long)(DWORD64)springboard,
              (unsigned long long)(DWORD64)trampoline);

    /* Now patch PKT_SEND_FUNC: 5-byte rel32 JMP to springboard */
    DWORD64 patch_from = PKT_SEND_FUNC + 5; /* next instruction after our JMP */
    DWORD64 patch_to   = (DWORD64)springboard;
    LONG64 patch_delta = (LONG64)(patch_to - patch_from);

    if (patch_delta > 0x7FFFFFFFLL || patch_delta < -0x80000000LL) {
        probe_log("# ERROR: springboard too far for rel32 JMP (delta=%lld)", (long long)patch_delta);
        VirtualFree(g_nearby_page, 0, MEM_RELEASE);
        g_nearby_page = NULL;
        return FALSE;
    }

    if (!VirtualProtect((LPVOID)PKT_SEND_FUNC, STOLEN_BYTES, PAGE_EXECUTE_READWRITE, &old_prot)) {
        probe_log("# ERROR: VirtualProtect on target failed: %lu", GetLastError());
        VirtualFree(g_nearby_page, 0, MEM_RELEASE);
        g_nearby_page = NULL;
        return FALSE;
    }

    *(volatile BYTE *)PKT_SEND_FUNC = 0xE9; /* JMP rel32 */
    *(volatile LONG *)(PKT_SEND_FUNC + 1) = (LONG)patch_delta;

    FlushInstructionCache(GetCurrentProcess(), (LPCVOID)PKT_SEND_FUNC, STOLEN_BYTES);
    VirtualProtect((LPVOID)PKT_SEND_FUNC, STOLEN_BYTES, old_prot, &old_prot);

    /* Verify the patch */
    BYTE verify[5];
    safe_read(PKT_SEND_FUNC, verify, 5);
    probe_log("# patched bytes: %02x %02x %02x %02x %02x",
              verify[0], verify[1], verify[2], verify[3], verify[4]);

    g_hooked = TRUE;
    probe_log("# hook flow: 0x%llx -E9-> 0x%llx -FF25-> %p -> trampoline -> 0x%llx",
              (unsigned long long)PKT_SEND_FUNC,
              (unsigned long long)(DWORD64)springboard,
              hook_pkt_send,
              (unsigned long long)(PKT_SEND_FUNC + STOLEN_BYTES));

    return TRUE;
}

/* ── Remove the inline hook ── */
static void remove_hook(void) {
    if (!g_hooked) return;
    DWORD old_prot;
    if (VirtualProtect((LPVOID)PKT_SEND_FUNC, STOLEN_BYTES, PAGE_EXECUTE_READWRITE, &old_prot)) {
        memcpy((void *)PKT_SEND_FUNC, g_saved_prologue, STOLEN_BYTES);
        FlushInstructionCache(GetCurrentProcess(), (LPCVOID)PKT_SEND_FUNC, STOLEN_BYTES);
        VirtualProtect((LPVOID)PKT_SEND_FUNC, STOLEN_BYTES, old_prot, &old_prot);
        probe_log("# hook removed");
    }
    if (g_nearby_page) {
        VirtualFree(g_nearby_page, 0, MEM_RELEASE);
        g_nearby_page = NULL;
    }
    g_hooked = FALSE;
}

/* ── DllMain ── */
BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);

        char path[MAX_PATH];
        snprintf(path, sizeof(path), LOG_PATH, GetCurrentProcessId());
        g_log = fopen(path, "a");
        if (!g_log) return TRUE;

        HMODULE exe = GetModuleHandleW(NULL);
        DWORD64 base = (DWORD64)exe;
        probe_log("# neon opcode probe v4.1 (inline hook, nearby springboard), pid=%lu, base=0x%llx",
                  GetCurrentProcessId(), (unsigned long long)base);

        if (!install_hook()) {
            probe_log("# FATAL: hook installation failed");
            return TRUE;
        }

        probe_log("# probe active — capturing outgoing opcodes");

    } else if (reason == DLL_PROCESS_DETACH) {
        remove_hook();
        if (g_log) {
            probe_log("# stopped, total outgoing: %ld",
                      InterlockedCompareExchange(&g_out_count, 0, 0));
            fclose(g_log);
            g_log = NULL;
        }
    }
    return TRUE;
}
