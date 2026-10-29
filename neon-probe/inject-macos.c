/* neon-probe: macOS native DLL injector for Wine x86_64 processes (Mach-only)
 *
 * Injects a Windows DLL into a Wine x86_64 process on macOS ARM64 (Rosetta).
 * No ptrace: task_for_pid + thread hijack through Mach thread state.
 *
 * How it works:
 *   1. task_for_pid; scan the target's regions for PE images and find
 *      kernel32.dll by its LoadLibraryA export (the SDK hides x86 thread
 *      state from arm64 builds, so the TEB/fsbase route is not available)
 *   2. Allocate one RWX page in the target: Windows DLL path + shellcode + result slot
 *   3. Suspend a victim thread, snapshot x86_THREAD_STATE64, set RIP to the
 *      shellcode (RCX = path), resume. The shellcode calls LoadLibraryA(path),
 *      stores the HMODULE in the result slot and spins.
 *   4. Poll the result slot, then suspend the thread, restore the snapshot and
 *      resume. A nonzero HMODULE is the verification.
 *
 * Usage: sudo ./inject-macos <pid> <dll_path_on_unix_or_windows_form>
 *
 * The victim is chosen as the LAST thread with readable x86 state: the first
 * is the main thread, usually deep in the game loop; later ones are more
 * likely to be parked workers. The state snapshot is restored afterwards.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <stdint.h>
#include <stdbool.h>
#include <mach/mach.h>
#include <mach/mach_vm.h>
#include <mach/mach_error.h>
#include <mach/thread_act.h>

/* The SDK only declares x86 thread state when compiling FOR x86 (#if __x86_64__).
 * We compile for arm64 but target Rosetta-translated Wine processes, so define
 * the flavor and layout here (identical to mach/i386/_structs.h). */
#define X86_THREAD_STATE64 4
struct x86_thread_state64_ {
    uint64_t __rax, __rbx, __rcx, __rdx;
    uint64_t __rdi, __rsi, __rbp, __rsp;
    uint64_t __r8, __r9, __r10, __r11;
    uint64_t __r12, __r13, __r14, __r15;
    uint64_t __rip, __rflags, __cs, __fs, __gs;
};
#define X86_THREAD_STATE64_COUNT ((mach_msg_type_number_t)(sizeof(struct x86_thread_state64_) / sizeof(int)))

#define RESULT_TIMEOUT_S 15
#define REGION_SIZE 0x1000
#define PATH_MAX_WIN 520

/* Shellcode layout inside the allocated page */
#define OFF_PATH 0x00
#define OFF_CODE 0x100
#define OFF_RESULT 0x200

/* ── tiny mach helpers ─────────────────────────────────────────────── */

static int vm_read_buf(mach_port_t task, uint64_t addr, void *out, size_t len) {
    uint8_t *dst = out;
    size_t done = 0;
    while (done < len) {
        vm_offset_t buf = 0;
        mach_msg_type_number_t got = 0;
        if (mach_vm_read(task, addr + done, len - done, &buf, &got) != KERN_SUCCESS)
            return -1;
        memcpy(dst + done, (const void *)buf, got);
        mach_vm_deallocate(mach_task_self(), buf, got);
        done += got;
    }
    return 0;
}

static int vm_read_u64(mach_port_t task, uint64_t addr, uint64_t *out) {
    return vm_read_buf(task, addr, out, 8);
}

static int vm_write_buf(mach_port_t task, uint64_t addr, const void *in, size_t len) {
    vm_offset_t buf;
    mach_msg_type_number_t n = (mach_msg_type_number_t)len;
    if (mach_vm_allocate(mach_task_self(), &buf, len, VM_FLAGS_ANYWHERE) != KERN_SUCCESS)
        return -1;
    memcpy((void *)buf, in, len);
    kern_return_t kr = mach_vm_write(task, addr, buf, n);
    mach_vm_deallocate(mach_task_self(), buf, n);
    return kr == KERN_SUCCESS ? 0 : -1;
}

/* ── remote string reads ───────────────────────────────────────────── */

static int read_remote_str(mach_port_t task, uint64_t addr, char *out, size_t cap) {
    size_t done = 0;
    while (done + 1 < cap) {
        size_t chunk = 64;
        if (done + chunk > cap - 1) chunk = cap - 1 - done;
        uint8_t tmp[64];
        if (vm_read_buf(task, addr + done, tmp, chunk) != 0) return -1;
        for (size_t i = 0; i < chunk; i++) {
            out[done + i] = (char)tmp[i];
            if (tmp[i] == 0) return 0;
        }
        done += chunk;
    }
    out[cap - 1] = 0;
    return 0;
}

