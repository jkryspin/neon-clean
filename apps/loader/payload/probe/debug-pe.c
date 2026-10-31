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
    
    int mz_count = 0;
    mach_vm_address_t max_addr = 0xC0000000ULL;
    
    while (addr < max_addr && mz_count < 10) {
        kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                           (vm_region_info_t)&info, &count, &object);
        if (kr != KERN_SUCCESS || size == 0) {
            addr += 0x1000;
            continue;
        }
        
        if (info.protection & VM_PROT_READ && size > 64) {
            uint16_t mz_sig = 0;
            vm_offset_t mem_buf = 0;
            mach_msg_type_number_t mem_buf_len = 0;
            
            if (mach_vm_read(task, addr, 64, &mem_buf, &mem_buf_len) == KERN_SUCCESS) {
                memcpy(&mz_sig, (const void *)mem_buf, 2);
                mach_vm_deallocate(task, mem_buf, mem_buf_len);
                
                if (mz_sig == 0x5A4D) {
                    printf("MZ at %llx (size: %llu, prot: %d)\n", (unsigned long long)addr,
                           (unsigned long long)size, info.protection);
                    
                    // Check e_lfanew at offset 0x3C
                    uint32_t e_lfanew = 0;
                    if (addr + 0x3C + 4 <= addr + size) {
                        vm_offset_t buf2 = 0;
                        mach_msg_type_number_t len2 = 0;
                        if (mach_vm_read(task, addr + 0x3C, 4, &buf2, &len2) == KERN_SUCCESS) {
                            memcpy(&e_lfanew, (const void *)buf2, 4);
                            mach_vm_deallocate(task, buf2, len2);
                            printf("  e_lfanew=%u (0x%X)\n", e_lfanew, e_lfanew);
                            
                            // Check PE signature
                            if (e_lfanew > 0 && e_lfanew + 4 <= size) {
                                uint32_t pe_sig = 0;
                                vm_offset_t buf3 = 0;
                                mach_msg_type_number_t len3 = 0;
                                if (mach_vm_read(task, addr + 0x3C + e_lfanew, 4, &buf3, &len3) == KERN_SUCCESS) {
                                    memcpy(&pe_sig, (const void *)buf3, 4);
                                    mach_vm_deallocate(task, buf3, len3);
                                    printf("  PE sig at 0x%X+0x%X: %08X (%s)\n", 
                                           0x3C, e_lfanew, pe_sig,
                                           pe_sig == 0x00004550 ? "VALID PE" : "NOT PE");
                                }
                            }
                        }
                    }
                    mz_count++;
                }
            }
        }
        
        addr += size;
    }
    
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
