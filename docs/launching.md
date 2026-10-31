# Launching

## Single client

```
target\debug\neon-loader.exe        # elevated, or --headless for a step log
```

The loader restores the clean stock client and the real `grap64.dll`/`nmcogame64.dll`,
starts the Nexon launcher, triggers PLAY, stealth-injects `neon-hack`, verifies the mapping
(private region, PEB-unlinked, wiped header, read-only header, RX text) and opens neon-ce.
Log in manually.

If a step fails, read the on-screen detail: `Clean client` is a hash mismatch, `Client
started` means the launcher never spawned the game, `Injection verified` means the module
does not look stealth-mapped.

## Several clients

One loader+CE pair per account. Start the loader again for the next client; give CE a
different port with `neon-ce --pid N --port 8791` and a different `--session` in neon-web.
`neon-instances.json` still records the HWID/display bookkeeping used by the old Wine setup.

## Mocks (b2)

`apps/loader/payload/probe/mock-grap64-win.dll` and `mock-nmcogame64-win.dll` are kept for
the mock path described in `docs/anticheat-mock.md`. The default loader path is B1 (real
anti-cheat), so it restores the `.orig` DLLs when they exist.
