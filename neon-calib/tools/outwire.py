import gdb, json, os, time
# Test-account only, short attach (scripts/capture-wire.sh). Logs for each outgoing packet:
#   "obj":  the client's packet object at the send function (internal opcode at +0x434, plaintext at +8),
#   "enc":  the buffer handed to the cipher dispatcher 0x140762ec0 (rdx, length r8d, mode ecx): the WIRE plaintext, before encryption,
#   "w":    the first write to obj+8 (hardware watch): where the opcode bytes change, with the registers.
# Output holds plaintext packets (may contain tokens): local only.
OUT = os.environ["OUTWIRE_JSONL"]
def log(d):
    d["t"] = time.time()
    # the file is created by the caller; root may not create or reopen it with O_CREAT in a sticky /tmp (protected_regular), so open without it
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
    def stop(self):
        regs = {r: int(gdb.parse_and_eval("$" + r)) for r in ("rip", "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11")}
        log({"k": "w", "regs": regs, "bytes": rd(self.addr, 16)})
        self.delete()
        return False
class PK(gdb.Breakpoint):
    def stop(self):
        obj = int(gdb.parse_and_eval("$rcx"))
        o = rd(obj, 0x440)
        log({"k": "obj", "addr": obj, "obj": o})
        if not getattr(PK, "watched", False):
            PK.watched = True
            w = Watch("*(unsigned short *)%d" % (obj + 8), gdb.BP_WATCHPOINT, gdb.WP_WRITE)
            w.addr = obj + 8
        return False
class ENC(gdb.Breakpoint):
    def stop(self):
        mode = int(gdb.parse_and_eval("$ecx")) & 0xffffffff
        buf = int(gdb.parse_and_eval("$rdx")); n = int(gdb.parse_and_eval("$r8d")) & 0xffffffff
        log({"k": "enc", "mode": mode, "buf": buf, "n": n, "data": rd(buf, min(n, 96))})
        return False
PK("*0x141660360"); ENC("*0x140762ec0")
