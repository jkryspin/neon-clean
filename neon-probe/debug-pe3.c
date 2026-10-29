#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mach/mach.h>
#include <mach/mach_error.h>
#include <mach/mach_vm.h>

void dump_pe(mach_port_t task, mach_vm_address_t base, uint32_t e_lfanew) {
    printf("PE at %llx, e_lfanew=0x%X\n", (unsigned long long)base, e_lfanew);
    
    // COFF header starts at base + e_lfanew + 4
    uint32_t coff_offset = e_lfanew + 4;
    
    // Machine
    uint16_t machine = 0;
    vm_offset_t buf = 0; mach_msg_type_number_t len = 0;
    if (mach_vm_read(task, base + coff_offset, 2, &buf, &len) == KERN_SUCCESS) {
        memcpy(&machine, (const void *)buf, 2);
        mach_vm_deallocate(task, buf, len);
        printf("  Machine: 0x%04X (%s)\n", machine, machine == 0x8664 ? "x86_64" : machine == 0x14c ? "x86" : "unknown");
    }
    
    // NumberOfSections at coff_offset + 2
    uint16_t num_sections = 0;
    buf = 0; len = 0;
    if (mach_vm_read(task, base + coff_offset + 2, 2, &buf, &len) == KERN_SUCCESS) {
        memcpy(&num_sections, (const void *)buf, 2);
        mach_vm_deallocate(task, buf, len);
        printf("  NumberOfSections: %u\n", num_sections);
    }
    
    // OptionalHeader starts at coff_offset + 20 = e_lfanew + 24
    uint32_t opt_offset = e_lfanew + 24;
    
    // Magic
    uint16_t magic = 0;
    buf = 0; len = 0;
    if (mach_vm_read(task, base + opt_offset, 2, &buf, &len) == KERN_SUCCESS) {
        memcpy(&magic, (const void *)buf, 2);
        mach_vm_deallocate(task, buf, len);
        printf("  OptionalHeader Magic: 0x%04X (%s)\n", magic,
               magic == 0x20b ? "PE32+" : magic == 0x10b ? "PE32" : "unknown");
    }
    
    // DataDirectory[0] (Export Table) at opt_offset + 0x70
    uint32_t export_rva = 0, export_size = 0;
    buf = 0; len = 0;
    if (mach_vm_read(task, base + opt_offset + 0x70, 8, &buf, &len) == KERN_SUCCESS) {
        memcpy(&export_rva, (const void *)buf, 4);
        memcpy(&export_size, (const void *)(buf + 4), 4);
        mach_vm_deallocate(task, buf, len);
        printf("  Export Table: RVA=0x%08X, Size=%u\n", export_rva, export_size);
    }
    
    // Read first 5 section headers
    if (num_sections > 0 && num_sections < 96) {
        uint32_t sec_size = num_sections * 40;
        vm_offset_t sec_buf = 0; mach_msg_type_number_t sec_len = 0;
        if (mach_vm_read(task, base + opt_offset + 256, sec_size, &sec_buf, &sec_len) == KERN_SUCCESS) {
            for (uint16_t i = 0; i < num_sections && i < 5; i++) {
                char name[9] = {0};
                memcpy(name, (const uint8_t *)sec_buf + i * 40, 8);
                uint32_t virt_addr = ((const uint8_t *)sec_buf)[i*40+16] | (((const uint8_t *)sec_buf)[i*40+17]<<8) |
                                     (((const uint8_t *)sec_buf)[i*40+18]<<16) | (((const uint8_t *)sec_buf)[i*40+19]<<24);
                uint32_t virt_size = ((const uint8_t *)sec_buf)[i*40+12] | (((const uint8_t *)sec_buf)[i*40+13]<<8) |
                                     (((const uint8_t *)sec_buf)[i*40+14]<<16) | (((const uint8_t *)sec_buf)[i*40+15]<<24);
                printf("  Section[%d]: %.8s VirtAddr=0x%08X VirtSize=0x%08X\n",
                       i, name, virt_addr, virt_size);
            }
            mach_vm_deallocate(task, sec_buf, sec_len);
        }
    }
}

int main(int argc, char *argv[]) {
    if (argc != 2) { fprintf(stderr, "Usage: %s <pid>\n", argv[0]); return 1; }
    int pid = atoi(argv[1]);
    
    mach_port_t task;
    if (task_for_pid(mach_task_self_, pid, &task) != KERN_SUCCESS) {
        fprintf(stderr, "task_for_pid failed\n");
        return 1;
    }
    
    // Check the two known PEs
    dump_pe(task, 0x220000, 96);
    printf("\n");
    dump_pe(task, 0xb70000, 96);
    
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
