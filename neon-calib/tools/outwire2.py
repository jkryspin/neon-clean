import gdb, json, os, time
# Test-account only, short attach (scripts/capture-wire.sh with this script). At the entry of the 16-bit store helper 0x14075fa20
# (rcx = buffer object, edx = position, r8w = value) log the calls that write at position 0..2 (the opcode): the value, the
# registers and the return addresses on the stack (no filter on edx: it is not a position), to find the code that maps the internal opcode to the wire opcode.
OUT = os.environ["OUTWIRE_JSONL"]
def log(d):
    d["t"] = time.time()
    fd = os.open(OUT, os.O_WRONLY | os.O_APPEND)
    try:
        os.write(fd, (json.dumps(d) + "\n").encode())
    finally:
        os.close(fd)
class ST(gdb.Breakpoint):
    n = 0
    def stop(self):
        pos = int(gdb.parse_and_eval("$edx")) & 0xffffffff
        if ST.n > 80:
            return False
        ST.n += 1
        regs = {r: int(gdb.parse_and_eval("$" + r)) & 0xffffffffffffffff for r in ("rip", "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15", "rsp")}
        try:
            stack = bytes(gdb.selected_inferior().read_memory(regs["rsp"], 8 * 24)).hex()
        except Exception:
            stack = None
        log({"k": "st", "pos": pos, "value": regs["r8"] & 0xffff, "regs": regs, "stack": stack})
        return False
ST("*0x14075fa20")
