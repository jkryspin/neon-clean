import gdb, json, os, time
# Test-account only (scripts/capture-wire3.sh). At the send function 0x141660360 read the packet object; when its internal opcode (+0x434)
# is one whose wire number is known (0x00db move, 0x00e1 attack, 0x02f0 keepalive) put a hardware write watch on its plaintext
# (obj + 8, 4 bytes) and log every write to it (rip, registers, stack) until the next packet object arrives.
OUT = os.environ["OUTWIRE_JSONL"]
WANT = {0xdb, 0xe1, 0x2f0}
def log(d):
    d["t"] = time.time()
    fd = os.open(OUT, os.O_WRONLY | os.O_APPEND)
    try:
        os.write(fd, (json.dumps(d) + "\n").encode())
    finally:
        os.close(fd)
def rd(addr, n):
    try:
        return bytes(gdb.selected_inferior().read_memory(addr, n)).hex()
    except Exception:
        return None
class Watch(gdb.Breakpoint):
    hits = 0
    def stop(self):
        Watch.hits += 1
        if Watch.hits > 60:
            return False
        regs = {r: int(gdb.parse_and_eval("$" + r)) & 0xffffffffffffffff for r in ("rip", "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15", "rsp")}
        log({"k": "w", "op": self.op, "regs": regs, "now": rd(self.addr, 16), "stack": rd(regs["rsp"], 8 * 16)})
        return False
class PK(gdb.Breakpoint):
    cur = None
    def stop(self):
        obj = int(gdb.parse_and_eval("$rcx"))
        o = rd(obj, 0x440)
        if o is None:
            return False
        op = int.from_bytes(bytes.fromhex(o)[0x434:0x438], "little")
        log({"k": "obj", "op": op, "plain": o[16:16 + 48]})
        if PK.cur is not None:
            try:
                PK.cur.delete()
            except Exception:
                pass
            PK.cur = None
        if op in WANT:
            w = Watch("*(unsigned int *)%d" % (obj + 8), gdb.BP_WATCHPOINT, gdb.WP_WRITE)
            w.addr = obj + 8; w.op = op
            PK.cur = w
        return False
PK("*0x141660360")
