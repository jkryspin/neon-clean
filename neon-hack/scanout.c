/* scanout.exe - external memory harness for the neon-hack scanner.
 *   scanout <pid> read            walk + read everything (sanity check)
 *   scanout <pid> find <a> <b>    list addresses holding the u32 pair (a,b)
 *   scanout <pid> write <addr> <v> write one u32
 */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAXHITS 400000

/* ReadProcessMemory fails across a PAGE_GUARD, so halve the span until it works. */
static void read_into(HANDLE p, char *addr, unsigned char *buf, SIZE_T n) {
    SIZE_T got = 0;
    if (ReadProcessMemory(p, addr, buf, n, &got) && got == n) return;
    if (n > 0x1000) {
        SIZE_T h = n / 2 & ~(SIZE_T)3;
        read_into(p, addr, buf, h);
        read_into(p, addr + h, buf + h, n - h);
        return;
    }
    memset(buf, 0, n);
}
static int readable(const MEMORY_BASIC_INFORMATION *m) {
    if (m->State != MEM_COMMIT) return 0;
    if (m->Protect & PAGE_GUARD) return 0;
    return (m->Protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY |
                          PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE |
                          PAGE_EXECUTE_WRITECOPY)) != 0;
}

static HANDLE open_target(DWORD pid) {
    HANDLE p = OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION | PROCESS_VM_WRITE |
                           PROCESS_VM_OPERATION, FALSE, pid);
    if (!p) { fprintf(stderr, "OpenProcess %lu\n", GetLastError()); exit(1); }
    return p;
}

static void do_read(HANDLE p) {
    unsigned char *buf = malloc(1 << 20);
    ULONGLONG cur = 0x10000;
    unsigned long regions = 0, bytes = 0;
    DWORD t0 = GetTickCount();
    while (cur < 0x00007FFFFFFEFFFFull) {
        MEMORY_BASIC_INFORMATION m;
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (readable(&m)) {
            regions++;
            for (ULONGLONG off = 0; off < m.RegionSize; off += (1 << 20)) {
                SIZE_T n = (SIZE_T)(m.RegionSize - off > (1 << 20) ? (1 << 20) : m.RegionSize - off);
                SIZE_T got = 0;
                if (ReadProcessMemory(p, (char *)m.BaseAddress + off, buf, n, &got)) bytes += (unsigned long)got;
            }
        }
        if (next <= cur) cur += 0x1000; else cur = next;
    }
    printf("done: %lu regions, %lu MB, %lums\n", regions, bytes >> 20, GetTickCount() - t0);
}

static int g_privonly = 0;

static void do_findmax(HANDLE p, unsigned long b) {
    unsigned char *buf = malloc((1 << 20) + 8);
    ULONGLONG cur = 0x10000;
    int hits = 0;
    while (cur < 0x00007FFFFFFEFFFFull && hits < MAXHITS) {
        MEMORY_BASIC_INFORMATION m;
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (readable(&m) && (!g_privonly || m.Type == MEM_PRIVATE)) {
            for (ULONGLONG off = 0; off < m.RegionSize && hits < MAXHITS; off += (1 << 20)) {
                SIZE_T n = (SIZE_T)(m.RegionSize - off > (1 << 20) ? (1 << 20) : m.RegionSize - off);
                SIZE_T got = 0;
                read_into(p, (char *)m.BaseAddress + off, buf, n); got = n;
                if (got < 8) continue;
                for (SIZE_T i = 0; i + 8 <= got; i += 4) {
                    unsigned long x, y;
                    memcpy(&x, buf + i, 4);
                    memcpy(&y, buf + i + 4, 4);
                    if (y == b && x <= y && x >= 1) {
                        printf("%llX %lu\n", (unsigned long long)((char *)m.BaseAddress + off + i), x);
                        if (++hits >= MAXHITS) break;
                    }
                }
            }
        }
        if (next <= cur) cur += 0x1000; else cur = next;
    }
    printf("hits=%d\n", hits);
}

