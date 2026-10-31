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
    
    const char *targets[] = {"LoadLibraryA", "LoadLibraryW", "FreeLibrary", NULL};
    
    printf("Scanning up to 8GB for common DLL loading functions...\n");
    
    for (int t = 0; targets[t] != NULL; t++) {
        const char *target = targets[t];
        size_t target_len = strlen(target) + 1;
        int found = 0;
        
        addr = 0;
        while (addr < 0x2000000000ULL) {
            kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                               (vm_region_info_t)&info, &count, &object);
            if (kr != KERN_SUCCESS || size == 0) {
                addr += 0x1000;
                continue;
            }
            
            if (info.protection & VM_PROT_READ && size > target_len) {
                vm_offset_t mem_buf = 0; mach_msg_type_number_t mem_buf_len = 0;
                if (mach_vm_read(task, addr, size, &mem_buf, &mem_buf_len) == KERN_SUCCESS) {
                    const uint8_t *data = (const uint8_t *)mem_buf;
                    size_t buf_len = (size_t)mem_buf_len;
                    
                    for (size_t i = 0; i + target_len <= buf_len; i++) {
                        if (memcmp(data + i, target, target_len - 1) == 0) {
                            uint8_t next = (i + target_len - 1 < buf_len) ? data[i + target_len - 1] : 0;
                            if (next == '\0' || next == ' ') {
                                if (found < 3) {
                                    printf("Found '%s' at addr %llx (region %llx+%llx)\n",
                                           target, (unsigned long long)(addr + i),
                                           (unsigned long long)addr, (unsigned long long)size);
                                    printf("  Context: ");
                                    size_t start = (i > 16) ? i - 16 : 0;
                                    size_t end = i + target_len + 16;
                                    if (end > buf_len) end = buf_len;
                                    for (size_t j = start; j < end; j++) {
                                        printf("%02x ", data[j]);
                                    }
                                    printf("\n");
                                }
                                found++;
                            }
                        }
                    }
                    mach_vm_deallocate(task, mem_buf, mem_buf_len);
                }
            }
            
            addr += size;
        }
        
        if (found == 0) printf("'%s': NOT FOUND\n", target);
        else printf("'%s': %d total occurrences\n\n", target, found);
    }
    
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
