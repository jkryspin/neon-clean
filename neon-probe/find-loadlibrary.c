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
    
    const char *targets[] = {"LoadLibraryA", "LdrLoadDll", NULL};
    
    for (int t = 0; targets[t]; t++) {
        const char *target = targets[t];
        size_t tlen = strlen(target);
        int found = 0;
        
        addr = 0;
        while (addr < 0x200000000ULL) {
            kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                               (vm_region_info_t)&info, &count, &object);
            if (kr != KERN_SUCCESS || size == 0) { addr += 0x1000; continue; }
            if (!(info.protection & VM_PROT_READ) || size < tlen) { addr += size; continue; }
            
            vm_offset_t mem_buf = 0; mach_msg_type_number_t mem_buf_len = 0;
            if (mach_vm_read(task, addr, size, &mem_buf, &mem_buf_len) == KERN_SUCCESS) {
                const uint8_t *data = (const uint8_t *)mem_buf;
                size_t buf_len = (size_t)mem_buf_len;
                
                for (size_t i = 0; i + tlen <= buf_len; i++) {
                    if (memcmp(data + i, target, tlen) == 0) {
                        uint8_t next = (i + tlen < buf_len) ? data[i + tlen] : 0;
                        if (next == '\0' || next == ' ') {
                            found++;
                            if (found <= 10) {
                                printf("[%s] #%d: addr %llx (region %llx prot=%d)\n",
                                       target, found, (unsigned long long)(addr + i),
                                       (unsigned long long)addr, info.protection);
                            }
                        }
                    }
                }
                mach_vm_deallocate(task, mem_buf, mem_buf_len);
            }
            addr += size;
        }
        
        if (found == 0) printf("[%s]: NOT FOUND\n", target);
        else printf("[%s]: %d total\n\n", target, found);
    }
    
    // Wine-specific functions
    const char *wine_funcs[] = {"wine_server_call", "wine_unix_to_win_file_name",
                                 "wine_get_unix_file_name", "NtLoadDll", NULL};
    for (int w = 0; wine_funcs[w]; w++) {
        const char *wf = wine_funcs[w];
        size_t wlen = strlen(wf);
        int found = 0;
        
        addr = 0;
        while (addr < 0x200000000ULL) {
            kern_return_t kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                                               (vm_region_info_t)&info, &count, &object);
            if (kr != KERN_SUCCESS || size == 0) { addr += 0x1000; continue; }
            if (!(info.protection & VM_PROT_READ) || size < wlen) { addr += size; continue; }
            
            vm_offset_t mem_buf = 0; mach_msg_type_number_t mem_buf_len = 0;
            if (mach_vm_read(task, addr, size, &mem_buf, &mem_buf_len) == KERN_SUCCESS) {
                const uint8_t *data = (const uint8_t *)mem_buf;
                size_t buf_len = (size_t)mem_buf_len;
                
                for (size_t i = 0; i + wlen <= buf_len; i++) {
                    if (memcmp(data + i, wf, wlen) == 0) {
                        uint8_t next = (i + wlen < buf_len) ? data[i + wlen] : 0;
                        if (next == '\0' || next == ' ') {
                            found++;
                            if (found <= 3) {
                                printf("[WINE %s] #%d: addr %llx\n", wf, found,
                                       (unsigned long long)(addr + i));
                            }
                        }
                    }
                }
                mach_vm_deallocate(task, mem_buf, mem_buf_len);
            }
            addr += size;
        }
        
        if (found > 0) printf("[WINE %s]: %d total\n", wf, found);
    }
    
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