/* Find a static pointer P such that *(P+o1)==v1 && *(P+o2)==v2 (heap-object chains). */
static void do_findchain(HANDLE p, unsigned long long o1, unsigned long long o2,
                         unsigned v1lo, unsigned v1hi, unsigned v2lo, unsigned v2hi) {
    MEMORY_BASIC_INFORMATION m;
    unsigned char *buf = malloc((1 << 20) + 8);
    ULONGLONG cur = 0x140000000ull;
    int hits = 0;
    while (cur < 0x00007FFFFFFEFFFFull && hits < 50) {
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (m.Type == MEM_IMAGE && readable(&m)) {
            for (ULONGLONG off = 0; off < m.RegionSize && hits < 50; off += (1 << 20)) {
                SIZE_T n = (SIZE_T)(m.RegionSize - off > (1 << 20) ? (1 << 20) : m.RegionSize - off);
                read_into(p, (char *)m.BaseAddress + off, buf, n);
                for (SIZE_T i = 0; i + 8 <= n; i += 8) {
                    unsigned long long v;
                    memcpy(&v, buf + i, 8);
                    if (v < 0x100000000ull || v > 0x7FFFFFFEFFFFull) continue;
                    unsigned x = 0, y = 0; SIZE_T got = 0;
                    unsigned lvl = 1;
                    if (ReadProcessMemory(p, (void *)(ULONG_PTR)(v + 0x160), &lvl, 4, &got) && got == 4 &&
                        lvl == 0 &&
                        ReadProcessMemory(p, (void *)(ULONG_PTR)(v + o1), &x, 4, &got) && got == 4 &&
                        ReadProcessMemory(p, (void *)(ULONG_PTR)(v + o2), &y, 4, &got) && got == 4 &&
                        x >= v1lo && x <= v1hi && y >= v2lo && y <= v2hi)
                        printf("root=%llX obj=%llX\n",
                               (unsigned long long)((char *)m.BaseAddress + off + i), v);
                    hits += (x >= v1lo && x <= v1hi && y >= v2lo && y <= v2hi);
                }
            }
        }
        if (next <= cur) cur += 0x1000; else cur = next;
    }
    printf("hits=%d\n", hits);
}

static void do_findblob(HANDLE p, unsigned char *pat, int plen) {
    MEMORY_BASIC_INFORMATION m;
    unsigned char *buf = malloc((1 << 20) + 64);
    ULONGLONG cur = 0x10000;
    int hits = 0;
    while (cur < 0x00007FFFFFFEFFFFull && hits < 20) {
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (m.Type == MEM_PRIVATE && readable(&m) && m.RegionSize >= (64u << 10)) {
            for (ULONGLONG off = 0; off + 0x100000 <= m.RegionSize + 0x100000 && hits < 20; off += (1 << 20)) {
                SIZE_T n = (SIZE_T)(m.RegionSize - off > (1 << 20) ? (1 << 20) : m.RegionSize - off);
                read_into(p, (char *)m.BaseAddress + off, buf, n);
                for (SIZE_T i = 0; i + plen <= n; i++) {
                    if (!memcmp(buf + i, pat, plen)) {
                        printf("%llX\n", (unsigned long long)((char *)m.BaseAddress + off + i));
                        hits++;
                        if (hits >= 20) break;
                    }
                }
            }
        }
        if (next <= cur) cur += 0x1000; else cur = next;
    }
    printf("hits=%d\n", hits);
}

static void do_bigregions(HANDLE p) {
    MEMORY_BASIC_INFORMATION m;
    ULONGLONG cur = 0x10000;
    while (cur < 0x00007FFFFFFEFFFFull) {
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (m.RegionSize >= (32u << 20))
            printf("%llX size=%llXMB type=%lx prot=%lx\n",
                   (unsigned long long)(ULONG_PTR)m.BaseAddress,
                   (unsigned long long)(m.RegionSize >> 20), m.Type, m.Protect);
        if (next <= cur) cur += 0x1000; else cur = next;
    }
}

