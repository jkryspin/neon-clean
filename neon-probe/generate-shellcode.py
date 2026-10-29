#!/usr/bin/env python3
"""Generate shellcode-embed.h from a Mach-O x86_64 .o file."""
import sys, re, os

def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <shellcode-x86_64.o>", file=sys.stderr)
        sys.exit(1)

    obj_file = sys.argv[1]
    out_file = os.path.join(os.path.dirname(obj_file), "shellcode-embed.h")

    # Assemble the object file first
    cc = os.environ.get("MACOS_CC", "clang")
    src_file = obj_file.rsplit('.', 1)[0] + ".S"  # shellcode-x86_64.S
    os.system(f'{cc} -c -arch x86_64 "{src_file}" -o "{obj_file}"')

    # Disassemble and extract bytes
    ret = os.system(f"objdump -d {obj_file} > /tmp/shellcode-objdump.txt")
    if ret != 0:
        print("objdump failed", file=sys.stderr)
        sys.exit(1)

    with open("/tmp/shellcode-objdump.txt") as f:
        lines = f.read().strip().split("\n")

    bytes_list = []
    for line in lines:
        m = re.match(r"^\s+([0-9a-f]+):\s+([0-9a-f ]+)\s+(pushq|movq|popq|cmp|test|je|jne|jl|jle|jg|jge|jmp|call|ret|xor|incl|leaq|movzwl|xchg|addq|subq|and|or|shr|shl|sar|neg|not|inc|dec|clts|lgdt|lidt|smsw|wrmsr|rdmsr)", line)
        if m:
            hex_bytes = m.group(2).strip()
            if hex_bytes:
                for p in hex_bytes.split():
                    bytes_list.append(int(p, 16))

    print(f"Extracted {len(bytes_list)} bytes from {obj_file}")

    # Write C header
    with open(out_file, "w") as f:
        f.write("/* Auto-generated from shellcode-x86_64.S — do not edit */\n")
        f.write("#ifndef SHELLCODE_EMBED_H\n")
        f.write("#define SHELLCODE_EMBED_H\n\n")
        f.write(f"static const unsigned char shellcode_bytes[] = {{\n")

        for i in range(0, len(bytes_list), 16):
            chunk = bytes_list[i:i+16]
            hex_strs = [f"0x{b:02x}" for b in chunk]
            line = "    " + ", ".join(hex_strs)
            if i + 16 < len(bytes_list):
                line += ","
            f.write(line + "\n")

        f.write("};\n\n")
        f.write(f"#define SHELLCODE_SIZE {len(bytes_list)}\n")
        f.write("#endif\n")

    print(f"Wrote {out_file}")

if __name__ == "__main__":
    main()
