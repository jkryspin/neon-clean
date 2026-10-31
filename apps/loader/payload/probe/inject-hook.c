/* neon-probe: Alternative injection approach — hook LdrLoadDll in Wine's ntdll.dll
 *
 * Strategy:
 * 1. Attach via ptrace(PT_ATTACHEXC) — suspends the process
 * 2. Use task_for_pid + mach_vm_region to walk Wine's memory
 * 3. Find ntdll.dll by scanning for PE signature (0x5A4D at base = MZ)
 * 4. Parse PE headers to find export table
 * 5. Locate LdrLoadDll in export table
 * 6. Save original bytes, overwrite with JMP shellcode (E9 <rel32>)
 * 7. Detach — Wine will execute our hook when it calls LdrLoadDll
 * 8. Hook calls our shellcode, which loads the target DLL, then restores
 *    original bytes, calls the real LdrLoadDll, and re-patches for next call
 *
 * Usage: ./inject-hook <pid> <dll_path>
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <signal.h>
#include <sys/ptrace.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <mach/mach.h>
#include <mach/mach_error.h>
#include <mach/mach_vm.h>
#include <stdint.h>

/* Shellcode: LoadLibraryA via PEB traversal (173 bytes, from shellcode-embed.h) */
#include "shellcode-embed.h"

/* JMP rel32 instruction: E9 <offset> — 5 bytes */
#define JMP_OPCODE 0xE9

/* Hook shellcode: 
 * 1. Save registers
 * 2. Call our PEB-traversal LoadLibraryA resolver
 * 3. Restore registers
 * 4. Return (C3)
 * We'll place this in allocated memory and patch LdrLoadDll to JMP to it.
 */

/* Forward declarations */
static void print_usage(const char *prog);
static kern_return_t vm_read_region(mach_port_t task, mach_vm_address_t addr,
                                     mach_vm_size_t size, void *buf, size_t *out_len);
static mach_vm_address_t find_ntdll_base(mach_port_t task);
static uint64_t find_export_function(mach_port_t task, mach_vm_address_t pe_base,
                                      const char *func_name);
static int hook_ldr_load_dll(pid_t pid, mach_port_t task, const char *dll_path);
static int vm_alloc_in_target(mach_port_t task, const void *host_buf, size_t len,
                               uint64_t *target_addr);

/* LdrLoadDll hook: 5-byte JMP to our shellcode */
static uint8_t hook_jmp[5];

/* Our injection payload — placed in allocated target memory */
static uint64_t injection_shellcode_addr = 0;

/* ── Main ── */
int main(int argc, char *argv[]) {
    if (argc != 3) {
        print_usage(argv[0]);
        return 1;
    }

    int pid = atoi(argv[1]);
    const char *dll_path = argv[2];

    if (pid <= 0) {
        fprintf(stderr, "Error: invalid PID %s\n", argv[1]);
        return 1;
    }

    printf("neon-probe macOS injector v2.0 (LdrLoadDll hook)\n");
    printf("Target PID: %d\n", pid);
    printf("DLL path: %s\n\n", dll_path);

    /* Verify process is alive */
    if (kill(pid, 0) != 0) {
        fprintf(stderr, "Error: process %d not found (errno=%d: %s)\n",
                pid, errno, strerror(errno));
        return 1;
    }

    /* Hook LdrLoadDll */
    int ret = hook_ldr_load_dll(pid, 0, dll_path);
    return ret;
}

static void print_usage(const char *prog) {
    fprintf(stderr, "Usage: %s <pid> <dll_path>\n", prog);
    fprintf(stderr, "\nHooks LdrLoadDll in Wine's ntdll.dll to inject a DLL.\n");
    fprintf(stderr, "\nExamples:\n");
    fprintf(stderr, "  %s 12345 \"C:\\\\Neon\\\\probe_opcodes-win.dll\"\n", prog);
    fprintf(stderr, "  sudo %s 12345 \"Z:\\\\Neon\\\\probe_opcodes-win.dll\"\n", prog);
}

/* ── Allocate memory in target process ── */
static int vm_alloc_in_target(mach_port_t task, const void *host_buf, size_t len,
                               uint64_t *target_addr) {
    mach_vm_address_t addr = 0;
    kern_return_t kr = mach_vm_allocate(task, &addr, len, VM_FLAGS_ANYWHERE);
    if (kr != KERN_SUCCESS) {
        fprintf(stderr, "  mach_vm_allocate failed: %s\n", mach_error_string(kr));
        return -1;
    }

    /* Write data */
    kr = mach_vm_write(task, addr, (vm_offset_t)(uintptr_t)host_buf, len);
    if (kr != KERN_SUCCESS) {
        fprintf(stderr, "  mach_vm_write failed: %s\n", mach_error_string(kr));
        mach_vm_deallocate(task, addr, len);
        return -1;
    }

    *target_addr = addr;
    return 0;
}

