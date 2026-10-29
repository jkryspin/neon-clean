#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <libproc.h>
#include <unistd.h>
#include <sys/sysctl.h>
#include <mach/mach.h>
#include <mach/mach_error.h>

int main(int argc, char *argv[]) {
    if (argc != 2) { fprintf(stderr, "Usage: %s <pid>\n", argv[0]); return 1; }
    int pid = atoi(argv[1]);
    
    // Check proc_pidinfo
    int n = proc_pidinfo(pid, PROC_PIDLISTTHREADS, 0, NULL, 0);
    printf("proc_pidinfo(PROC_PIDLISTTHREADS) returned: %d\n", n);
    
    // Check task_for_pid
    mach_port_t task;
    kern_return_t kr = task_for_pid(mach_task_self_, pid, &task);
    printf("task_for_pid: %s\n", kr == KERN_SUCCESS ? "OK" : mach_error_string(kr));
    
    // Try sysctl kinfo_proc to get thread count
    struct kinfo_proc kp;
    size_t kp_size = sizeof(kp);
    int mib[] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, pid};
    if (sysctl(mib, 4, &kp, &kp_size, NULL, 0) == 0) {
        printf("sysctl kinfo_proc: kithread=%d\n", kp.ki_thread);
    } else {
        printf("sysctl kinfo_proc failed: %s\n", strerror(errno));
    }
    
    if (kr == KERN_SUCCESS) {
        mach_port_deallocate(mach_task_self_, task);
    }
    
    return 0;
}
