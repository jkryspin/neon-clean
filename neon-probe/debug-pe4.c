#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mach/mach.h>
#include <mach/mach_error.h>
#include <mach/mach_vm.h>

int main(int argc, char *argv[]) {
    if (argc != 2) { fprintf(stderr, "Usage: %s <pid>\n", argv[0]); return 1; }
    int pid = atoi(argv[1]);
    
    mach_port_t task;
    if (task_for_pid(mach_task_self_, pid, &task) != KERN_SUCCESS) {
        fprintf(stderr, "task_for_pid failed\n");
        return 1;
    }
    
    mach_vm_address_t addr = 0;
    mach_vm_size_t size = 0;
    vm_region_basic_info_data_64_t info;
    mach_msg_type_number_t count = VM_REGION_BASIC_INFO_64;
    mach_port_t object;
    
    int pe_count = 0;
    
    while (addr < 0x200000000ULL) { /* Search up to 8GB */
        kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                           (vm_region_info_t)&info, &count, &object);
        if (kr != KERN_SUCCESS || size == 0) {
            addr += 0x1000;
            continue;
        }
        
        if (info.protection & VM_PROT_READ && size > 4096) {
            uint16_t mz_sig = 0;
            vm_offset_t mem_buf = 0; mach_msg_type_number_t mem_buf_len = 0;
            
            if (mach_vm_read(task, addr, 64, &mem_buf, &mem_buf_len) == KERN_SUCCESS) {
                memcpy(&mz_sig, (const void *)mem_buf, 2);
                mach_vm_deallocate(task, mem_buf, mem_buf_len);
                
                if (mz_sig == 0x5A4D) {
                    uint32_t e_lfanew = 0;
                    vm_offset_t buf2 = 0; mach_msg_type_number_t len2 = 0;
                    if (mach_vm_read(task, addr + 0x3C, 4, &buf2, &len2) == KERN_SUCCESS) {
                        memcpy(&e_lfanew, (const void *)buf2, 4);
                        mach_vm_deallocate(task, buf2, len2);
                        
                        if (e_lfanew > 0 && e_lfanew < 0x200 && e_lfanew + 4 <= size) {
                            uint32_t pe_sig = 0;
                            vm_offset_t buf3 = 0; mach_msg_type_number_t len3 = 0;
                            if (mach_vm_read(task, addr + e_lfanew, 4, &buf3, &len3) == KERN_SUCCESS) {
                                memcpy(&pe_sig, (const void *)buf3, 4);
                                mach_vm_deallocate(task, buf3, len3);
                                
                                if (pe_sig == 0x00004550) {
                                    pe_count++;
                                    
                                    // Read OptionalHeader Magic and Export Table
                                    uint32_t opt_offset = e_lfanew + 24;
                                    uint16_t magic = 0;
                                    vm_offset_t buf4 = 0; mach_msg_type_number_t len4 = 0;
                                    if (mach_vm_read(task, addr + opt_offset, 2, &buf4, &len4) == KERN_SUCCESS) {
                                        memcpy(&magic, (const void *)buf4, 2);
                                        mach_vm_deallocate(task, buf4, len4);
                                        
                                        if (magic == 0x20b) { // PE32+
                                            uint32_t export_rva = 0, export_size = 0;
                                            vm_offset_t buf5 = 0; mach_msg_type_number_t len5 = 0;
                                            if (mach_vm_read(task, addr + opt_offset + 0x70, 8, &buf5, &len5) == KERN_SUCCESS) {
                                                memcpy(&export_rva, (const void *)buf5, 4);
                                                memcpy(&export_size, (const void *)(buf5 + 4), 4);
                                                mach_vm_deallocate(task, buf5, len5);
                                                
                                                if (export_size > 1024) { // Has a real export table
                                                    printf("PE with exports at %llx (size: %llu): ExportRVA=0x%08X ExportSize=%u\n",
                                                           (unsigned long long)addr, (unsigned long long)size,
                                                           export_rva, export_size);
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
        }
        
        addr += size;
    }
    
    printf("\nTotal PEs found: %d\n", pe_count);
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