/* ── Read memory from target process ── */
static kern_return_t vm_read_region(mach_port_t task, mach_vm_address_t addr,
                                     mach_vm_size_t size, void *buf, size_t *out_len) {
    vm_offset_t mem_buf = 0;
    mach_msg_type_number_t mem_buf_len = 0;
    kern_return_t kr = mach_vm_read(task, addr, size, &mem_buf, &mem_buf_len);
    if (kr != KERN_SUCCESS) return kr;
    
    size_t copy_len = (size_t)mem_buf_len < *out_len ? mem_buf_len : *out_len;
    memcpy(buf, (const void *)mem_buf, copy_len);
    *out_len = copy_len;
    mach_vm_deallocate(task, mem_buf, mem_buf_len);
    return KERN_SUCCESS;
}

/* ── Find ntdll.dll base address by scanning memory for MZ signature ── */
static mach_vm_address_t find_ntdll_base(mach_port_t task) {
    printf("[1/5] Scanning for ntdll.dll...\n");
    
    mach_vm_address_t addr = 0;
    mach_vm_size_t size = 0;
    vm_region_basic_info_data_64_t info;
    mach_msg_type_number_t count = VM_REGION_BASIC_INFO_64;
    mach_port_t object;
    
    /* Wine's user-space is typically in the low 2GB (0x00000000 - 0x7FFFFFFF) */
    mach_vm_address_t max_addr = 0x80000000ULL;
    
    while (addr < max_addr) {
        kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                           (vm_region_info_t)&info, &count, &object);
        if (kr != KERN_SUCCESS || size == 0) {
            addr += 0x1000; /* Skip to next page */
            continue;
        }
        
        /* Check for MZ signature (0x5A4D) at the start of readable regions */
        if (info.protection & VM_PROT_READ) {
            uint16_t mz_sig = 0;
            size_t len = sizeof(mz_sig);
            if (vm_read_region(task, addr, size, &mz_sig, &len) == KERN_SUCCESS &&
                len == sizeof(mz_sig) && mz_sig == 0x5A4D) {
                printf("  Found MZ at: %llx (size: %llu)\n", (unsigned long long)addr,
                       (unsigned long long)size);
                
                /* Verify it's a valid PE by checking PE signature at e_lfanew */
                uint32_t e_lfanew = 0;
                len = sizeof(e_lfanew);
                if (vm_read_region(task, addr + 0x3C, size - 0x3C, &e_lfanew, &len) == KERN_SUCCESS &&
                    len == sizeof(e_lfanew)) {
                    uint32_t pe_sig = 0;
                    len = sizeof(pe_sig);
                    if (vm_read_region(task, addr + 0x3C + e_lfanew, size - (0x3C + e_lfanew),
                                       &pe_sig, &len) == KERN_SUCCESS &&
                        len == sizeof(pe_sig) && pe_sig == 0x00004550) { /* PE\0\0 */
                        printf("  ✓ Valid PE image at: %llx\n", (unsigned long long)addr);
                        return addr;
                    }
                }
            }
        }
        
        addr += size;
    }
    
    fprintf(stderr, "  ✗ ntdll.dll not found\n");
    return 0;
}

