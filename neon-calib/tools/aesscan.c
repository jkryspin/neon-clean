// Brute-force AES-256 key finder over a process's readable memory (read-only, run niced).
// Tests every 4-byte-aligned 32-byte window as a key: AES-OFB with the handshake IV repeated to
// 16 bytes, decrypt the first client->server packet, accept when the character id appears.
//   gcc -O2 -maes -o aesscan aesscan.c
//   aesscan <pid> <packet_hex> <charid_hex> <iv_hex> [<iv_hex>...]
#include <immintrin.h>
#include <omp.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>

static inline void assist1(__m128i *t1, __m128i *t2) {
    __m128i t4;
    *t2 = _mm_shuffle_epi32(*t2, 0xff);
    t4 = _mm_slli_si128(*t1, 4); *t1 = _mm_xor_si128(*t1, t4);
    t4 = _mm_slli_si128(t4, 4);  *t1 = _mm_xor_si128(*t1, t4);
    t4 = _mm_slli_si128(t4, 4);  *t1 = _mm_xor_si128(*t1, t4);
    *t1 = _mm_xor_si128(*t1, *t2);
}
static inline void assist2(__m128i *t1, __m128i *t3) {
    __m128i t2, t4;
    t4 = _mm_aeskeygenassist_si128(*t1, 0);
    t2 = _mm_shuffle_epi32(t4, 0xaa);
    t4 = _mm_slli_si128(*t3, 4); *t3 = _mm_xor_si128(*t3, t4);
    t4 = _mm_slli_si128(t4, 4);  *t3 = _mm_xor_si128(*t3, t4);
    t4 = _mm_slli_si128(t4, 4);  *t3 = _mm_xor_si128(*t3, t4);
    *t3 = _mm_xor_si128(*t3, t2);
}
#define STEP(r, rc) do { t2 = _mm_aeskeygenassist_si128(t3, rc); assist1(&t1, &t2); rk[r] = t1; assist2(&t1, &t3); rk[(r)+1] = t3; } while (0)
static void expand256(const uint8_t *key, __m128i *rk) {
    __m128i t1 = _mm_loadu_si128((const __m128i *)key), t3 = _mm_loadu_si128((const __m128i *)(key + 16)), t2;
    rk[0] = t1; rk[1] = t3;
    STEP(2, 0x01); STEP(4, 0x02); STEP(6, 0x04); STEP(8, 0x08); STEP(10, 0x10); STEP(12, 0x20);
    t2 = _mm_aeskeygenassist_si128(t3, 0x40); assist1(&t1, &t2); rk[14] = t1;
}
static inline __m128i assist128(__m128i k, __m128i g) {
    g = _mm_shuffle_epi32(g, 0xff);
    k = _mm_xor_si128(k, _mm_slli_si128(k, 4));
    k = _mm_xor_si128(k, _mm_slli_si128(k, 4));
    k = _mm_xor_si128(k, _mm_slli_si128(k, 4));
    return _mm_xor_si128(k, g);
}
#define K128(i, rc) rk[i] = assist128(rk[(i)-1], _mm_aeskeygenassist_si128(rk[(i)-1], rc))
static void expand128(const uint8_t *key, __m128i *rk) {
    rk[0] = _mm_loadu_si128((const __m128i *)key);
    K128(1, 0x01); K128(2, 0x02); K128(3, 0x04); K128(4, 0x08); K128(5, 0x10);
    K128(6, 0x20); K128(7, 0x40); K128(8, 0x80); K128(9, 0x1b); K128(10, 0x36);
}
static inline __m128i enc10(const __m128i *rk, __m128i b) {
    b = _mm_xor_si128(b, rk[0]);
    for (int i = 1; i < 10; i++) b = _mm_aesenc_si128(b, rk[i]);
    return _mm_aesenclast_si128(b, rk[10]);
}
static inline __m128i enc(const __m128i *rk, __m128i b) {
    b = _mm_xor_si128(b, rk[0]);
    for (int i = 1; i < 14; i++) b = _mm_aesenc_si128(b, rk[i]);
    return _mm_aesenclast_si128(b, rk[14]);
}
static int hex(const char *s, uint8_t *out, int max) {
    int n = 0;
    while (s[0] && s[1] && n < max) { unsigned v; sscanf(s, "%2x", &v); out[n++] = v; s += 2; }
    return n;
}
static uint8_t ct[512]; static int ctlen; static uint8_t cid[16]; static int cidlen;
static uint8_t ivs[32][16]; static int nivs;

