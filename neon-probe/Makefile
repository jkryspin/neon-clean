CC = gcc
CFLAGS = -Wall -O2 -D_WIN32_WINNT=0x0600
LDFLAGS = -lpsapi -lkernel32

# Cross-compile for Windows using mingw
CROSS_CC = x86_64-w64-mingw32-gcc
CROSS_LDFLAGS = -lpsapi -lkernel32

all: inject.exe inject_ud.exe probe.dll probe_opcodes.dll

inject.exe: inject.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

hello-win.dll: hello-win.c
	$(CROSS_CC) $(CFLAGS) -shared -o $@ $< -lkernel32

inject_ud.exe: inject-undetectable.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

cross: inject-win.exe inject_ud-win.exe probe-win.dll probe_opcodes-win.dll

inject-win.exe: inject.c
	$(CROSS_CC) $(CFLAGS) -o $@ $< $(CROSS_LDFLAGS)

inject_ud-win.exe: inject-undetectable.c
	$(CROSS_CC) $(CFLAGS) -o $@ $< $(CROSS_LDFLAGS)

probe.dll: probe.c
	$(CC) $(CFLAGS) -shared -o $@ $<

probe_opcodes.dll: probe-opcodes.c
	$(CC) $(CFLAGS) -shared -o $@ $<

probe-win.dll: probe.c
	$(CROSS_CC) $(CFLAGS) -shared -o $@ $<

probe_opcodes-win.dll: probe-opcodes.c
	$(CROSS_CC) $(CFLAGS) -shared -o $@ $<

# ── macOS native injector (ARM64 host) ──
# NOTE: the bundled Wine ("MapleStory Launcher.app") is x86_64-on-ARM64 (native
# unix side, Rosetta PE side). Mach x86 thread state is not exposed to arm64
# callers and the x86 address space is not reachable through mach_vm, so the
# native inject-macos cannot see the PE images of a running Wine process. It
# builds, but the working path on this machine is inject.exe run UNDER wine
# (scripts/inject.sh): the Win32 APIs are serviced by wineserver, no debugger,
# no ptrace - same as on Linux.
MACOS_CC = clang
MACOS_CFLAGS = -Wall -O2

macos: inject-macos

inject-macos: inject-macos.c
	$(MACOS_CC) $(MACOS_CFLAGS) -arch arm64 -o $@ $<

# Hook-based injector: patches LdrLoadDll in Wine's ntdll.dll
inject-hook: inject-hook.c shellcode-x86_64.S
	python3 generate-shellcode.py shellcode-x86_64.o
	$(MACOS_CC) $(MACOS_CFLAGS) -arch arm64 -I. -c inject-hook.c -o inject-hook.o
	$(MACOS_CC) -arch arm64 inject-hook.o -o inject-hook

clean:
	rm -f *.exe *.dll shellcode-x86_64.o shellcode-embed.h shellcode.bin inject-macos

.PHONY: all cross clean
