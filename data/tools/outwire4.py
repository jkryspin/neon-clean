import gdb, json, os, time
# Test-account only (scripts/capture-wire4.sh). Pairs the client's packet object (internal opcode at +0x434, send function 0x141660360)
# with what reaches the AES-OFB encryptor entry points 0x140ce9470 / 0x140ce9590 (the wire plaintext, before encryption) and logs
# the return addresses of those calls, to find the code that builds the wire buffer.
OUT = os.environ["OUTWIRE_JSONL"]
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
def reg(r):
    return int(gdb.parse_and_eval("$" + r)) & 0xffffffffffffffff
class PK(gdb.Breakpoint):
    def stop(self):
        obj = reg("rcx")
        o = rd(obj, 0x440)
        if o:
            b = bytes.fromhex(o)
            log({"k": "obj", "op": int.from_bytes(b[0x434:0x438], "little"), "plain": o[16:16 + 64], "addr": obj})
        return False
class ENC(gdb.Breakpoint):
    def stop(self):
        rsp = reg("rsp")
        log({"k": "enc", "at": hex(reg("rip")), "rcx": reg("rcx"), "rdx": reg("rdx"), "r8": reg("r8") & 0xffffffff, "r9": reg("r9"),
             "in_rcx": rd(reg("rcx"), 48), "in_rdx": rd(reg("rdx"), 48), "stack": rd(rsp, 8 * 12)})
        return False
PK("*0x141660360"); ENC("*0x140ce9470"); ENC("*0x140ce9590")