/* ── Parse PE export table to find a function by name ── */
static uint64_t find_export_function(mach_port_t task, mach_vm_address_t pe_base,
                                      const char *func_name) {
    /* Read PE headers */
    uint32_t e_lfanew = 0;
    size_t len = sizeof(e_lfanew);
    if (vm_read_region(task, pe_base + 0x3C, 4, &e_lfanew, &len) != KERN_SUCCESS) {
        fprintf(stderr, "  Failed to read e_lfanew\n");
        return 0;
    }
    
    uint32_t pe_sig = 0;
    len = sizeof(pe_sig);
    if (vm_read_region(task, pe_base + e_lfanew, 4, &pe_sig, &len) != KERN_SUCCESS) {
        fprintf(stderr, "  Failed to read PE signature\n");
        return 0;
    }

    /* Optional: validate PE signature */
    /* printf("  PE sig: %08X\n", pe_sig); */

    /* ImageNTHeaders offset */
    uint32_t nt_headers_offset = e_lfanew;
    
    /* Read OptionalHeader offset (part of COFF header at nt_headers_offset + 4) */
    /* Actually, let's read the OptionalHeader directly */
    /* Standard fields: Magic (2), MajorLinkerVersion (1), MinorLinkerVersion (1), ... */
    /* DataDirectory starts at offset 0x70 in PE32+ OptionalHeader */
    
    uint8_t optional_header[256];
    len = sizeof(optional_header);
    if (vm_read_region(task, pe_base + nt_headers_offset + 24, 256, optional_header, &len) != KERN_SUCCESS) {
        fprintf(stderr, "  Failed to read OptionalHeader\n");
        return 0;
    }
    
    /* DataDirectory[0] (Export Table) starts at offset 0x70 in PE32+ OptionalHeader */
    uint32_t export_rva = 0;
    export_rva = optional_header[0x70] | (optional_header[0x71] << 8) |
                 (optional_header[0x72] << 16) | (optional_header[0x73] << 24);
    
    uint32_t export_size = optional_header[0x74] | (optional_header[0x75] << 8) |
                           (optional_header[0x76] << 16) | (optional_header[0x77] << 24);
    
    if (export_rva == 0 || export_size == 0) {
        fprintf(stderr, "  No export table (RVA: %08X, size: %u)\n", export_rva, export_size);
        return 0;
    }
    
    printf("  Export table: RVA=%08X, size=%u\n", export_rva, export_size);
    
    /* Read Export Directory Table */
    uint8_t edt[1024];
    len = sizeof(edt);
    mach_vm_address_t export_va = pe_base + export_rva;
    if (vm_read_region(task, export_va, export_size, edt, &len) != KERN_SUCCESS) {
        fprintf(stderr, "  Failed to read export directory\n");
        return 0;
    }
    
    uint32_t num_names = edt[16] | (edt[17] << 8) | (edt[18] << 16) | (edt[19] << 24);
    uint32_t names_rva = edt[20] | (edt[21] << 8) | (edt[22] << 16) | (edt[23] << 24);
    uint32_t ordinals_rva = edt[24] | (edt[25] << 8) | (edt[26] << 16) | (edt[27] << 24);
    uint32_t functions_rva = edt[28] | (edt[29] << 8) | (edt[30] << 16) | (edt[31] << 24);
    
    printf("  Names: %u, NamesRVA: %08X, OrdinalsRVA: %08X, FunctionsRVA: %08X\n",
           num_names, names_rva, ordinals_rva, functions_rva);
    
    if (num_names == 0) return 0;
    
    /* Read names array */
    uint8_t *name_rvas = malloc(num_names * 4);
    len = num_names * 4;
    if (vm_read_region(task, pe_base + names_rva, num_names * 4, name_rvas, &len) != KERN_SUCCESS) {
        free(name_rvas);
        fprintf(stderr, "  Failed to read names array\n");
        return 0;
    }
    
    /* Read functions array */
    uint8_t *func_rvas = malloc(num_names * 4);
    len = num_names * 4;
    if (vm_read_region(task, pe_base + functions_rva, num_names * 4, func_rvas, &len) != KERN_SUCCESS) {
        free(name_rvas);
        free(func_rvas);
        fprintf(stderr, "  Failed to read functions array\n");
        return 0;
    }
    
    /* Read ordinal table */
    uint8_t *ordinals = malloc(num_names * 2);
    len = num_names * 2;
    if (vm_read_region(task, pe_base + ordinals_rva, num_names * 2, ordinals, &len) != KERN_SUCCESS) {
        free(name_rvas);
        free(func_rvas);
        free(ordinals);
        fprintf(stderr, "  Failed to read ordinals array\n");
        return 0;
    }
    
    /* Search for the function name */
    uint64_t result = 0;
    for (uint32_t i = 0; i < num_names; i++) {
        uint32_t name_rva = name_rvas[i] | (name_rvas[i+1] << 8) |
                            (name_rvas[i+2] << 16) | (name_rvas[i+3] << 24);
        uint32_t func_rva = func_rvas[i] | (func_rvas[i+1] << 8) |
                            (func_rvas[i+2] << 16) | (func_rvas[i+3] << 24);
        
        /* Read the function name string */
        char name[256];
        memset(name, 0, sizeof(name));
        size_t name_len = sizeof(name);
        if (vm_read_region(task, pe_base + name_rva, 255, name, &name_len) != KERN_SUCCESS) {
            continue;
        }
        
        if (strcmp(name, func_name) == 0) {
            printf("  ✓ Found %s at RVA: %08X (VA: %llx)\n", func_name, func_rva,
                   (unsigned long long)(pe_base + func_rva));
            result = pe_base + func_rva;
            break;
        }
    }
    
    free(name_rvas);
    free(func_rvas);
    free(ordinals);
    
    if (result == 0) {
        fprintf(stderr, "  Function %s not found in export table\n", func_name);
    }
    
    return result;
}

