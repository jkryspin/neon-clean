import gdb, struct, json
import os
OUT=os.environ.get("OUTCAP_JSON","outcap.json")
D={}; PEND=[None]; N=[0]
def save():
    json.dump(D,open(OUT,"w"),indent=1)
class PK(gdb.Breakpoint):
    def stop(self):
        inf=gdb.selected_inferior()
        pk=int(gdb.parse_and_eval("$rcx"))
        try: m=bytes(inf.read_memory(pk,0x440))
        except Exception: return False
        op=struct.unpack_from("<I",m,0x434)[0]
        PEND[0]=(op,m[8:8+96].hex())
        return False
class SD_(gdb.Breakpoint):
    def stop(self):
        if PEND[0] is None: return False
        ln=int(gdb.parse_and_eval("$r8")); op,hx=PEND[0]; PEND[0]=None
        e=D.setdefault("0x%04x"%op,{"count":0,"wire_lens":{},"samples":[]})
        e["count"]+=1
        e["wire_lens"][str(ln)]=e["wire_lens"].get(str(ln),0)+1
        if len(e["samples"])<4: e["samples"].append({"wire_len":ln,"plain_head":hx[:2*min(ln-4,96)+0]})
        N[0]+=1
        if N[0]%10==0: save()
        return False
PK("*0x141660360"); SD_("*0x6fffff992c80")
