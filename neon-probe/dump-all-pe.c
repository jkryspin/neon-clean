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
    
    // Scan ALL memory for MZ signatures and dump PE info for each
    mach_vm_address_t addr = 0;
    mach_vm_size_t size = 0;
    vm_region_basic_info_data_64_t info;
    mach_msg_type_number_t count = VM_REGION_BASIC_INFO_64;
    mach_port_t object;
    
    int pe_count = 0;
    
    while (addr < 0x100000000ULL) {
        kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                           (vm_region_info_t)&info, &count, &object);
        if (kr != KERN_SUCCESS || size == 0) {
            addr += 0x1000;
            continue;
        }
        
        if (info.protection & VM_PROT_READ && size > 2) {
            uint16_t mz = 0;
            size_t len = sizeof(mz);
            if (vm_read_region(task, addr, 2, &mz, &len) == KERN_SUCCESS && mz == 0x5A4D) {
                // Found MZ! Check PE signature
                uint32_t e_lfanew = 0;
                len = sizeof(e_lfanew);
                if (vm_read_region(task, addr + 0x3C, 4, &e_lfanew, &len) == KERN_SUCCESS) {
                    uint32_t pe_sig = 0;
                    len = sizeof(pe_sig);
                    if (vm_read_region(task, addr + e_lfanew, 4, &pe_sig, &len) == KERN_SUCCESS &&
                        pe_sig == 0x00004550) {
                        
                        pe_count++;
                        printf("\n=== PE #%d at %llx ===\n", pe_count, (unsigned long long)addr);
                        printf("e_lfanew: 0x%08X\n", e_lfanew);
                        
                        // Read COFF header
                        uint8_t coff[20];
                        len = sizeof(coff);
                        if (vm_read_region(task, addr + e_lfanew + 4, 20, coff, &len) == KERN_SUCCESS) {
                            uint16_t machine = coff[0] | (coff[1] << 8);
                            uint16_t num_sections = coff[2] | (coff[3] << 8);
                            uint16_t sizeof_optional = coff[14] | (coff[15] << 8);
                            uint16_t characteristics = coff[16] | (coff[17] << 8);
                            
                            printf("Machine: 0x%04X  Sections: %d  OptHdr: 0x%04X  Char: 0x%04X\n",
                                   machine, num_sections, sizeof_optional, characteristics);
                            
                            // Read OptionalHeader
                            if (sizeof_optional >= 96 + 128) { // Need at least up to IAT
                                uint8_t opt_hdr[256];
                                len = sizeof(opt_hdr);
                                if (vm_read_region(task, addr + e_lfanew + 24, sizeof_optional < 256 ? sizeof_optional : 256, opt_hdr, &len) == KERN_SUCCESS) {
                                    uint16_t magic = opt_hdr[0] | (opt_hdr[1] << 8);
                                    printf("Magic: 0x%04X (%s)\n", magic, magic == 0x20B ? "PE32+" : (magic == 0x10B ? "PE32" : "UNKNOWN"));
                                    
                                    // Check for export table
                                    uint32_t export_rva = opt_hdr[96] | (opt_hdr[97] << 8) | (opt_hdr[98] << 16) | (opt_hdr[99] << 24);
                                    uint32_t export_size = opt_hdr[100] | (opt_hdr[101] << 8) | (opt_hdr[102] << 16) | (opt_hdr[103] << 24);
                                    
                                    // Check for import table
                                    uint32_t import_rva = opt_hdr[104] | (opt_hdr[105] << 8) | (opt_hdr[106] << 16) | (opt_hdr[107] << 24);
                                    uint32_t import_size = opt_hdr[108] | (opt_hdr[109] << 8) | (opt_hdr[110] << 16) | (opt_hdr[111] << 24);
                                    
                                    printf("Export: RVA=%08X Size=%u  Import: RVA=%08X Size=%u\n",
                                           export_rva, export_size, import_rva, import_size);
                                    
                                    // If export table exists and is valid, dump function names
                                    if (export_rva > 0 && export_size > 0 && export_size < 1000000) {
                                        uint8_t edt[4096];
                                        len = sizeof(edt);
                                        mach_vm_address_t export_va = addr + export_rva;
                                        if (vm_read_region(task, export_va, export_size, edt, &len) == KERN_SUCCESS) {
                                            uint32_t num_names = edt[16] | (edt[17] << 8) | (edt[18] << 16) | (edt[19] << 24);
                                            if (num_names > 0 && num_names < 5000) {
                                                uint32_t names_rva = edt[20] | (edt[21] << 8) | (edt[22] << 16) | (edt[23] << 24);
                                                uint32_t functions_rva = edt[28] | (edt[29] << 8) | (edt[30] << 16) | (edt[31] << 24);
                                                
                                                printf("\n  EXPORTS (%d functions):\n", num_names);
                                                
                                                uint8_t *name_rvas = malloc(num_names * 4);
                                                uint8_t *func_rvas = malloc(num_names * 4);
                                                size_t tmp;
                                                
                                                tmp = num_names * 4;
                                                vm_read_region(task, addr + names_rva, tmp, name_rvas, &tmp);
                                                tmp = num_names * 4;
                                                vm_read_region(task, addr + functions_rva, tmp, func_rvas, &tmp);
                                                
                                                for (uint32_t i = 0; i < num_names; i++) {
                                                    uint32_t nrva = name_rvas[i] | (name_rvas[i+1] << 8) | (name_rvas[i+2] << 16) | (name_rvas[i+3] << 24);
                                                    uint32_t frva = func_rvas[i] | (func_rvas[i+1] << 8) | (func_rvas[i+2] << 16) | (func_rvas[i+3] << 24);
                                                    
                                                    char name[256];
                                                    memset(name, 0, sizeof(name));
                                                    size_t nlen = sizeof(name);
                                                    vm_read_region(task, addr + nrva, 255, name, &nlen);
                                                    
                                                    printf("    %s (RVA: %08X)\n", name, frva);
                                                }
                                                
                                                free(name_rvas);
                                                free(func_rvas);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        addr += size;
    }
    
    printf("\n=== Total PEs found: %d ===\n", pe_count);
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