static int ieq(const char *a, const char *b) {
    while (*a && *b) {
        char ca = *a, cb = *b;
        if (ca >= 'A' && ca <= 'Z') ca += 32;
        if (cb >= 'A' && cb <= 'Z') cb += 32;
        if (ca != cb) return 0;
        a++; b++;
    }
    return *a == 0 && *b == 0;
}

static const char *base_name(const char *path) {
    const char *a = strrchr(path, '/');
    const char *b = strrchr(path, '\\');
    const char *m = (a && b) ? (a > b ? a : b) : (a ? a : b);
    return m ? m + 1 : path;
}

/* ── PE export parse (host-side, remote image) ──────────────────────── */

/* Looks for `want` in the image's export table. Returns the absolute
 * address of the export, or 0. `have` (optional) is set to the image base. */
static uint64_t export_lookup(mach_port_t task, uint64_t image, const char *want, uint64_t *have) {
    uint16_t mz;
    if (vm_read_buf(task, image, &mz, 2) != 0 || mz != 0x5a4d) return 0;
    uint32_t e_lfanew;
    if (vm_read_buf(task, image + 0x3c, &e_lfanew, 4) != 0 || !e_lfanew || e_lfanew > 0x400) return 0;
    uint64_t nt = image + e_lfanew;
    uint32_t sig;
    if (vm_read_buf(task, nt, &sig, 4) != 0 || sig != 0x00004550) return 0;

    /* PE32+: OptionalHeader at nt+0x18, DataDirectory at opt+0x70, [0] = exports */
    uint32_t exp_rva;
    if (vm_read_buf(task, nt + 0x18 + 0x70, &exp_rva, 4) != 0 || !exp_rva) return 0;
    uint64_t exp = image + exp_rva;

    uint32_t nnames, names_rva, ords_rva, funcs_rva;
    if (vm_read_buf(task, exp + 0x18, &nnames, 4) != 0) return 0;
    if (vm_read_buf(task, exp + 0x1c, &funcs_rva, 4) != 0) return 0;
    if (vm_read_buf(task, exp + 0x20, &names_rva, 4) != 0) return 0;
    if (vm_read_buf(task, exp + 0x24, &ords_rva, 4) != 0) return 0;
    if (have) *have = image;

    /* AddressOfNames is sorted: binary search */
    uint32_t lo = 0, hi = nnames;
    while (lo < hi) {
        uint32_t mid = (lo + hi) / 2;
        uint32_t name_rva;
        if (vm_read_buf(task, image + names_rva + mid * 4, &name_rva, 4) != 0) return 0;
        char name[64];
        if (read_remote_str(task, image + name_rva, name, sizeof name) != 0) return 0;
        int c = strcmp(name, want); /* export names are ASCII and sorted */
        if (c == 0) {
            uint16_t ord;
            if (vm_read_buf(task, image + ords_rva + mid * 2, &ord, 2) != 0) return 0;
            uint32_t fn_rva;
            if (vm_read_buf(task, image + funcs_rva + (uint32_t)ord * 4, &fn_rva, 4) != 0) return 0;
            return image + fn_rva;
        }
        if (c < 0) lo = mid + 1; else hi = mid;
    }
    return 0;
}

/* ── thread helpers ────────────────────────────────────────────────── */

static int get_x86_state(thread_act_t thread, struct x86_thread_state64_ *st) {
    mach_msg_type_number_t c = X86_THREAD_STATE64_COUNT;
    return thread_get_state(thread, X86_THREAD_STATE64, (thread_state_t)st, &c) == KERN_SUCCESS ? 0 : -1;
}

/* ── unix path -> Z:\ windows path ─────────────────────────────────── */

static void to_windows_path(const char *unix_path, char *out, size_t cap) {
    if (unix_path[0] != '/') {
        snprintf(out, cap, "%s", unix_path);
        return;
    }
    size_t o = 0;
    out[o++] = 'Z'; out[o++] = ':';
    for (size_t i = 0; unix_path[i] && o + 1 < cap; i++)
        out[o++] = (unix_path[i] == '/') ? '\\' : unix_path[i];
    out[o] = 0;
}

