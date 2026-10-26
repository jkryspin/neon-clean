import gdb, json, os, struct, time
# Logs, for every outgoing packet, the raw packet object (0x440 bytes) and, for every send, its wire length.
# Local use only: the objects can contain login tokens. Analyse with neon-calib/tools/outbuild.py.
OUT = os.environ["OUTRAW_JSONL"]
class PK(gdb.Breakpoint):
    def stop(self):
        try:
            m = bytes(gdb.selected_inferior().read_memory(int(gdb.parse_and_eval("$rcx")), 0x440))
        except Exception:
            return False
        with open(OUT, "a") as f:
            f.write(json.dumps({"t": time.time(), "k": "pkt", "obj": m.hex()}) + "\n")
        return False
class SD_(gdb.Breakpoint):
    def stop(self):
        with open(OUT, "a") as f:
            f.write(json.dumps({"t": time.time(), "k": "send", "wire": int(gdb.parse_and_eval("$r8"))}) + "\n")
        return False
PK("*0x141660360"); SD_("*0x6fffff992c80")