static void do_findrange(HANDLE p, unsigned a, unsigned b) {
    unsigned char *buf = malloc((1 << 20) + 8);
    ULONGLONG cur = 0x10000;
    int hits = 0;
    while (cur < 0x00007FFFFFFEFFFFull && hits < 200000) {
        MEMORY_BASIC_INFORMATION m;
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (readable(&m) && (!g_privonly || m.Type == MEM_PRIVATE)) {
            for (ULONGLONG off = 0; off < m.RegionSize; off += (1 << 20)) {
                SIZE_T n = (SIZE_T)(m.RegionSize - off > (1 << 20) ? (1 << 20) : m.RegionSize - off);
                read_into(p, (char *)m.BaseAddress + off, buf, n);
                for (SIZE_T i = 0; i + 4 <= n; i += 4) {
                    unsigned x;
                    memcpy(&x, buf + i, 4);
                    if (x >= a && x <= b)
                        printf("%llX %u\n", (unsigned long long)((char *)m.BaseAddress + off + i), x);
                }
            }
        }
        if (next <= cur) cur += 0x1000; else cur = next;
    }
}

static int cmpull(const void *x, const void *y) {
    unsigned long long a = *(const unsigned long long *)x, b = *(const unsigned long long *)y;
    return a < b ? -1 : a > b;
}

static int g_rangepriv = 0;

typedef struct { unsigned long long addr; unsigned val; } HIT;

static int cmphit(const void *x, const void *y) {
    unsigned long long a = ((const HIT *)x)->addr, b = ((const HIT *)y)->addr;
    return a < b ? -1 : a > b;
}

#define MAXHIT 2000000

static void do_findrange4(HANDLE p, unsigned lo, unsigned hi) {
    unsigned char *buf = malloc((1 << 20) + 8);
    ULONGLONG cur = 0x10000;
    HIT *hit = malloc((size_t)MAXHIT * sizeof(HIT));
    int n = 0;
    while (cur < 0x00007FFFFFFEFFFFull && n < MAXHIT) {
        MEMORY_BASIC_INFORMATION m;
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (readable(&m) && (!g_rangepriv || m.Type == MEM_PRIVATE)) {
            for (ULONGLONG off = 0; off < m.RegionSize && n < MAXHIT; off += (1 << 20)) {
                SIZE_T sz = (SIZE_T)(m.RegionSize - off > (1 << 20) ? (1 << 20) : m.RegionSize - off);
                read_into(p, (char *)m.BaseAddress + off, buf, sz);
                for (SIZE_T i = 0; i + 4 <= sz && n < MAXHIT; i += 4) {
                    unsigned x;
                    memcpy(&x, buf + i, 4);
                    if (x >= lo && x <= hi) {
                        hit[n].addr = (unsigned long long)((char *)m.BaseAddress + off + i);
                        hit[n].val = x;
                        n++;
                    }
                }
            }
        }
        if (next <= cur) cur += 0x1000; else cur = next;
    }
    qsort(hit, n, sizeof(HIT), cmphit);
    for (int i = 0; i < n; i++) printf("%llX %u\n", hit[i].addr, hit[i].val);
    fprintf(stderr, "n=%d\n", n);
}

static void do_diffcheck(HANDLE p, const char *path) {
    FILE *f = fopen(path, "r");
    if (!f) { fprintf(stderr, "no list\\n"); return; }
    char line[64];
    unsigned long long a;
    while (fgets(line, sizeof line, f)) {
        if (sscanf(line, "%llx", &a) != 1) continue;
        unsigned v = 0; SIZE_T got = 0;
        if (ReadProcessMemory(p, (void *)(ULONG_PTR)a, &v, 4, &got) && got == 4) printf("%llX %u\n", a, v);
    }
    fclose(f);
}