static int test(const uint8_t *key, int klen, uint64_t addr) {
    __m128i rk[15];
    if (klen == 32) expand256(key, rk); else expand128(key, rk);
    int found = 0;
    for (int v = 0; v < nivs; v++) {
        __m128i cur = _mm_loadu_si128((const __m128i *)ivs[v]);
        uint8_t pt[512]; int off = 0; int ok = 1;
        while (off < ctlen) {
            cur = klen == 32 ? enc(rk, cur) : enc10(rk, cur);
            uint8_t ks[16]; _mm_storeu_si128((__m128i *)ks, cur);
            for (int i = 0; i < 16 && off + i < ctlen; i++) pt[off + i] = ct[off + i] ^ ks[i];
            if (off == 0 && pt[1] > 0x10) { ok = 0; break; }
            off += 16;
        }
        if (!ok) continue;
        int zeros = 0; for (int i = 0; i < ctlen; i++) zeros += pt[i] == 0;
        int hasid = 0;
        for (int i = 0; i + cidlen <= ctlen; i++) if (!memcmp(pt + i, cid, cidlen)) { hasid = 1; break; }
        if (zeros >= 5) {
            found = 1;
            printf("KEY FOUND @ %#llx form#%d AES-%d key=", (unsigned long long)addr, v, klen * 8);
            for (int i = 0; i < klen; i++) printf("%02x", key[i]);
            printf(" %s zeros=%d\nplaintext: ", hasid ? "CHARID" : "zeros", zeros);
            for (int i = 0; i < ctlen; i++) printf("%02x", pt[i]);
            printf("\n"); fflush(stdout);
        }
    }
    return found;
}

int main(int argc, char **argv) {
    // Known-answer self-test (FIPS-197 AES-256).
    uint8_t k[32]; for (int i = 0; i < 32; i++) k[i] = i;
    __m128i rk[15]; expand256(k, rk);
    uint8_t p[16], c[16]; for (int i = 0; i < 16; i++) p[i] = (i << 4) | i;
    _mm_storeu_si128((__m128i *)c, enc(rk, _mm_loadu_si128((__m128i *)p)));
    const uint8_t exp[16] = {0x8e,0xa2,0xb7,0xca,0x51,0x67,0x45,0xbf,0xea,0xfc,0x49,0x90,0x4b,0x49,0x60,0x89};
    if (memcmp(c, exp, 16)) { fprintf(stderr, "AES self-test FAILED\n"); return 2; }
    if (argc < 5) { fprintf(stderr, "self-test ok; usage: aesscan pid packet_hex charid_hex iv_hex...\n"); return 0; }
    int pid = atoi(argv[1]);
    ctlen = hex(argv[2], ct, sizeof ct); cidlen = hex(argv[3], cid, sizeof cid);
    for (int i = 4; i < argc && nivs + 6 <= 32; i++) {
        uint8_t iv4[4], r4[4]; hex(argv[i], iv4, 4); for (int j = 0; j < 4; j++) r4[j] = iv4[3 - j];
        for (int j = 0; j < 16; j++) { ivs[nivs][j] = iv4[j % 4]; ivs[nivs + 1][j] = r4[j % 4]; ivs[nivs + 2][j] = j < 4 ? iv4[j] : 0; ivs[nivs + 3][j] = j >= 12 ? iv4[j - 12] : 0; ivs[nivs + 4][j] = j < 4 ? r4[j] : 0; ivs[nivs + 5][j] = j >= 12 ? r4[j - 12] : 0; }
        nivs += 6;
    }
    if (nice(10)) {}
    char path[64]; snprintf(path, sizeof path, "/proc/%d/maps", pid);
    FILE *maps = fopen(path, "r"); snprintf(path, sizeof path, "/proc/%d/mem", pid);
    int mem = open(path, O_RDONLY);
    if (!maps || mem < 0) { perror("open"); return 1; }
    char line[512]; uint64_t scanned = 0, windows = 0; int hits = 0;
    static uint8_t buf[(8 << 20) + 64];
    while (fgets(line, sizeof line, maps)) {
        unsigned long long a, b; char perms[8];
        if (sscanf(line, "%llx-%llx %7s", &a, &b, perms) != 3 || perms[0] != 'r') continue;
        if (strstr(line, "dri") || strstr(line, "nvidia")) continue;
        for (uint64_t pos = a; pos < b; ) {
            size_t want = b - pos > (8 << 20) ? (8 << 20) : b - pos;
            ssize_t n = pread(mem, buf, want + 32 <= sizeof buf ? want + 32 : want, pos);
            if (n < 32) break;
            scanned += n;
            ssize_t lim = (ssize_t)want < n - 31 ? (ssize_t)want : n - 31;
            #pragma omp parallel for schedule(static) reduction(+:hits,windows)
            for (ssize_t off = 0; off < lim; off += 4) {
                // A real key looks random: need many distinct bytes in the first 16.
                uint8_t seen[256] = {0}; int distinct = 0;
                for (int i = 0; i < 16; i++) if (!seen[buf[off + i]]++) distinct++;
                if (distinct < 11) continue;
                windows++;
                hits += test(buf + off, 32, pos + off);
                hits += test(buf + off, 16, pos + off);
            }
            pos += want;
        }
    }
    printf("scanned %llu MB, %llu windows, %d hits\n", (unsigned long long)(scanned >> 20), (unsigned long long)windows, hits);
    return 0;
}
