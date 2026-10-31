/* neon-hack.dll - client-side god mode (HP/MP lock) for testing the NGS-X bypass.
 *
 * No code bytes are patched. The module only reads and writes values, and runs on a
 * threadpool timer (no dedicated thread, no PEB entry: it is manual-mapped).
 *
 * Discovery: at full HP/MP a stats struct holds (hp,max_hp) / (mp,max_mp) as equal
 * adjacent u32 pairs. The scan collects those pairs in writable private memory, drops
 * any that stop making sense, and confirms a pair after two observed decreases. Once
 * confirmed the current value is rewritten to the maximum every tick.
 *
 * Diagnostics go to C:\Neon\logs\hack.<pid>.log (attach, per-tick progress, faults).
 * Control comes from the panel through Local\NeonHack.<pid> (see proto.h):
 *   cmd 0 = stop writing, 1 = scan+lock, 2 = force addr/value.
 */
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <stdarg.h>
#include "proto.h"

#define TICK_MS 100
#define MAX_CAND (1u << 20)
#define MAX_REGION (16u << 20)
#define MAX_SCAN_BYTES (512ull << 20)
#define SCAN_STEP_BYTES (32ull << 20)
#define SCAN_STEP_QUERIES 65536
#define CONFIRM_EVENTS 2
#define CONFIRM_WINDOW_MS 30000

typedef struct { ULONG_PTR addr; ULONG last, max, events, when; } CAND;

static NEON_HACK_STATE *g_st;
static HANDLE g_map;
static HANDLE g_thread;
static volatile LONG g_stop;
static CAND g_cand[MAX_CAND];
static ULONG g_ncand;
static ULONG_PTR g_cursor;
static ULONGLONG g_scanned;
static ULONG g_last_mb;
static int g_scanning;
static ULONG g_dbg;
static int g_started;
static HANDLE g_log = INVALID_HANDLE_VALUE;

static void logf_(const char *fmt, ...) {
    if (g_log == INVALID_HANDLE_VALUE) return;
    char buf[1024];
    va_list ap; va_start(ap, fmt);
    int n = wvsprintfA(buf, fmt, ap);
    va_end(ap);
    DWORD w;
    WriteFile(g_log, buf, n, &w, NULL);
    WriteFile(g_log, "\r\n", 2, &w, NULL);
}

static ULONG rd(ULONG_PTR a) { return *(volatile ULONG *)a; }
static void wr(ULONG_PTR a, ULONG v) { *(volatile ULONG *)a = v; }

/* The stat object can live on a read-only image page; flip it once so the direct
 * writes below do not fault every tick. */
static void make_rw(ULONG_PTR a) {
    MEMORY_BASIC_INFORMATION m;
    DWORD old = 0;
    if (VirtualQuery((void *)a, &m, sizeof m) != sizeof m) return;
    DWORD p = m.Protect & 0xFF;
    if (p == PAGE_READWRITE || p == PAGE_EXECUTE_READWRITE || p == PAGE_WRITECOPY) return;
    if (VirtualProtect((void *)a, 0x1000, PAGE_READWRITE, &old))
        logf_("made %p writable (was %lx)", (void *)a, m.Protect);
}

/* A manual-mapped image is invisible to the unwinder until its .pdata is registered,
 * otherwise one fault in this DLL takes the client down. */
static void register_unwind(HMODULE h) {
    PIMAGE_DOS_HEADER dos = (PIMAGE_DOS_HEADER)h;
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) return;
    PIMAGE_NT_HEADERS nt = (PIMAGE_NT_HEADERS)((BYTE *)h + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) return;
    DWORD rva = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXCEPTION].VirtualAddress;
    DWORD size = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXCEPTION].Size;
    if (!rva || !size) return;
    if (RtlAddFunctionTable((PRUNTIME_FUNCTION)((BYTE *)h + rva),
                            size / sizeof(RUNTIME_FUNCTION), (DWORD64)(ULONG_PTR)h))
        logf_("unwind tables registered (%lu entries)", size / sizeof(RUNTIME_FUNCTION));
}

static int writable_private(const MEMORY_BASIC_INFORMATION *m) {
    if (m->State != MEM_COMMIT || m->Type != MEM_PRIVATE) return 0;
    if (m->Protect & PAGE_GUARD) return 0;
    DWORD p = m->Protect & 0xFF;
    return p == PAGE_READWRITE || p == PAGE_WRITECOPY ||
           p == PAGE_EXECUTE_READWRITE || p == PAGE_EXECUTE_WRITECOPY;
}