static void do_findseq4(HANDLE p, unsigned a, unsigned b, unsigned c, unsigned d) {
    unsigned char *buf = malloc((1 << 20) + 8);
    ULONGLONG cur = 0x10000;
    int hits = 0;
    while (cur < 0x00007FFFFFFEFFFFull && hits < MAXHITS) {
        MEMORY_BASIC_INFORMATION m;
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (readable(&m) && (!g_privonly || m.Type == MEM_PRIVATE)) {
            for (ULONGLONG off = 0; off < m.RegionSize && hits < MAXHITS; off += (1 << 20)) {
                SIZE_T n = (SIZE_T)(m.RegionSize - off > (1 << 20) ? (1 << 20) : m.RegionSize - off);
                read_into(p, (char *)m.BaseAddress + off, buf, n);
                for (SIZE_T i = 0; i + 8 <= n; i += 2) {
                    unsigned short w[4];
                    memcpy(w, buf + i, 8);
                    if (w[0] == a && w[1] == b && w[2] == c && w[3] == d) {
                        printf("%llX\n", (unsigned long long)((char *)m.BaseAddress + off + i));
                        if (++hits >= MAXHITS) break;
                    }
                }
            }
        }
        if (next <= cur) cur += 0x1000; else cur = next;
    }
    printf("hits=%d\n", hits);
}

static void do_findv(HANDLE p, unsigned long v) {
    unsigned char *buf = malloc((1 << 20) + 8);
    ULONGLONG cur = 0x10000;
    int hits = 0;
    while (cur < 0x00007FFFFFFEFFFFull && hits < MAXHITS) {
        MEMORY_BASIC_INFORMATION m;
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (readable(&m) && (!g_privonly || m.Type == MEM_PRIVATE)) {
            for (ULONGLONG off = 0; off < m.RegionSize && hits < MAXHITS; off += (1 << 20)) {
                SIZE_T n = (SIZE_T)(m.RegionSize - off > (1 << 20) ? (1 << 20) : m.RegionSize - off);
                read_into(p, (char *)m.BaseAddress + off, buf, n);
                for (SIZE_T i = 0; i + 4 <= n; i += 2) {
                    unsigned long x;
                    memcpy(&x, buf + i, 4);
                    if (x == v) {
                        printf("%llX\n", (unsigned long long)((char *)m.BaseAddress + off + i));
                        if (++hits >= MAXHITS) break;
                    }
                }
            }
        }
        if (next <= cur) cur += 0x1000; else cur = next;
    }
    printf("hits=%d\n", hits);
}

