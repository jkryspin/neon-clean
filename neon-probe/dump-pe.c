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
    
    printf("=== Memory regions in 0x100000000-0x200000000 ===\n");
    while (addr < 0x200000000ULL) {
        kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                           (vm_region_info_t)&info, &count, &object);
        if (kr != KERN_SUCCESS || size == 0) {
            addr += 0x1000;
            continue;
        }
        
        if (addr >= 0x100000000ULL) {
            printf("%llx - %llx (%llu MB)  prot=%d  maxprot=%d\n",
                   (unsigned long long)addr, (unsigned long long)(addr + size),
                   (unsigned long long)(size / (1024*1024)),
                   info.protection, info.max_protection);
        }
        
        addr += size;
    }
    
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
