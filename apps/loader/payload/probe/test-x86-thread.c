/* Minimal test: can we set ANY x86_64 thread state on Wine threads? */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mach/mach.h>
#include <mach/mach_vm.h>
#include <mach/mach_error.h>
#include <stdint.h>
#include <unistd.h>
#include <libproc.h>

extern kern_return_t thread_get_state(mach_port_t thread_act,
                                       int flavor,
                                       thread_state_t thread_state_out,
                                       mach_msg_type_number_t *thread_state_count);
extern kern_return_t thread_set_state(mach_port_t thread_act,
                                       int flavor,
                                       thread_state_t thread_state_in,
                                       mach_msg_type_number_t thread_state_count);
extern kern_return_t thread_resume(mach_port_t thread);
extern kern_return_t thread_suspend(mach_port_t thread);

#define TLS_MAX_STATE_COUNT 256

int main(int argc, char *argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <pid>\n", argv[0]);
        return 1;
    }

    int pid = atoi(argv[1]);
    printf("=== x86_64 thread state test on PID %d ===\n\n", pid);

    mach_port_t task;
    if (task_for_pid(mach_task_self_, pid, &task) != KERN_SUCCESS) {
        fprintf(stderr, "task_for_pid failed\n");
        return 1;
    }

    /* Get thread list */
    int tid_count = proc_pidinfo(pid, PROC_PIDLISTTHREADS, 0, NULL, 0);
    if (tid_count <= 0) {
        fprintf(stderr, "No threads found\n");
        return 1;
    }

    uint64_t *tids = malloc(tid_count * sizeof(uint64_t));
    proc_pidinfo(pid, PROC_PIDLISTTHREADS, 0, tids, tid_count * sizeof(uint64_t));

    for (int i = 0; i < tid_count; i++) {
        mach_port_t thread = (mach_port_t)tids[i];
        printf("Thread %d (port=%u):\n", i, thread);

        /* Get ARM64 state */
        natural_t arm[TLS_MAX_STATE_COUNT];
        mach_msg_type_number_t arm_count = TLS_MAX_STATE_COUNT;
        kern_return_t kr = thread_get_state(thread, ARM_THREAD_STATE64, arm, &arm_count);
        if (kr != KERN_SUCCESS) {
            printf("  ARM64 GET: %s\n", mach_error_string(kr));
            continue;
        }
        printf("  ARM64 GET: OK\n");

        /* Try x86_64 GET */
        natural_t x86[TLS_MAX_STATE_COUNT];
        mach_msg_type_number_t x86_count = TLS_MAX_STATE_COUNT;
        kr = thread_get_state(thread, 2 /* X86_THREAD_STATE64 */, x86, &x86_count);
        if (kr == KERN_SUCCESS) {
            printf("  x86_64 GET: OK!\n");
        } else {
            printf("  x86_64 GET: %s\n", mach_error_string(kr));
        }

        /* Try x86_64 SET on suspended thread */
        thread_suspend(thread);

        memset(x86, 0, sizeof(x86));
        /* Set x86_64 state via natural_t[] (32-bit slots, 64-bit values split)
         * Layout: [0-1]=rip, [2-3]=rsp, [4-5]=rflags
         * 0x100000000 = high32=1, low32=0
         * 0x7fffffffe000 = high32=0, low32=0xffffe000 */
        x86[0] = 1;               /* rip high32 */
        x86[1] = 0;               /* rip low32 */
        x86[2] = 0;               /* rsp high32 */
        x86[3] = 0xffffe000;      /* rsp low32 */
        x86[4] = 0;               /* rflags high32 */
        x86[5] = 0x200;           /* rflags low32 */

        kr = thread_set_state(thread, 2, x86, 19);
        if (kr == KERN_SUCCESS) {
            printf("  x86_64 SET: OK! Restoring ARM64...\n");
            thread_set_state(thread, ARM_THREAD_STATE64, arm, arm_count);
            thread_resume(thread);
            printf("  RESTORED. This approach might work!\n");
            free(tids);
            mach_port_deallocate(mach_task_self_, task);
            return 0;
        } else {
            printf("  x86_64 SET: %s\n", mach_error_string(kr));
        }

        /* Restore ARM64 state */
        thread_set_state(thread, ARM_THREAD_STATE64, arm, arm_count);
        thread_resume(thread);
    }

    free(tids);
    mach_port_deallocate(mach_task_self_, task);
    printf("\nAll threads rejected x86_64 state.\n");
    return 0;
}