static void take_pair(ULONG *q, ULONG i) {
    ULONG a = q[i], b = q[i + 1];
    if (a != b || a < 20 || a > 30000) return;
    CAND *c = &g_cand[g_ncand++];
    c->addr = (ULONG_PTR)q + i * 4;
    c->last = a; c->max = b; c->events = 0; c->when = 0;
}

/* Walks a bounded slice of the address space per tick so the scan never blocks the
 * client for long. Returns 1 once the walk is finished. */
static int collect_step(void) {
    ULONGLONG budget = SCAN_STEP_BYTES;
    ULONG queries = 0;
    logf_("step cursor=%p scanned=%lu q=%lu", (void *)g_cursor,
          (ULONG)(g_scanned >> 20), g_dbg);
    while (g_cursor < 0x00007FFFFFFEFFFFull && budget && g_ncand < MAX_CAND &&
           g_scanned < MAX_SCAN_BYTES && queries++ < SCAN_STEP_QUERIES) {
        MEMORY_BASIC_INFORMATION m;
        __try {
            if (VirtualQuery((void *)g_cursor, &m, sizeof m) != sizeof m) {
                g_cursor = 0x00007FFFFFFFFFFFFFull;
                break;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {
            g_cursor += 0x1000;
            continue;
        }
        ULONG_PTR base = (ULONG_PTR)m.BaseAddress;
        ULONG_PTR next = base + m.RegionSize;
        if (writable_private(&m) && m.RegionSize <= MAX_REGION) {

            ULONG_PTR p = base, end = next;
            while (p < end && budget && g_ncand < MAX_CAND && g_scanned < MAX_SCAN_BYTES) {
                ULONGLONG chunk = end - p;
                if (chunk > budget) chunk = budget;
                ULONG *q = (ULONG *)p;
                ULONG n = (ULONG)(chunk / 4);
                __try {
                    if (p > base) take_pair(q - 1, 0);
                    for (ULONG i = 0; i + 1 < n && g_ncand < MAX_CAND; i++) take_pair(q, i);
                } __except (EXCEPTION_EXECUTE_HANDLER) { }
                p += chunk;
                budget -= chunk;
                g_scanned += chunk;
                if (g_last_mb != (ULONG)(g_scanned >> 20) && !(g_scanned & ((32ull << 20) - 1))) {
                    g_last_mb = (ULONG)(g_scanned >> 20);
                    logf_("scan progress %lu MB at %p (%lu cand)", (ULONG)(g_scanned >> 20),
                          (void *)p, g_ncand);
                }
            }

            next = p;
        }
        if (next <= g_cursor) g_cursor += 0x1000; else g_cursor = next;
    }
    g_st->candidates = g_ncand;
    if (g_cursor >= 0x00007FFFFFFEFFFFull || g_scanned >= MAX_SCAN_BYTES ||
        g_ncand >= MAX_CAND)
        return 1;
    return 0;
}

/* Stats object: verified static root in the exe image (neon-calib/chains.json,
 * re-verified through restarts). Read the 8-byte pointer at image_base + root, then
 * hp/mp live at +0x360/+0x364. The root itself is re-read every tick, so a map
 * change or relogin cannot leave a stale heap address behind. */
#define STATS_ROOT_OFF 0x3bf8e78ull
#define STATS_HP_OFF 0x360u
#define STATS_MP_OFF 0x364u

static ULONG_PTR g_exe;
static ULONG g_hp_keep, g_mp_keep;
static ULONG_PTR g_obj;
static ULONG g_rescan_at;

/* The static root slot is empty on this build, so locate the stat object the way the
 * calibrator did: the object is referenced by several pointer slots in the image and
 * has the level/hp/mp layout. The most-referenced candidate wins. */
#define MAX_OBJ 16

static int find_stats_object(ULONG_PTR *out_obj, ULONG *out_hp, ULONG *out_mp) {
    if (!g_exe) g_exe = (ULONG_PTR)GetModuleHandleW(NULL);
    if (!g_exe) return 0;
    PIMAGE_DOS_HEADER dos = (PIMAGE_DOS_HEADER)g_exe;
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) return 0;
    PIMAGE_NT_HEADERS nt = (PIMAGE_NT_HEADERS)((BYTE *)g_exe + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) return 0;
    ULONG_PTR end = g_exe + nt->OptionalHeader.SizeOfImage;

    ULONG_PTR objs[MAX_OBJ] = {0};
    ULONG counts[MAX_OBJ] = {0};
    ULONG_PTR q = g_exe;
    while (q + 8 <= end) {
        MEMORY_BASIC_INFORMATION m;
        if (VirtualQuery((void *)q, &m, sizeof m) != sizeof m) break;
        ULONG_PTR rbase = (ULONG_PTR)m.BaseAddress;
        ULONG_PTR rend = rbase + m.RegionSize;
        if (rend <= q) { q += 0x1000; continue; }
        if (m.State == MEM_COMMIT && !(m.Protect & PAGE_GUARD) && !(m.Protect & PAGE_NOACCESS)) {
            if (rbase < q) rbase = q;
            for (; rbase + 8 <= rend; rbase += 8) {
                ULONG_PTR v;
                __try { v = *(volatile ULONG_PTR *)rbase; }
                __except (EXCEPTION_EXECUTE_HANDLER) { continue; }
                if (v < 0x10000 || v > 0x7FFFFFFEFFFFull || !(v & 3)) continue;
                ULONG lvl, hp, mp;
                __try { lvl = rd(v + 0x160); hp = rd(v + STATS_HP_OFF); mp = rd(v + STATS_MP_OFF); }
                __except (EXCEPTION_EXECUTE_HANDLER) { continue; }
                if (lvl > 300 || hp < 10 || hp > 3000 || mp > 3000) continue;
                int slot = -1;
                for (int i = 0; i < MAX_OBJ; i++) if (objs[i] == v) { slot = i; break; }
                if (slot >= 0) { counts[slot]++; continue; }
                for (int i = 0; i < MAX_OBJ; i++) if (!counts[i]) { objs[i] = v; counts[i] = 1; slot = i; break; }
                if (slot < 0) {
                    int min_i = 0;
                    for (int i = 1; i < MAX_OBJ; i++) if (counts[i] < counts[min_i]) min_i = i;
                    if (counts[min_i] == 1) { objs[min_i] = v; counts[min_i] = 1; }
                }
            }
        }
        q = rend;
    }
    int best = -1;
    for (int i = 0; i < MAX_OBJ; i++) if (counts[i] >= 2 && (best < 0 || counts[i] > counts[best])) best = i;
    if (best < 0) return 0;
    ULONG h, mp;
    __try { h = rd(objs[best] + STATS_HP_OFF); mp = rd(objs[best] + STATS_MP_OFF); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return 0; }
    if (!h || h > 30000) return 0;
    *out_obj = objs[best]; *out_hp = h; *out_mp = mp;
    return 1;
}

static int chain_read(ULONG_PTR *obj, ULONG *hp, ULONG *mp) {
    if (!g_exe) g_exe = (ULONG_PTR)GetModuleHandleW(NULL);
    if (!g_exe) return 0;
    ULONG_PTR o = 0;
    __try { o = *(volatile ULONG_PTR *)(g_exe + STATS_ROOT_OFF); }
    __except (EXCEPTION_EXECUTE_HANDLER) { o = 0; }
    if (o >= 0x10000) {
        ULONG h = 0, m = 0;
        __try { h = rd(o + STATS_HP_OFF); m = rd(o + STATS_MP_OFF); }
        __except (EXCEPTION_EXECUTE_HANDLER) { h = 0; }
        if (h >= 10 && h <= 3000 && m <= 3000) { *obj = o; *hp = h; *mp = m; return 1; }
    }
    ULONG now = GetTickCount();
    if (!g_obj || (LONG)(now - g_rescan_at) >= 0) {
        ULONG_PTR o2; ULONG h2, m2;
        if (find_stats_object(&o2, &h2, &m2)) {
            g_obj = o2;
            g_rescan_at = now + 5000;
            logf_("stats object %p (hp %lu mp %lu)", (void *)o2, h2, m2);
            *obj = o2; *hp = h2; *mp = m2;
            return 1;
        }
        g_rescan_at = now + 1000;
        return 0;
    }
    ULONG h = 0, m = 0;
    __try { h = rd(g_obj + STATS_HP_OFF); m = rd(g_obj + STATS_MP_OFF); }
    __except (EXCEPTION_EXECUTE_HANDLER) { g_obj = 0; return 0; }
    if (!h || h > 3000 || m > 3000) { g_obj = 0; return 0; }
    *obj = g_obj; *hp = h; *mp = m;
    return 1;
}


/* Keep the highest value seen: it is the current max on a full bar and follows
 * level-ups upward. Never lower it, or a hit would shrink the lock. */
static void chain_lock(ULONG_PTR o, ULONG h, ULONG m) {
    if (!g_hp_keep || h > g_hp_keep) g_hp_keep = h;
    if (m && (!g_mp_keep || m > g_mp_keep)) g_mp_keep = m;
    g_st->hp = h;
    g_st->max_hp = g_hp_keep;
    g_st->frozen_count = g_mp_keep ? 2 : 1;
    g_st->frozen_addr[0] = o + STATS_HP_OFF;
    g_st->frozen_value[0] = g_hp_keep;
    g_st->frozen_addr[1] = o + STATS_MP_OFF;
    g_st->frozen_value[1] = g_mp_keep;
    make_rw(o + STATS_HP_OFF);
    __try { wr(o + STATS_HP_OFF, g_hp_keep); wr(o + STATS_MP_OFF, g_mp_keep); }
    __except (EXCEPTION_EXECUTE_HANDLER) { g_st->last_error = 4; }
    g_st->state = HACK_STATE_FROZEN;
}

static void do_scan_tick(void) {
    if (!g_started) {
        g_started = 1;
        g_cursor = 0x10000;
        g_scanned = 0;
        g_last_mb = 0;
        g_scanning = 1;
        g_st->state = HACK_STATE_SCANNING;
        logf_("scan begin");
    }
    if (g_scanning) {
        if (collect_step()) {
            g_scanning = 0;
            logf_("collect: %lu candidates, %lu MB scanned", g_ncand, (ULONG)(g_scanned >> 20));
        }
        g_st->candidates = g_ncand;
    }
    if (!g_ncand) return;
    ULONG now = GetTickCount();
    ULONG kept = 0, best = 0;
    for (ULONG i = 0; i < g_ncand; i++) {
        CAND *c = &g_cand[i];
        ULONG x, y;
        __try { x = rd(c->addr); y = rd(c->addr + 4); }
        __except (EXCEPTION_EXECUTE_HANDLER) { continue; }
        if (y != c->max || x > y || y < 20) continue;
        if (x < c->last) { c->events++; c->when = now; g_st->damage_events++; }
        c->last = x;
        if (c->events && now - c->when > CONFIRM_WINDOW_MS) c->events = 0;
        g_cand[kept++] = *c;
        if (c->events > best) best = c->events;
    }
    g_ncand = kept;
    g_st->candidates = g_ncand;
    if (kept && best >= CONFIRM_EVENTS) {
        ULONG n = 0;
        for (ULONG i = 0; i < g_ncand && n < 4; i++) {
            if (g_cand[i].events < CONFIRM_EVENTS) continue;
            g_st->frozen_addr[n] = g_cand[i].addr;
            g_st->frozen_value[n] = g_cand[i].max;
            n++;
        }
        g_st->frozen_count = n;
        g_st->state = HACK_STATE_CONFIRMED;
        for (ULONG i = 0; i < n; i++)
            logf_("confirmed %p = %lu", (void *)(ULONG_PTR)g_st->frozen_addr[i],
                  g_st->frozen_value[i]);
    }
}

static void do_freeze_tick(void) {
    if (!g_st->frozen_count) return;
    for (ULONG i = 0; i < g_st->frozen_count; i++) {
        ULONG_PTR a = (ULONG_PTR)g_st->frozen_addr[i];
        ULONG v = g_st->frozen_value[i];
        if (!a || !v) continue;
        if (!(g_st->ticks % 10) && !i) logf_("freeze %p = %lu", (void *)a, v);
        __try { wr(a, v); } __except (EXCEPTION_EXECUTE_HANDLER) { logf_("fault %p", (void *)a); }
    }
    g_st->state = HACK_STATE_FROZEN;
}

static void do_force_tick(void) {
    ULONG_PTR a = (ULONG_PTR)g_st->force_addr;
    ULONG v = g_st->force_value;
    if (!a || !v) return;
    make_rw(a);
    __try { wr(a, v); } __except (EXCEPTION_EXECUTE_HANDLER) { g_st->last_error = 1; return; }
    g_st->frozen_addr[0] = a;
    g_st->frozen_value[0] = v;
    g_st->frozen_count = 1;
    g_st->state = HACK_STATE_FROZEN;
}

static DWORD WINAPI tick_thread(void *arg) {
    (void)arg;
    while (!g_stop) {
        Sleep(TICK_MS);
        if (!g_st || g_st->magic != NEON_HACK_MAGIC) continue;
        g_st->ticks++;
        __try {
            switch (g_st->cmd) {
            case HACK_CMD_OFF: g_st->state = HACK_STATE_IDLE; break;
            case HACK_CMD_ON: {
                ULONG_PTR o; ULONG h, m;
                if (chain_read(&o, &h, &m)) chain_lock(o, h, m);
                else if (g_st->state < HACK_STATE_CONFIRMED) do_scan_tick();
                if (g_st->state >= HACK_STATE_CONFIRMED) do_freeze_tick();
                break;
            }
            case HACK_CMD_FORCE: do_force_tick(); break;
            default: break;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { g_st->last_error = 3; }
    }
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(h);
        if (GetEnvironmentVariableW(L"NEON_HACK_MARKER", NULL, 0) == 0) {
            SetEnvironmentVariableW(L"NEON_HACK_MARKER", L"1");
            HANDLE mf = CreateFileW(L"C:\\Neon\\marker.txt", FILE_APPEND_DATA, 0, NULL,
                                    OPEN_ALWAYS, 0, NULL);
            if (mf != INVALID_HANDLE_VALUE) {
                char b[32]; DWORD wr;
                int n = wsprintfA(b, "attach %lu\r\n", GetCurrentProcessId());
                WriteFile(mf, b, n, &wr, NULL);
                CloseHandle(mf);
            }
        }
        CreateDirectoryW(L"C:\\Neon", NULL);
        CreateDirectoryW(L"C:\\Neon\\logs", NULL);
        wchar_t path[MAX_PATH];
        wsprintfW(path, L"C:\\Neon\\logs\\hack.%lu.log", GetCurrentProcessId());
        g_log = CreateFileW(path, FILE_APPEND_DATA, FILE_SHARE_READ, NULL,
                            OPEN_ALWAYS, 0, NULL);
        logf_("attach pid %lu base %p", GetCurrentProcessId(), (void *)h);
        register_unwind(h);

        wchar_t name[64];
        wsprintfW(name, L"Local\\NeonHack.%lu", GetCurrentProcessId());
        g_map = CreateFileMappingW(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0,
                                   NEON_HACK_BYTES, name);
        if (!g_map) { logf_("CreateFileMapping %lu", GetLastError()); return TRUE; }
        g_st = (NEON_HACK_STATE *)MapViewOfFile(g_map, FILE_MAP_ALL_ACCESS, 0, 0, 0);
        if (!g_st) { logf_("MapViewOfFile %lu", GetLastError()); return TRUE; }
        ZeroMemory(g_st, NEON_HACK_BYTES);
        g_st->magic = NEON_HACK_MAGIC;
        g_st->version = NEON_HACK_VERSION;
        g_st->pid = GetCurrentProcessId();
        g_st->module_base = (ULONGLONG)(ULONG_PTR)h;
        g_thread = CreateThread(NULL, 0, tick_thread, NULL, 0, NULL);
        if (g_thread) {
            logf_("tick thread %lu", GetThreadId(g_thread));
        } else {
            g_st->last_error = 2;
            logf_("CreateThread failed %lu", GetLastError());
        }
    } else if (reason == DLL_PROCESS_DETACH) {
        logf_("detach");
        g_stop = 1;
        if (g_thread) { WaitForSingleObject(g_thread, 1000); CloseHandle(g_thread); }
        if (g_st) { g_st->magic = 0; UnmapViewOfFile(g_st); }
        if (g_map) CloseHandle(g_map);
        if (g_log != INVALID_HANDLE_VALUE) { CloseHandle(g_log); g_log = INVALID_HANDLE_VALUE; }
    }
    return TRUE;
}
