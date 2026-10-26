// Mode-flexible AES key finder: packets whose plaintext is a constant 2-byte opcode (keep-alives).
// For the right key and IV layout, AES_K(layout(IV_n))[0:2] ^ ct_n is the same for every ping.
//   gcc -O2 -maes -fopenmp -o aesscan2 aesscan2.c ;  aesscan2 <pid> <pings.txt>   (lines: "<iv hex> <ct hex>")
#include <immintrin.h>
#include <omp.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#define MAXP 64
#define NFORMS 12
static uint8_t ivb[MAXP][4], ctb[MAXP][2]; static int np;
static void layout(int f, const uint8_t *iv, uint8_t out[16]) {
    uint8_t r[4] = {iv[3], iv[2], iv[1], iv[0]};
    const uint8_t *s = f < 6 ? iv : r;
    switch (f % 6) {
    case 0: for (int j = 0; j < 16; j++) out[j] = s[j % 4]; break;                       // IV x4
    case 1: memset(out, 0, 16); memcpy(out, s, 4); break;                                  // IV, zero pad
    case 2: memset(out, 0, 16); memcpy(out + 12, s, 4); break;                             // zero pad, IV
    case 3: memset(out, 0, 16); memcpy(out + 8, s, 4); break;
    case 4: for (int j = 0; j < 16; j++) out[j] = s[j % 4] ^ 0; out[0] = s[0]; break;      // (kept = x4)
    case 5: memset(out, 0xff, 16); memcpy(out, s, 4); break;
    }
}
static inline __m128i assist128(__m128i k, __m128i g) { g = _mm_shuffle_epi32(g, 0xff); k = _mm_xor_si128(k, _mm_slli_si128(k, 4)); k = _mm_xor_si128(k, _mm_slli_si128(k, 4)); k = _mm_xor_si128(k, _mm_slli_si128(k, 4)); return _mm_xor_si128(k, g); }
#define K128(i, rc) rk[i] = assist128(rk[(i)-1], _mm_aeskeygenassist_si128(rk[(i)-1], rc))
static void expand128(const uint8_t *key, __m128i *rk) { rk[0] = _mm_loadu_si128((const __m128i *)key); K128(1,1); K128(2,2); K128(3,4); K128(4,8); K128(5,16); K128(6,32); K128(7,64); K128(8,128); K128(9,0x1b); K128(10,0x36); }
static inline void a1(__m128i *t1, __m128i *t2) { __m128i t4; *t2 = _mm_shuffle_epi32(*t2, 0xff); t4 = _mm_slli_si128(*t1, 4); *t1 = _mm_xor_si128(*t1, t4); t4 = _mm_slli_si128(t4, 4); *t1 = _mm_xor_si128(*t1, t4); t4 = _mm_slli_si128(t4, 4); *t1 = _mm_xor_si128(*t1, t4); *t1 = _mm_xor_si128(*t1, *t2); }
static inline void a2(__m128i *t1, __m128i *t3) { __m128i t2, t4; t4 = _mm_aeskeygenassist_si128(*t1, 0); t2 = _mm_shuffle_epi32(t4, 0xaa); t4 = _mm_slli_si128(*t3, 4); *t3 = _mm_xor_si128(*t3, t4); t4 = _mm_slli_si128(t4, 4); *t3 = _mm_xor_si128(*t3, t4); t4 = _mm_slli_si128(t4, 4); *t3 = _mm_xor_si128(*t3, t4); *t3 = _mm_xor_si128(*t3, t2); }
#define S256(r, rc) do { t2 = _mm_aeskeygenassist_si128(t3, rc); a1(&t1, &t2); rk[r] = t1; a2(&t1, &t3); rk[(r)+1] = t3; } while (0)
static void expand256(const uint8_t *key, __m128i *rk) { __m128i t1 = _mm_loadu_si128((const __m128i *)key), t3 = _mm_loadu_si128((const __m128i *)(key + 16)), t2; rk[0] = t1; rk[1] = t3; S256(2,1); S256(4,2); S256(6,4); S256(8,8); S256(10,16); S256(12,32); t2 = _mm_aeskeygenassist_si128(t3, 64); a1(&t1, &t2); rk[14] = t1; }
static inline __m128i enc(const __m128i *rk, int rounds, __m128i b) { b = _mm_xor_si128(b, rk[0]); for (int i = 1; i < rounds; i++) b = _mm_aesenc_si128(b, rk[i]); return _mm_aesenclast_si128(b, rk[rounds]); }
static int test(const uint8_t *key, int klen, uint64_t addr) {
    __m128i rk[15]; int rounds = klen == 32 ? 14 : 10;
    if (klen == 32) expand256(key, rk); else expand128(key, rk);
    int found = 0;
    for (int f = 0; f < NFORMS; f++) {
        uint16_t v[MAXP];
        for (int n = 0; n < np; n++) {
            uint8_t b[16], o[16]; layout(f, ivb[n], b);
            _mm_storeu_si128((__m128i *)o, enc(rk, rounds, _mm_loadu_si128((__m128i *)b)));
            v[n] = (uint16_t)(((o[0] ^ ctb[n][0]) << 8) | (o[1] ^ ctb[n][1]));
            // Cheap early out: among the first 4 pings at least two must already agree.
            if (n == 3) {
                int agree = 0;
                for (int i = 0; i < 4; i++) for (int j = i + 1; j < 4; j++) agree += v[i] == v[j];
                if (!agree) goto next_form;
            }
        }
        {
            int best = 0; uint16_t bv = 0;
            for (int i = 0; i < np; i++) { int c = 0; for (int j = 0; j < np; j++) c += v[i] == v[j]; if (c > best) { best = c; bv = v[i]; } }
            if (best >= (np < 8 ? np : 8)) {
                #pragma omp critical
                { printf("KEY @ %#llx AES-%d layout#%d agree=%d/%d opcode_bytes=%04x key=", (unsigned long long)addr, klen * 8, f, best, np, bv); for (int i = 0; i < klen; i++) printf("%02x", key[i]); printf("\n"); fflush(stdout); }
                found = 1;
            }
        }
        next_form:;
    }
    return found;
}
int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: aesscan2 pid pings.txt\n"); return 1; }
    FILE *pf = fopen(argv[2], "r"); char l1[64], l2[64];
    while (np < MAXP && fscanf(pf, "%63s %63s", l1, l2) == 2) { for (int i = 0; i < 4; i++) { unsigned v; sscanf(l1 + 2 * i, "%2x", &v); ivb[np][i] = v; } for (int i = 0; i < 2; i++) { unsigned v; sscanf(l2 + 2 * i, "%2x", &v); ctb[np][i] = v; } np++; }
    fprintf(stderr, "%d pings\n", np);
    int pid = atoi(argv[1]); char path[64]; snprintf(path, sizeof path, "/proc/%d/maps", pid); FILE *maps = fopen(path, "r"); snprintf(path, sizeof path, "/proc/%d/mem", pid); int mem = open(path, O_RDONLY);
    if (nice(10)) {}
    char line[512]; uint64_t scanned = 0, wins = 0; int hits = 0; static uint8_t buf[(8 << 20) + 64];
    while (fgets(line, sizeof line, maps)) {
        unsigned long long a, b; char perms[8];
        if (sscanf(line, "%llx-%llx %7s", &a, &b, perms) != 3 || perms[0] != 'r' || strstr(line, "dri") || strstr(line, "nvidia")) continue;
        for (uint64_t pos = a; pos < b; ) {
            size_t want = b - pos > (8 << 20) ? (8 << 20) : b - pos; ssize_t n = pread(mem, buf, want + 32 <= sizeof buf ? want + 32 : want, pos);
            if (n < 32) break; scanned += n; ssize_t lim = (ssize_t)want < n - 31 ? (ssize_t)want : n - 31;
            #pragma omp parallel for schedule(static) reduction(+:hits,wins)
            for (ssize_t off = 0; off < lim; off += 4) {
                uint8_t seen[256] = {0}; int d = 0; for (int i = 0; i < 16; i++) if (!seen[buf[off + i]]++) d++;
                if (d < 11) continue; wins++;
                hits += test(buf + off, 32, pos + off); hits += test(buf + off, 16, pos + off);
            }
            pos += want;
        }
    }
    printf("scanned %llu MB, %llu windows, %d hits\n", (unsigned long long)(scanned >> 20), (unsigned long long)wins, hits);
    return 0;
}