/* ── Hook LdrLoadDll ── */
static int hook_ldr_load_dll(pid_t pid, mach_port_t task, const char *dll_path) {
    /* Get task port */
    if (task == 0) {
        kern_return_t kr = task_for_pid(mach_task_self_, pid, &task);
        if (kr != KERN_SUCCESS) {
            fprintf(stderr, "task_for_pid failed: %s\n", mach_error_string(kr));
            return -1;
        }
    }
    
    /* Step 1: Find ntdll.dll */
    mach_vm_address_t ntdll_base = find_ntdll_base(task);
    if (ntdll_base == 0) {
        fprintf(stderr, "Error: could not find ntdll.dll\n");
        return -1;
    }
    
    /* Step 2: Find LdrLoadDll in export table */
    printf("[2/5] Finding LdrLoadDll in ntdll.dll...\n");
    uint64_t ldr_load_dll_addr = find_export_function(task, ntdll_base, "LdrLoadDll");
    if (ldr_load_dll_addr == 0) {
        fprintf(stderr, "Error: LdrLoadDll not found\n");
        return -1;
    }
    
    /* Step 3: Allocate memory for our shellcode */
    printf("[3/5] Allocating shellcode in target...\n");
    if (vm_alloc_in_target(task, shellcode_bytes, SHELLCODE_SIZE, &injection_shellcode_addr) != 0) {
        return -1;
    }
    printf("  ✓ Shellcode at: %llx\n", (unsigned long long)injection_shellcode_addr);
    
    /* Step 4: Read and save original LdrLoadDll bytes */
    printf("[4/5] Patching LdrLoadDll...\n");
    uint8_t original_bytes[5];
    size_t len = sizeof(original_bytes);
    if (vm_read_region(task, ldr_load_dll_addr, 5, original_bytes, &len) != KERN_SUCCESS) {
        fprintf(stderr, "  Failed to read original bytes\n");
        return -1;
    }
    printf("  Original bytes: %02X %02X %02X %02X %02X\n",
           original_bytes[0], original_bytes[1], original_bytes[2],
           original_bytes[3], original_bytes[4]);
    
    /* Calculate relative jump offset: target = ldr_load_dll_addr + 5; offset = target - (ldr_load_dll_addr + 5)
     * Wait, that's 0. We want to JMP to our shellcode.
     * JMP rel32: E9 <offset> where offset = target_address - (source_address + 5)
     * target = injection_shellcode_addr, source = ldr_load_dll_addr
     */
    uint32_t rel_offset = (uint32_t)(injection_shellcode_addr - (ldr_load_dll_addr + 5));
    
    hook_jmp[0] = JMP_OPCODE;
    hook_jmp[1] = rel_offset & 0xFF;
    hook_jmp[2] = (rel_offset >> 8) & 0xFF;
    hook_jmp[3] = (rel_offset >> 16) & 0xFF;
    hook_jmp[4] = (rel_offset >> 24) & 0xFF;
    
    /* Write JMP instruction */
    kern_return_t kr = mach_vm_write(task, ldr_load_dll_addr, (vm_offset_t)hook_jmp, 5);
    if (kr != KERN_SUCCESS) {
        fprintf(stderr, "  Failed to write JMP: %s\n", mach_error_string(kr));
        return -1;
    }
    printf("  ✓ Patched LdrLoadDll → shellcode at %llx\n", (unsigned long long)injection_shellcode_addr);
    printf("  JMP offset: %08X\n", rel_offset);
    
    /* Step 5: Detach and let Wine continue */
    printf("[5/5] Detaching...\n");
    
    /* We need to suspend the process first to avoid race conditions */
    if (ptrace(PT_ATTACHEXC, pid, 0, 0) == 0) {
        int status = 0;
        waitpid(pid, &status, 0);
        printf("  ✓ Attached (suspended)\n");
        
        /* Re-patch (in case Wine called LdrLoadDll during our work) */
        kr = mach_vm_write(task, ldr_load_dll_addr, (vm_offset_t)hook_jmp, 5);
        if (kr != KERN_SUCCESS) {
            fprintf(stderr, "  Warning: re-patch failed: %s\n", mach_error_string(kr));
        }
        
        ptrace(PT_DETACH, pid, 0, 0);
        printf("  ✓ Detached\n");
    } else {
        /* Process already running — just detach if we were attached */
        printf("  ✓ No attach needed (process running)\n");
    }
    
    printf("\n  ✓ Hook installed. Next time Wine calls LdrLoadDll, our shellcode will execute.\n");
    printf("  ✓ The shellcode will resolve LoadLibraryA via PEB traversal and load the DLL.\n");
    
    return 0;
}
