/* dump the mach_vm_region map of a target process */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mach/mach.h>
#include <mach/mach_vm.h>

int main(int argc, char *argv[]) {
    if (argc != 2) { fprintf(stderr, "Usage: %s <pid>\n", argv[0]); return 1; }
    mach_port_t task;
    if (task_for_pid(mach_task_self_, atoi(argv[1]), &task) != KERN_SUCCESS) {
        fprintf(stderr, "task_for_pid failed\n"); return 1;
    }
    mach_vm_address_t addr = 0;
    while (1) {
        mach_vm_size_t size = 0;
        vm_region_basic_info_data_64_t info;
        mach_msg_type_number_t count = VM_REGION_BASIC_INFO_64;
        mach_port_t object;
        kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                          (vm_region_info_t)&info, &count, &object);
        if (kr != KERN_SUCCESS) break;
        /* first 2 bytes if readable */
        char tag[3] = "..";
        if (info.protection & VM_PROT_READ) {
            vm_offset_t buf = 0; mach_msg_type_number_t got = 0;
            if (mach_vm_read(task, addr, 2, &buf, &got) == KERN_SUCCESS && got == 2) {
                memcpy(tag, (void *)buf, 2);
                mach_vm_deallocate(mach_task_self(), buf, got);
            }
        }
        printf("%12llx-%12llx prot=%c%c%c%s sz=%8lluk %s\n",
               (unsigned long long)addr, (unsigned long long)(addr + size),
               (info.protection & VM_PROT_READ) ? 'r' : '-',
               (info.protection & VM_PROT_WRITE) ? 'w' : '-',
               (info.protection & VM_PROT_EXECUTE) ? 'x' : '-',
               (info.max_protection & VM_PROT_EXECUTE) ? "" : "",
               (unsigned long long)(size >> 10), tag);
        addr += size;
    }
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
