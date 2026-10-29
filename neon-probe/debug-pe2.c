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
    
    int total_regions = 0;
    int pe_count = 0;
    
    while (addr < 0x100000000ULL) { /* Search up to 4GB */
        kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                           (vm_region_info_t)&info, &count, &object);
        if (kr != KERN_SUCCESS || size == 0) {
            addr += 0x1000;
            continue;
        }
        
        total_regions++;
        
        if (info.protection & VM_PROT_READ && size > 1024) {
            uint16_t mz_sig = 0;
            vm_offset_t mem_buf = 0;
            mach_msg_type_number_t mem_buf_len = 0;
            
            if (mach_vm_read(task, addr, 64, &mem_buf, &mem_buf_len) == KERN_SUCCESS) {
                memcpy(&mz_sig, (const void *)mem_buf, 2);
                mach_vm_deallocate(task, mem_buf, mem_buf_len);
                
                if (mz_sig == 0x5A4D) {
                    // Check e_lfanew
                    uint32_t e_lfanew = 0;
                    if (addr + 0x3C + 4 <= addr + size) {
                        vm_offset_t buf2 = 0;
                        mach_msg_type_number_t len2 = 0;
                        if (mach_vm_read(task, addr + 0x3C, 4, &buf2, &len2) == KERN_SUCCESS) {
                            memcpy(&e_lfanew, (const void *)buf2, 4);
                            mach_vm_deallocate(task, buf2, len2);
                            
                            // Check PE signature at base + e_lfanew
                            if (e_lfanew > 0 && e_lfanew < size && e_lfanew < 0x200) {
                                uint32_t pe_sig = 0;
                                vm_offset_t buf3 = 0;
                                mach_msg_type_number_t len3 = 0;
                                if (mach_vm_read(task, addr + e_lfanew, 4, &buf3, &len3) == KERN_SUCCESS) {
                                    memcpy(&pe_sig, (const void *)buf3, 4);
                                    mach_vm_deallocate(task, buf3, len3);
                                    
                                    if (pe_sig == 0x00004550) {
                                        printf("VALID PE at %llx (size: %llu)\n", 
                                               (unsigned long long)addr, (unsigned long long)size);
                                        pe_count++;
                                        
                                        // Read section count
                                        uint16_t num_sections = 0;
                                        vm_offset_t buf4 = 0;
                                        mach_msg_type_number_t len4 = 0;
                                        if (mach_vm_read(task, addr + e_lfanew + 20, 2, &buf4, &len4) == KERN_SUCCESS) {
                                            memcpy(&num_sections, (const void *)buf4, 2);
                                            mach_vm_deallocate(task, buf4, len4);
                                            printf("  Sections: %u\n", num_sections);
                                        }
                                        
                                        // Read image name (first 16 bytes of OptionalHeader name field)
                                        // Actually, let's read the section headers to find names
                                        if (num_sections > 0 && num_sections < 96) {
                                            uint8_t section_headers[96 * 40];
                                            mach_msg_type_number_t len5 = 0;
                                            if (mach_vm_read(task, addr + e_lfanew + 24 + 256, 
                                                           num_sections * 40, section_headers, &len5) == KERN_SUCCESS) {
                                                for (uint16_t i = 0; i < num_sections && i < 10; i++) {
                                                    char name[9] = {0};
                                                    memcpy(name, section_headers + i * 40, 8);
                                                    printf("  Section[%d]: %.8s (VirtAddr: %08X, VirtSize: %08X)\n",
                                                           i, name,
                                                           section_headers[i*40+16] | (section_headers[i*40+17]<<8) |
                                                           (section_headers[i*40+18]<<16) | (section_headers[i*40+19]<<24),
                                                           section_headers[i*40+12] | (section_headers[i*40+13]<<8) |
                                                           (section_headers[i*40+14]<<16) | (section_headers[i*40+15]<<24));
                                                }
                                                mach_vm_deallocate(task, (vm_offset_t)section_headers, len5);
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
    
    printf("\nTotal regions scanned: %d, Valid PE images: %d\n", total_regions, pe_count);
    
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