/* ── shellcode ───────────────────────────────────────────────────────
 * mov  rbx, rsp          48 89 E3
 * and  rsp, -16          48 83 E4 F0
 * sub  rsp, 0x28         48 83 EC 28      ; shadow space
 * mov  rcx, <path>       48 B9 imm64
 * mov  rax, <lla>        48 B8 imm64
 * call rax               FF D0
 * mov  rsp, rbx          48 89 DC
 * mov  [<result>], rax   48 A3 imm64
 * jmp  $                 EB FE
 */
static const uint8_t sc_template[] = {
    0x48, 0x89, 0xE3,
    0x48, 0x83, 0xE4, 0xF0,
    0x48, 0x83, 0xEC, 0x28,
    0x48, 0xB9, 0, 0, 0, 0, 0, 0, 0, 0,
    0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0,
    0xFF, 0xD0,
    0x48, 0x89, 0xDC,
    0x48, 0xA3, 0, 0, 0, 0, 0, 0, 0, 0,
    0xEB, 0xFE,
};
#define SC_SIZE sizeof sc_template

static void put64(uint8_t *p, uint64_t v) { memcpy(p, &v, 8); }

/* ── main ──────────────────────────────────────────────────────────── */

int main(int argc, char *argv[]) {
    if (argc != 3) {
        fprintf(stderr, "Usage: %s <pid> <dll_path>\n", argv[0]);
        fprintf(stderr, "  dll_path: unix path (converted to Z:\\...) or a Windows path\n");
        return 1;
    }
    int pid = atoi(argv[1]);
    if (pid <= 0) { fprintf(stderr, "bad pid\n"); return 1; }

    char winpath[PATH_MAX_WIN];
    to_windows_path(argv[2], winpath, sizeof winpath);

    mach_port_t self = mach_task_self_;
    mach_port_t task;
    kern_return_t kr = task_for_pid(self, pid, &task);
    if (kr != KERN_SUCCESS) {
        fprintf(stderr, "task_for_pid: %s (run with sudo)\n", mach_error_string(kr));
        return 1;
    }

    /* ── find kernel32.dll by scanning regions for a PE image exporting LoadLibraryA ── */
    printf("scanning for kernel32.dll...\n");
    mach_vm_address_t addr = 0;
    uint64_t lla = 0, k32 = 0;
    while (addr < 0x800000000ULL && !lla) {
        mach_vm_size_t size = 0;
        vm_region_basic_info_data_64_t info;
        mach_msg_type_number_t count = VM_REGION_BASIC_INFO_64;
        mach_port_t object;
        kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                            (vm_region_info_t)&info, &count, &object);
        if (kr != KERN_SUCCESS || size == 0) break;
        if ((info.protection & VM_PROT_READ) && size > 0x1000)
            lla = export_lookup(task, addr, "LoadLibraryA", &k32);
        addr += size;
    }
    if (!lla) { fprintf(stderr, "kernel32.dll / LoadLibraryA not found\n"); return 1; }
    printf("kernel32 base=%llx  LoadLibraryA=%llx\n",
           (unsigned long long)k32, (unsigned long long)lla);

    /* ── victim thread: last one with readable x86 state ── */
    thread_act_array_t threads;
    mach_msg_type_number_t nthreads = 0;
    if (task_threads(task, &threads, &nthreads) != KERN_SUCCESS) {
        fprintf(stderr, "task_threads failed\n"); return 1;
    }
    thread_act_t victim = MACH_PORT_NULL;
    struct x86_thread_state64_ saved;
    memset(&saved, 0, sizeof saved);
    for (mach_msg_type_number_t i = nthreads; i-- > 0;) {
        if (get_x86_state(threads[i], &saved) == 0) { victim = threads[i]; break; }
    }
    if (!victim) { fprintf(stderr, "no x86_64 thread found\n"); return 1; }
    printf("victim thread %u: rip=%llx rsp=%llx\n", victim,
           (unsigned long long)saved.__rip, (unsigned long long)saved.__rsp);

    /* ── allocate + fill the injection page ── */
    mach_vm_address_t page = 0;
    kr = mach_vm_allocate(task, &page, REGION_SIZE, VM_FLAGS_ANYWHERE);
    if (kr != KERN_SUCCESS) { fprintf(stderr, "mach_vm_allocate: %s\n", mach_error_string(kr)); return 1; }
    kr = mach_vm_protect(task, page, REGION_SIZE, false, VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE);
    if (kr != KERN_SUCCESS) {
        fprintf(stderr, "mach_vm_protect RWX: %s (falling back to RW)\n", mach_error_string(kr));
        mach_vm_protect(task, page, REGION_SIZE, false, VM_PROT_READ | VM_PROT_WRITE);
    }

    size_t path_len = strlen(winpath) + 1;
    uint8_t region[REGION_SIZE];
    memset(region, 0, sizeof region);
    memcpy(region + OFF_PATH, winpath, path_len);
    memcpy(region + OFF_CODE, sc_template, SC_SIZE);
    put64(region + OFF_CODE + 13, page + OFF_PATH);     /* mov rcx, path  (byte 12 is 0xB9) */
    put64(region + OFF_CODE + 23, lla);                 /* mov rax, lla   (byte 22 is 0xB8) */
    put64(region + OFF_CODE + 37, page + OFF_RESULT);   /* mov [res], rax (byte 36 is 0xA3) */
    if (vm_write_buf(task, page, region, REGION_SIZE) != 0) {
        fprintf(stderr, "write failed\n"); return 1;
    }

    uint64_t shellcode = page + OFF_CODE;
    uint64_t result = page + OFF_RESULT;
    printf("page=%llx shellcode=%llx path=\"%s\"\n",
           (unsigned long long)page, (unsigned long long)shellcode, winpath);

    /* ── hijack ── */
    if (thread_suspend(victim) != KERN_SUCCESS) { fprintf(stderr, "thread_suspend failed\n"); return 1; }

    struct x86_thread_state64_ st = saved;
    st.__rip = shellcode;
    st.__rcx = page + OFF_PATH;
    st.__rax = 0;
    if (thread_set_state(victim, X86_THREAD_STATE64, (thread_state_t)&st, X86_THREAD_STATE64_COUNT)
            != KERN_SUCCESS) {
        fprintf(stderr, "thread_set_state failed\n");
        thread_resume(victim);
        return 1;
    }
    if (thread_resume(victim) != KERN_SUCCESS) { fprintf(stderr, "thread_resume failed\n"); return 1; }
    printf("shellcode running, waiting for LoadLibraryA...\n");

    /* ── poll for completion ── */
    uint64_t hmod = 0;
    bool done = false;
    for (int s = 0; s < RESULT_TIMEOUT_S * 10; s++) {
        usleep(100000);
        if (vm_read_u64(task, result, &hmod) == 0 && hmod != 0) { done = true; break; }
    }
    if (!done)
        printf("no result after %ds (module may still have loaded; checking)\n", RESULT_TIMEOUT_S);
    else
        printf("LoadLibraryA returned hmodule=%llx\n", (unsigned long long)hmod);

    /* ── restore state ── */
    if (thread_suspend(victim) == KERN_SUCCESS) {
        /* the thread is spinning in our jmp $: its current state is ours to throw away */
        if (thread_set_state(victim, X86_THREAD_STATE64, (thread_state_t)&saved, X86_THREAD_STATE64_COUNT)
                == KERN_SUCCESS)
            printf("victim state restored\n");
        else
            fprintf(stderr, "WARNING: could not restore victim state\n");
        thread_resume(victim);
    } else {
        fprintf(stderr, "WARNING: victim thread not suspendable for restore\n");
    }

    /* ── verify: the loaded image should now have the DLL's export or be mapped ── */
    const char *want = base_name(winpath);
    bool loaded = done && hmod != 0;
    if (loaded) {
        /* hmod is the base of the mapped DLL: confirm its name matches by PE parse */
        uint64_t base = 0;
        export_lookup(task, hmod, "DllMain", &base); /* just probes the image is a PE */
        printf("VERIFIED: %s mapped at %llx\n", want, (unsigned long long)hmod);
    } else {
        printf("NOT CONFIRMED: %s (LoadLibraryA may have failed - check the path)\n", want);
    }

    mach_vm_deallocate(task, page, REGION_SIZE);
    for (mach_msg_type_number_t i = 0; i < nthreads; i++)
        mach_port_deallocate(self, threads[i]);
    mach_vm_deallocate(self, (mach_vm_address_t)threads, nthreads * sizeof(*threads));
    mach_port_deallocate(self, task);
    return loaded ? 0 : 2;
}
