#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mach/mach.h>
#include <mach/mach_error.h>
#include <mach/mach_vm.h>

static kern_return_t vm_read_region(mach_port_t task, mach_vm_address_t addr,
                                     mach_vm_size_t size, void *buf, size_t *out_len) {
    vm_offset_t mem_buf = 0; mach_msg_type_number_t mem_buf_len = 0;
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
    
    // Read first 512 bytes to find the real PE
    uint8_t header[512];
    size_t len = sizeof(header);
    vm_read_region(task, pe_base, 512, header, &len);
    
    printf("First 256 bytes (hex + ascii):\n");
    for (int i = 0; i < 256; i += 16) {
        printf("  %04x: ", i);
        for (int j = 0; j < 16; j++) printf("%02x ", header[i+j]);
        printf(" ");
        for (int j = 0; j < 16; j++) {
            char c = header[i+j];
            printf("%c", (c >= 32 && c < 127) ? c : '.');
        }
        printf("\n");
    }
    
    // Check if there's a PE at offset 0x100 (typical for DLLs)
    uint32_t e_lfanew = 0;
    len = sizeof(e_lfanew);
    for (uint32_t offset = 0; offset < 0x200; offset += 4) {
        if (vm_read_region(task, pe_base + offset, 4, &e_lfanew, &len) == KERN_SUCCESS) {
            uint32_t pe_sig = 0;
            len = sizeof(pe_sig);
            if (vm_read_region(task, pe_base + e_lfanew, 4, &pe_sig, &len) == KERN_SUCCESS &&
                pe_sig == 0x00004550) {
                printf("\nPE signature found at offset 0x%04X (e_lfanew=0x%08X)\n", offset, e_lfanew);
            }
        }
    }
    
    // Try reading at offset 0x100 (typical DLL preferred base)
    printf("\nTrying e_lfanew at offset 0x100:\n");
    len = sizeof(e_lfanew);
    if (vm_read_region(task, pe_base + 0x100, 4, &e_lfanew, &len) == KERN_SUCCESS) {
        printf("  e_lfanew at 0x100 = 0x%08X\n", e_lfanew);
        uint32_t pe_sig = 0;
        len = sizeof(pe_sig);
        if (vm_read_region(task, pe_base + e_lfanew, 4, &pe_sig, &len) == KERN_SUCCESS) {
            printf("  PE sig at 0x%llx = 0x%08X\n", (unsigned long long)(pe_base + e_lfanew), pe_sig);
        }
    }
    
    // Read section headers (right after COFF header)
    uint32_t e_lfanew_std = header[0x3C] | (header[0x3D] << 8) | (header[0x3E] << 16) | (header[0x3F] << 24);
    printf("\nStandard e_lfanew (at 0x3C): 0x%08X\n", e_lfanew_std);
    
    // Try to read section headers
    uint8_t section[40];
    len = sizeof(section);
    mach_vm_address_t first_section = pe_base + e_lfanew_std + 24; // After COFF + OptionalHeader
    if (vm_read_region(task, first_section, 40, section, &len) == KERN_SUCCESS) {
        char sig[5];
        memcpy(sig, section, 4); sig[4] = 0;
        printf("First section signature: %s\n", sig);
        uint16_t sec_size = section[8] | (section[9] << 8);
        uint32_t sec_rva = section[12] | (section[13] << 8) | (section[14] << 16) | (section[15] << 24);
        uint32_t sec_filesz = section[16] | (section[17] << 8) | (section[18] << 16) | (section[19] << 24);
        printf("  Name: .text?  VA: %08X  Size: %u\n", sec_rva, sec_size);
    }
    
    // Search for any DLL name strings in the PE region
    printf("\nSearching for DLL names in PE region...\n");
    vm_offset_t mem_buf = 0; mach_msg_type_number_t mem_buf_len = 0;
    if (mach_vm_read(task, pe_base, 0x30000, &mem_buf, &mem_buf_len) == KERN_SUCCESS) {
        const uint8_t *data = (const uint8_t *)mem_buf;
        size_t buf_len = (size_t)mem_buf_len;
        
        const char *dlls[] = {"ntdll.dll", "kernel32.dll", "kernelbase.dll", "user32.dll", "gdi32.dll", "win32u.dll", NULL};
        for (int d = 0; dlls[d]; d++) {
            for (size_t i = 0; i + 12 < buf_len; i++) {
                if (memcmp(data + i, dlls[d], strlen(dlls[d])) == 0) {
                    printf("  Found '%s' at offset %zu\n", dlls[d], i);
                }
            }
        }
        mach_vm_deallocate(task, mem_buf, mem_buf_len);
    }
    
    mach_port_deallocate(mach_task_self_, task);
    return 0;
}