static void do_find(HANDLE p, unsigned long a, unsigned long b) {
    unsigned char *buf = malloc((1 << 20) + 8);
    ULONGLONG cur = 0x10000;
    int hits = 0, scanned = 0;
    while (cur < 0x00007FFFFFFEFFFFull && hits < MAXHITS) {
        MEMORY_BASIC_INFORMATION m;
        if (VirtualQueryEx(p, (void *)cur, &m, sizeof m) != sizeof m) break;
        ULONGLONG next = (ULONGLONG)m.BaseAddress + m.RegionSize;
        if (readable(&m) && (!g_privonly || m.Type == MEM_PRIVATE)) {
            for (ULONGLONG off = 0; off < m.RegionSize && hits < MAXHITS; off += (1 << 20)) {
                SIZE_T n = (SIZE_T)(m.RegionSize - off > (1 << 20) ? (1 << 20) : m.RegionSize - off);
                SIZE_T got = 0;
                read_into(p, (char *)m.BaseAddress + off, buf, n); got = n;
                if (got < 8) continue;
                scanned += (int)(got >> 20);
                for (SIZE_T i = 0; i + 8 <= got; i += 4) {
                    unsigned long x, y;
                    memcpy(&x, buf + i, 4);
                    memcpy(&y, buf + i + 4, 4);
                    if (x == a && y == b) {
                        printf("%llX\n", (unsigned long long)((char *)m.BaseAddress + off + i));
                        if (++hits >= MAXHITS) break;
                    }
                }
            }
        }
        if (next <= cur) cur += 0x1000; else cur = next;
    }
    printf("hits=%d scanned=%dMB\n", hits, scanned);
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: %s <pid> read|find|write ...\n", argv[0]); return 2; }
    HANDLE p = open_target((DWORD)strtoul(argv[1], NULL, 10));
    if (!strcmp(argv[2], "read")) do_read(p);
    else if (!strcmp(argv[2], "findblob") && argc >= 4) {
        char hex[512]; FILE *bf = fopen(argv[3], "r");
        int plen = 0;
        if (bf) { if (fgets(hex, sizeof hex, bf)) { for (int i = 0; hex[i] && hex[i] != 10 && plen < 256; i += 2) {
            unsigned v; sscanf(hex + i, "%2x", &v); ((unsigned char *)hex)[plen++] = (unsigned char)v; } } fclose(bf); }
        do_findblob(p, (unsigned char *)hex, plen);
    }
    else if (!strcmp(argv[2], "bigregions")) do_bigregions(p);
    else if (!strcmp(argv[2], "findchain") && argc >= 9) do_findchain(p, strtoull(argv[3],0,0), strtoull(argv[4],0,0), strtoul(argv[5],0,0), strtoul(argv[6],0,0), strtoul(argv[7],0,0), strtoul(argv[8],0,0));
    else if (!strcmp(argv[2], "findrange") && argc >= 5) do_findrange4(p, strtoul(argv[3],0,0), strtoul(argv[4],0,0));
    else if (!strcmp(argv[2], "findrangep") && argc >= 5) { g_rangepriv = 1; do_findrange4(p, strtoul(argv[3],0,0), strtoul(argv[4],0,0)); }
    else if (!strcmp(argv[2], "diffcheck") && argc >= 4) do_diffcheck(p, argv[3]);
    else if (!strcmp(argv[2], "findseq") && argc >= 7) do_findseq4(p, strtoul(argv[3],0,0), strtoul(argv[4],0,0), strtoul(argv[5],0,0), strtoul(argv[6],0,0));
    else if (!strcmp(argv[2], "findv") && argc >= 4) do_findv(p, strtoul(argv[3], NULL, 0));
    else if (!strcmp(argv[2], "findmaxp") && argc >= 4) { g_privonly = 1; do_findmax(p, strtoul(argv[3], NULL, 0)); }
    else if (!strcmp(argv[2], "findmax") && argc >= 4) do_findmax(p, strtoul(argv[3], NULL, 0));
    else if (!strcmp(argv[2], "find") && argc >= 5) do_find(p, strtoul(argv[3], NULL, 0), strtoul(argv[4], NULL, 0));
    else if (!strcmp(argv[2], "peek") && argc >= 4) {
        ULONG_PTR addr = (ULONG_PTR)strtoull(argv[3], NULL, 0);
        unsigned long v = 0; SIZE_T got = 0;
        ReadProcessMemory(p, (void *)addr, &v, 4, &got);
        printf("%llX = %lu (got=%llu)\n", (unsigned long long)addr, v, (unsigned long long)got);
    }
    else if (!strcmp(argv[2], "peek64") && argc >= 4) {
        ULONG_PTR addr = (ULONG_PTR)strtoull(argv[3], NULL, 0);
        unsigned long long v = 0; SIZE_T got = 0;
        ReadProcessMemory(p, (void *)addr, &v, 8, &got);
        printf("%llX = %llX (got=%llu)\n", (unsigned long long)addr, v, (unsigned long long)got);
    }
    else if (!strcmp(argv[2], "write") && argc >= 5) {
        ULONG_PTR addr = (ULONG_PTR)strtoull(argv[3], NULL, 0);
        unsigned long v = strtoul(argv[4], NULL, 0);
        SIZE_T w = 0;
        printf("write %llX = %lu -> %d\n", (unsigned long long)addr, v,
               WriteProcessMemory(p, (void *)addr, &v, 4, &w));
    } else { fprintf(stderr, "bad args\n"); return 2; }
    return 0;
}
