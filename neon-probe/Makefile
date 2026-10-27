CC = gcc
CFLAGS = -Wall -O2 -D_WIN32_WINNT=0x0600
LDFLAGS = -lpsapi -lkernel32

# Cross-compile for Windows using mingw
CROSS_CC = x86_64-w64-mingw32-gcc
CROSS_LDFLAGS = -lpsapi -lkernel32

all: inject.exe inject_ud.exe

inject.exe: inject.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

inject_ud.exe: inject-undetectable.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

cross: inject-win.exe inject_ud-win.exe

inject-win.exe: inject.c
	$(CROSS_CC) $(CFLAGS) -o $@ $< $(CROSS_LDFLAGS)

inject_ud-win.exe: inject-undetectable.c
	$(CROSS_CC) $(CFLAGS) -o $@ $< $(CROSS_LDFLAGS)

probe.dll: probe.c
	$(CC) $(CFLAGS) -shared -o $@ $<

clean:
	rm -f *.exe *.dll

.PHONY: all cross clean
