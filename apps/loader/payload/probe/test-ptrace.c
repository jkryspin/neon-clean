/* Test ptrace on this system */
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/ptrace.h>
#include <sys/types.h>
#include <errno.h>
#include <string.h>

int main(int argc, char *argv[]) {
    pid_t pid;
    int ret;

    if (argc != 2) {
        fprintf(stderr, "Usage: %s <pid>\n", argv[0]);
        return 1;
    }

    pid = atoi(argv[1]);
    printf("Testing ptrace on PID %d...\n", pid);

    /* Test PT_ATTACHEXC */
    printf("ptrace(PT_ATTACHEXC, %d) = %d (errno=%d: %s)\n",
           pid, ptrace(PT_ATTACHEXC, pid, 0, 0), errno, strerror(errno));

    /* Test PT_ATTACH */
    printf("ptrace(PT_ATTACH, %d) = %d (errno=%d: %s)\n",
           pid, ptrace(PT_ATTACH, pid, 0, 0), errno, strerror(errno));

    /* Test PTRACE_ATTACH (Linux compat) */
    printf("ptrace(PTRACE_ATTACH, %d) = %d (errno=%d: %s)\n",
           pid, ptrace(PT_ATTACH, pid, 0, 0), errno, strerror(errno));

    return 0;
}
