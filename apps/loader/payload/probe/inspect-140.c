#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mach/mach.h>
#include <mach/mach_error.h>
#include <mach/mach_vm.h>

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

int main(int argc, char *argv[]) {
    if (argc != 2) { fprintf(stderr, "Usage: %s <pid>\n", argv[0]); return 1; }
    int pid = atoi(argv[1]);
    
    mach_port_t task;
    if (task_for_pid(mach_task_self_, pid, &task) != KERN_SUCCESS) {
        fprintf(stderr, "task_for_pid failed\n");
        return 1;
    }
    
    mach_vm_address_t pe_base = 0x140000000ULL;
    
    uint16_t mz = 0; size_t len = sizeof(mz);
    if (vm_read_region(task, pe_base, 2, &mz, &len) != KERN_SUCCESS || mz != 0x5A4D) {
        printf("No MZ at 0x140000000\n");
        mach_port_deallocate(mach_task_self_, task);
        return 1;
    }
    printf("MZ signature found at 0x140000000\n");
    
    uint32_t e_lfanew = 0;
    len = sizeof(e_lfanew);
    if (vm_read_region(task, pe_base + 0x3C, 4, &e_lfanew, &len) != KERN_SUCCESS) {
        printf("Failed to read e_lfanew\n");
        mach_port_deallocate(mach_task_self_, task);
        return 1;
    }
    printf("e_lfanew = 0x%08X (PE at 0x%llx)\n", e_lfanew, (unsigned long long)(pe_base + e_lfanew));
    
    uint32_t pe_sig = 0;
    len = sizeof(pe_sig);
    if (vm_read_region(task, pe_base + e_lfanew, 4, &pe_sig, &len) != KERN_SUCCESS) {
        printf("Failed to read PE signature\n");
        mach_port_deallocate(mach_task_self_, task);
        return 1;
    }
    printf("PE signature: 0x%08X\n", pe_sig);
    
    uint8_t coff[20];
    len = sizeof(coff);
    if (vm_read_region(task, pe_base + e_lfanew + 4, 20, coff, &len) != KERN_SUCCESS) {
        printf("Failed to read COFF header\n");
        mach_port_deallocate(mach_task_self_, task);
        return 1;
    }
    uint16_t machine = coff[0] | (coff[1] << 8);
    uint16_t num_sections = coff[2] | (coff[3] << 8);
    uint32_t time_stamp = coff[4] | (coff[5] << 8) | (coff[6] << 16) | (coff[7] << 24);
    uint16_t sizeof_optional = coff[14] | (coff[15] << 8);
    uint16_t characteristics = coff[16] | (coff[17] << 8);
    
    printf("Machine: 0x%04X\n", machine);
    printf("Sections: %d\n", num_sections);
    printf("Time stamp: 0x%08X\n", time_stamp);
    printf("SizeOfOptionalHeader: 0x%04X\n", sizeof_optional);
    printf("Characteristics: 0x%04X\n", characteristics);
    
    uint8_t opt_hdr[256];
    len = sizeof(opt_hdr);
    uint16_t opt_size = sizeof_optional < 256 ? sizeof_optional : 256;
    if (vm_read_region(task, pe_base + e_lfanew + 24, opt_size, opt_hdr, &len) != KERN_SUCCESS) {
        printf("Failed to read OptionalHeader\n");
        mach_port_deallocate(mach_task_self_, task);
        return 1;
    }
    
    uint16_t magic = opt_hdr[0] | (opt_hdr[1] << 8);
    printf("\nOptionalHeader Magic: 0x%04X (%s)\n", magic, magic == 0x20B ? "PE32+" : (magic == 0x10B ? "PE32" : "UNKNOWN"));
    
    printf("\n=== Data Directory ===\n");
    for (int i = 0; i < 16; i++) {
        uint32_t rva = opt_hdr[96 + i*8] | (opt_hdr[97 + i*8] << 8) |
                       (opt_hdr[98 + i*8] << 16) | (opt_hdr[99 + i*8] << 24);
        uint32_t sz = opt_hdr[100 + i*8] | (opt_hdr[101 + i*8] << 8) |
                      (opt_hdr[102 + i*8] << 16) | (opt_hdr[103 + i*8] << 24);
        const char *names[] = {"EXPORT", "IMPORT", "RESOURCE", "EXCEPTION", "SECURITY",
                               "RELOCATION", "DEBUG", "ARCHITECTURE", "GLOBALPTR",
                               "TLS", "LOAD_CONFIG", "BOUND_IMPORT", "IAT",
                               "DELAY_IMPORT", "COM_DESCRIPTOR", "RESERVED"};
        if (rva > 0) {
            printf("  [%02d] %-14s RVA=%08X Size=%u\n", i, names[i], rva, sz);
        }
    }
    
    // Export table
    uint32_t export_rva = opt_hdr[96] | (opt_hdr[97] << 8) | (opt_hdr[98] << 16) | (opt_hdr[99] << 24);
    uint32_t export_size = opt_hdr[100] | (opt_hdr[101] << 8) | (opt_hdr[102] << 16) | (opt_hdr[103] << 24);
    
    if (export_rva > 0 && export_size > 0) {
        printf("\n=== Export Table ===\n");
        printf("Export RVA: %08X, Size: %u\n", export_rva, export_size);
        
        uint8_t edt[1024];
        len = sizeof(edt);
        mach_vm_address_t export_va = pe_base + export_rva;
        if (vm_read_region(task, export_va, export_size, edt, &len) == KERN_SUCCESS) {
            uint32_t num_names = edt[16] | (edt[17] << 8) | (edt[18] << 16) | (edt[19] << 24);
            uint32_t names_rva = edt[20] | (edt[21] << 8) | (edt[22] << 16) | (edt[23] << 24);
            uint32_t ordinals_rva = edt[24] | (edt[25] << 8) | (edt[26] << 16) | (edt[27] << 24);
            uint32_t functions_rva = edt[28] | (edt[29] << 8) | (edt[30] << 16) | (edt[31] << 24);
            
            printf("Names: %u\n", num_names);
            printf("NamesRVA: %08X, OrdinalsRVA: %08X, FunctionsRVA: %08X\n",
                   names_rva, ordinals_rva, functions_rva);
            
            if (num_names > 0 && num_names < 5000) {
                uint8_t *name_rvas = malloc(num_names * 4);
                uint8_t *func_rvas = malloc(num_names * 4);
                uint8_t *ordinals = malloc(num_names * 2);
                
                size_t tmp = num_names * 4;
                vm_read_region(task, pe_base + names_rva, tmp, name_rvas, &tmp);
                tmp = num_names * 4;
                vm_read_region(task, pe_base + functions_rva, tmp, func_rvas, &tmp);
                tmp = num_names * 2;
                vm_read_region(task, pe_base + ordinals_rva, tmp, ordinals, &tmp);
                
                printf("\n=== Exported Functions ===\n");
                for (uint32_t i = 0; i < num_names; i++) {
                    uint32_t nrva = name_rvas[i] | (name_rvas[i+1] << 8) | (name_rvas[i+2] << 16) | (name_rvas[i+3] << 24);
                    uint32_t frva = func_rvas[i] | (func_rvas[i+1] << 8) | (func_rvas[i+2] << 16) | (func_rvas[i+3] << 24);
                    
                    char name[256];
                    memset(name, 0, sizeof(name));
                    size_t nlen = sizeof(name);
                    vm_read_region(task, pe_base + nrva, 255, name, &nlen);
                    
                    printf("  #%d: %s (RVA: %08X)\n", i, name, frva);
                }
                
                free(name_rvas);
                free(func_rvas);
                free(ordinals);
            }
        }
    }
    
    // Import table
    uint32_t import_rva = opt_hdr[104] | (opt_hdr[105] << 8) | (opt_hdr[106] << 16) | (opt_hdr[107] << 24);
    uint32_t import_size = opt_hdr[108] | (opt_hdr[109] << 8) | (opt_hdr[110] << 16) | (opt_hdr[111] << 24);
    
    if (import_rva > 0 && import_size > 0) {
        printf("\n=== Import Table ===\n");
        printf("Import RVA: %08X, Size: %u\n", import_rva, import_size);
        
        uint8_t *import_data = malloc(import_size);
        len = import_size;
        vm_read_region(task, pe_base + import_rva, import_size, import_data, &len);
        
        // Find DLL names (null-terminated strings after import descriptors)
        printf("Import directory bytes 0-128:\n");
        for (int i = 0; i < 128 && i < import_size; i += 16) {
            printf("  %04x: ", i);
            for (int j = 0; j < 16 && (i+j) < import_size; j++) {
                printf("%02x ", import_data[i+j]);
            }
            printf(" ");
            for (int j = 0; j < 16 && (i+j) < import_size; j++) {
                char c = import_data[i+j];
                printf("%c", (c >= 32 && c < 127) ? c : '.');
            }
            printf("\n");
        }
        
        free(import_data);
    }
    
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
