CC = gcc
CFLAGS = -Wall -O2 -D_WIN32_WINNT=0x0600
LDFLAGS = -lpsapi -lkernel32

# Cross-compile for Windows using mingw
CROSS_CC = x86_64-w64-mingw32-gcc
CROSS_LDFLAGS = -lpsapi -lkernel32

# The Nexon launcher (nexon_client.exe / nexon_runtime.exe) is 32-bit; the game
# (MapleStory.exe) is 64-bit. Injecting a 64-bit DLL into a 32-bit process fails
# (LoadLibraryW resolves to the wrong-arch kernel32), so launcher-side DLLs get a
# 32-bit build.
CROSS32_CC = i686-w64-mingw32-gcc
CROSS32_LDFLAGS = -lpsapi -lkernel32

all: inject.exe inject_ud.exe probe.dll probe_opcodes.dll

inject.exe: inject.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

hello-win.dll: hello-win.c
	$(CROSS_CC) $(CFLAGS) -shared -o $@ $< -lkernel32

inject_ud.exe: inject-undetectable.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

cross: inject-win.exe inject_ud-win.exe probe-win.dll probe_opcodes-win.dll \
	mock-grap64-win.dll mock-nmcogame64-win.dll hook-session-win.dll \
	inject32.exe hook-session32.dll

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

# ── Anti-cheat mocks + create-process hook (see docs/anticheat-mock.md) ──
mock-grap64-win.dll: mock-grap64.c mock-grap64.def
	$(CROSS_CC) $(CFLAGS) -shared -o $@ mock-grap64.c mock-grap64.def

mock-nmcogame64-win.dll: mock-nmcogame64.c mock-nmcogame64.def
	$(CROSS_CC) $(CFLAGS) -shared -o $@ mock-nmcogame64.c mock-nmcogame64.def

hook-session-win.dll: hook-session.c
	$(CROSS_CC) $(CFLAGS) -shared -o $@ $< -lpsapi -lkernel32

# ── 32-bit builds for the (32-bit) Nexon launcher processes ──
inject32.exe: inject.c
	$(CROSS32_CC) $(CFLAGS) -o $@ $< $(CROSS32_LDFLAGS)

hook-session32.dll: hook-session.c
	$(CROSS32_CC) $(CFLAGS) -shared -o $@ $< -lpsapi -lkernel32

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
