#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mach/mach.h>
#include <mach/mach_error.h>
#include <mach/mach_vm.h>

static kern_return_t vm_read_region(mach_port_t task, mach_vm_address_t addr,
                                     mach_vm_size_t size, void *buf, size_t *out_len) {
    vm_offset_t mem_buf = 0; mach_msg_type_number_t mem_buf_len = 0;
    kern_return_t kr = mach_vm_read(task, addr, size, &mem_buf, &mem_buf_len);
    if (kr != KERN_SUCCESS) return kr;
    size_t copy_len = (size_t)mem_buf_len < *out_len ? mem_buf_len : *out_len;
    memcpy(buf, (const void *)mem_buf, copy_len);
    *out_len = copy_len;
    mach_vm_deallocate(task, mem_buf, mem_buf_len);
    return KERN_SUCCESS;
}

int main(int argc, char *argv[]) {
    if (argc != 2) { fprintf(stderr, "Usage: %s <pid>\n", argv[0]); return 1; }
    int pid = atoi(argv[1]);
    
    mach_port_t task;
    if (task_for_pid(mach_task_self_, pid, &task) != KERN_SUCCESS) {
        fprintf(stderr, "task_for_pid failed\n");
        return 1;
    }
    
    mach_vm_address_t pe_base = 0x140000000ULL;
    uint32_t e_lfanew = 0x80;
    
    // Read OptionalHeader (523 bytes for PE32+)
    uint8_t opt_hdr[523];
    size_t len = sizeof(opt_hdr);
    vm_read_region(task, pe_base + e_lfanew + 24, 523, opt_hdr, &len);
    
    // Print all DataDirectory entries
    printf("=== Data Directory (full) ===\n");
    for (int i = 0; i < 16; i++) {
        uint32_t rva = opt_hdr[96 + i*8] | (opt_hdr[97 + i*8] << 8) |
                       (opt_hdr[98 + i*8] << 16) | (opt_hdr[99 + i*8] << 24);
        uint32_t sz = opt_hdr[100 + i*8] | (opt_hdr[101 + i*8] << 8) |
                      (opt_hdr[102 + i*8] << 16) | (opt_hdr[103 + i*8] << 24);
        const char *names[] = {"EXPORT", "IMPORT", "RESOURCE", "EXCEPTION", "SECURITY",
                               "RELOCATION", "DEBUG", "ARCHITECTURE", "GLOBALPTR",
                               "TLS", "LOAD_CONFIG", "BOUND_IMPORT", "IAT",
                               "DELAY_IMPORT", "COM_DESCRIPTOR", "RESERVED"};
        printf("  [%02d] %-14s RVA=%08X Size=%u\n", i, names[i], rva, sz);
    }
    
    // Read section headers (40 bytes each, 10 sections)
    uint8_t sections[400];
    len = sizeof(sections);
    vm_read_region(task, pe_base + e_lfanew + 24 + 523, 400, sections, &len);
    
    printf("\n=== Section Headers ===\n");
    for (int i = 0; i < 10; i++) {
        uint8_t *sec = sections + i * 40;
        char name[9];
        memcpy(name, sec, 8); name[8] = 0;
        uint32_t va = sec[12] | (sec[13] << 8) | (sec[14] << 16) | (sec[15] << 24);
        uint32_t sz = sec[16] | (sec[17] << 8) | (sec[18] << 16) | (sec[19] << 24);
        uint32_t ra = sec[20] | (sec[21] << 8) | (sec[22] << 16) | (sec[23] << 24);
        uint32_t fsz = sec[24] | (sec[25] << 8) | (sec[26] << 16) | (sec[27] << 24);
        
        printf("  [%02d] %-8s VA=%08X RVA=%08X Size=%u FileSize=%u\n",
               i, name, va, ra, sz, fsz);
    }
    
    // Read export directory
    uint32_t export_rva = opt_hdr[96] | (opt_hdr[97] << 8) | (opt_hdr[98] << 16) | (opt_hdr[99] << 24);
    uint32_t export_size = opt_hdr[100] | (opt_hdr[101] << 8) | (opt_hdr[102] << 16) | (opt_hdr[103] << 24);
    
    if (export_rva > 0 && export_size > 0 && export_size < 100000) {
        printf("\n=== Export Directory (RVA=%08X, Size=%u) ===\n", export_rva, export_size);
        uint8_t edt[10000];
        len = sizeof(edt);
        vm_read_region(task, pe_base + export_rva, export_size, edt, &len);
        
        printf("  Characteristics: %08X\n", edt[0] | (edt[1] << 8) | (edt[2] << 16) | (edt[3] << 24));
        printf("  Time/Date: %08X\n", edt[4] | (edt[5] << 8) | (edt[6] << 16) | (edt[7] << 24));
        uint32_t name_rva = edt[28] | (edt[29] << 8) | (edt[30] << 16) | (edt[31] << 24);
        printf("  Name RVA: %08X\n", name_rva);
        uint32_t num_fwd = edt[32] | (edt[33] << 8) | (edt[34] << 16) | (edt[35] << 24);
        printf("  Number of Functions: %u\n", num_fwd);
        uint32_t num_names = edt[36] | (edt[37] << 8) | (edt[38] << 16) | (edt[39] << 24);
        printf("  Number of Names: %u\n", num_names);
        
        if (num_names > 0 && num_names < 5000) {
            uint32_t names_rva = edt[40] | (edt[41] << 8) | (edt[42] << 16) | (edt[43] << 24);
            uint32_t ordinals_rva = edt[44] | (edt[45] << 8) | (edt[46] << 16) | (edt[47] << 24);
            uint32_t functions_rva = edt[48] | (edt[49] << 8) | (edt[50] << 16) | (edt[51] << 24);
            
            printf("\n  NamesRVA: %08X, OrdinalsRVA: %08X, FunctionsRVA: %08X\n",
                   names_rva, ordinals_rva, functions_rva);
            
            // Read arrays
            uint8_t *names_arr = malloc(num_names * 4);
            uint8_t *funcs_arr = malloc(num_names * 4);
            uint8_t *ord_arr = malloc(num_names * 2);
            size_t tmp;
            
            tmp = num_names * 4;
            vm_read_region(task, pe_base + names_rva, tmp, names_arr, &tmp);
            tmp = num_names * 4;
            vm_read_region(task, pe_base + functions_rva, tmp, funcs_arr, &tmp);
            tmp = num_names * 2;
            vm_read_region(task, pe_base + ordinals_rva, tmp, ord_arr, &tmp);
            
            printf("\n  === Exported Functions ===\n");
            for (uint32_t i = 0; i < num_names; i++) {
                uint32_t nrva = names_arr[i] | (names_arr[i+1] << 8) | (names_arr[i+2] << 16) | (names_arr[i+3] << 24);
                uint32_t frva = funcs_arr[i] | (funcs_arr[i+1] << 8) | (funcs_arr[i+2] << 16) | (funcs_arr[i+3] << 24);
                uint16_t ordinal = ord_arr[i] | (ord_arr[i+1] << 8);
                
                char name[256];
                memset(name, 0, sizeof(name));
                size_t nlen = sizeof(name);
                vm_read_region(task, pe_base + nrva, 255, name, &nlen);
                
                printf("    #%04d (ord %04X): %s @ RVA %08X\n", i, ordinal, name, frva);
            }
            
            free(names_arr);
            free(funcs_arr);
            free(ord_arr);
        }
    }
    
    // Read import directory
    uint32_t import_rva = opt_hdr[104] | (opt_hdr[105] << 8) | (opt_hdr[106] << 16) | (opt_hdr[107] << 24);
    uint32_t import_size = opt_hdr[108] | (opt_hdr[109] << 8) | (opt_hdr[110] << 16) | (opt_hdr[111] << 24);
    
    if (import_rva > 0 && import_size > 0 && import_size < 100000) {
        printf("\n=== Import Directory (RVA=%08X, Size=%u) ===\n", import_rva, import_size);
        uint8_t imp[10000];
        len = sizeof(imp);
        vm_read_region(task, pe_base + import_rva, import_size, imp, &len);
        
        // Print raw bytes
        printf("  Raw bytes (first 256):\n");
        for (int i = 0; i < import_size && i < 256; i += 16) {
            printf("    %04x: ", i);
            for (int j = 0; j < 16 && (i+j) < import_size; j++) printf("%02x ", imp[i+j]);
            printf(" ");
            for (int j = 0; j < 16 && (i+j) < import_size; j++) {
                char c = imp[i+j];
                printf("%c", (c >= 32 && c < 127) ? c : '.');
            }
            printf("\n");
        }
    }
    
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
