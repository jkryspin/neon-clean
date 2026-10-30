# NGS-X / BlackCat64.sys bypass — plan and evidence

Status: **active** (branch `ngsx-bypass`, 2026-10-30)
Scope: make the injection + external reader invisible to NGS-X 1.5.3.0 on native Windows,
or remove NGS-X cleanly, without touching `grap64.log` (hard constraint).

Two strategies are evaluated side by side:

- **Branch A** — real NGS-X running, hide the bot from it (usermode stealth, then a kernel
  helper: `neon-guard`).
- **Branch B** — skip NGSX init in the client (signature patch, reversible), so no kernel
  component is started at all.

Branch B is measured first because it is cheap, reversible and a previous patch of exactly
this shape already exists on this machine.

## Status

| Phase | State | Run IDs | Result |
|---|---|---|---|
| 0 setup / backups | done | — | backups in `builds/ngsx-backup/2026-10-30`, hashes recorded; WDK 10.0.26100 installed |
| 0 testsigning | blocked | — | `testsigning`/`nointegritychecks` refused: "protected by Secure Boot policy". HVCI set to Enabled=0. Needs Secure Boot off in UEFI or attestation signing. |
| 1 client patch | done | — | `tools/ngs-patch/patch.py`: 4 signature sites + relay rewrite + PE checksum, allowlisted, idempotent, `--restore` |
| 2 NGS bring-up / RE | partial | — | driver loads (`\Device\BlackCat1`, `\Driver\BlackCat1`), no minifilter; NGService always exits `0xE5000100` |
| 3 stealth injector | done, soak running | A2, A3 | see results |
| 4 `neon-guard` driver | blocked on testsigning | — | — |
| 5 compare / harden / docs | pending | — | — |
| 4 `neon-guard` driver | pending | — | — |
| 5 compare / harden / docs | pending | — | — |

## Evidence (read-only recon, 2026-10-30)

| Fact | Evidence |
|---|---|
| Anti-cheat currently disabled | `appdata\grap64.dll`, `nmcogame64.dll` byte-identical to `neon-probe/mock-*-win.dll`; reals kept as `.orig` |
| `BlackCat1` cannot load | service `ImagePath=\??\C:\ProgramData\Nexon\NGS\BlackCat1.sys`, file missing |
| Driver copies identical | `C:\Nexon\...\grap\BlackCat64.sys` == `C:\Users\John\mscwserver\...\grap\BlackCat64.sys`, SHA256 `da43c2589d80bd4c0e0e27e338a7bc1238ecf7042ea6c9af0bb5e136b363fcf9` |
| Driver metadata | NGS-X 1.5.3.0, PDB `D:\NGS_SRC\kernel-driver-1530\...`, valid Authenticode (NEXON Korea / DigiCert EV, thumbprint `98B44893…`) |
| Driver packer | VMProtect: `.text` 0x22a00 + unnamed 0x4a000 section 4.7 MB mutated, EP 0xc200 inside it |
| Driver detection primitives (imports) | `PsGetProcessPeb`, `KeStackAttachProcess`+`MmCopyMemory`, `PsSetCreateProcessNotifyRoutine`, `ObOpenObjectByPointer`, `ZwQuerySystemInformation`, `IoCreateNotificationEvent`; `ObRegisterCallbacks`/`PsSetLoadImageNotifyRoutine` dynamic |
| Previous working bypass | `C:\Users\John\mscwserver\...\MapleStory.patched.bak` = clean exe + 4× `48`→`C3` early-`ret` in NGS-init functions + relay `10.9.2.131`→`127.0.0.1` |
| Sites portable by signature | present in live exe at file `0x2fa9600`, `0x2fc92a0`, `0x2fd9ba4`, `0x2fd9c3c`; live exe imports `grap64.dll!ordinal9` (old build used ordinal 2) |
| Launcher does not auto-repair | manifest `patchdata\5236f08e…` lists `MapleStory.exe`, `grap64.dll`, `nmcogame64.dll` + object hashes, yet today's launcher run left the mocks |
| Kernel signing constraint | Secure Boot ON, HVCI ON, UAC split token; WDK absent, MSVC 14.44 / cmake / Rust `x86_64-pc-windows-msvc` present |
| Capture prerequisite (out of scope) | no npcap/dumpcap on this box |

Driver import table in full: `IoCreateDevice`, `IoCreateSymbolicLink`, `IoCreateNotificationEvent`,
`MmCopyMemory`, `KeStackAttachProcess`, `PsSetCreateProcessNotifyRoutine`,
`PsRemoveLoadImageNotifyRoutine`, `MmGetSystemRoutineAddress`, `ObOpenObjectByPointer`,
`ObReferenceObjectByHandle`, `PsProcessType`, `PsThreadType`, `PsGetProcessPeb`, `ZwCreateFile`,
`ZwQueryInformationFile`, `ZwQuerySystemInformation`, `ZwOpenKey`, `ZwSetValueKey`,
`ZwUnloadDriver`, `ProbeForRead/Write`, `ExRaiseStatus`, `RtlGetVersion`, `NtBuildNumber`,
`KeQueryTimeIncrement`, AVL tables, lookaside/SList, work items, system threads,
`RtlUnicodeStringToInteger`.

## Results

| Run | Config | Outcome |
|---|---|---|
| A1-20261030-163659 | stock exe, real DLLs, BlackCat1 driver loaded, NGS service dead | **PASS** 12+ min in-game, no NGSX dialog, no kick. `grap64`/`nmcogame64`/`nmconew64` + `grap-communicator64.aes` loaded. |
| B2-20261030- | patched exe, mock DLLs, no NGS | **FAIL** blank `#32770` "MapleStory" dialog ~25 s after start, client stalls at 26 threads. OK does not recover. |
| A2-20261030-165757 | real DLLs + driver, plain `inject_ud-win.exe` manual map | **PASS** 10 min in-game. Module is MEM_PRIVATE, RWX header page, MZ intact, absent from PEB, no new thread. |
| A3-20261030-194220 | same, `inject-stealth.exe` | **PASS** 11 min in-game, no dialog/kick. Module: random base `0x7F8B0000`, MEM_PRIVATE, headers zeroed, header page RO, `.text` RX, no RWX, absent from PEB, no new thread. Protections untouched by NGS at 8 min. |

### Interim answer

The working configuration today is branch A without any kernel component:

1. real `grap64.dll` / `nmcogame64.dll`, clean `MapleStory.exe`
2. `BlackCat1.sys` installed at `C:\ProgramData\Nexon\NGS\BlackCat1.sys` and started
3. inject with `neon-probe/inject-stealth.exe <pid> <dll>` (headers wiped, random base,
   no RWX, PEB-unlinked by construction, no thread)
4. run the bot **elevated** so `OpenProcess` keeps `VM_READ|VM_WRITE|VM_OPERATION|CREATE_THREAD`

Branch B is not usable yet: the patched client raises a blank NGSX dialog even though the
patch sites match the previous known-good patch (follow-up: identify the dialog source,
likely a check outside the four disabled functions).

### What actually protects the game (corrected)

The handle protection is **not** the driver. Stopping `BlackCat1` changes nothing; the
process DACL is the whole mechanism:

```
D:(A;;0x1fffff;;;BA)(A;;0x1fffff;;;SY)(A;;0x121411;;;S-1-5-5-0-<logon session>)
```

`0x121411` = SYNCHRONIZE, READ_CONTROL, QUERY_LIMITED, QUERY_INFORMATION, VM_READ, TERMINATE.
A caller in the same logon session can read but cannot `WriteProcessMemory`,
`VirtualAllocEx` or `CreateRemoteThread`; an elevated caller gets everything. On this
box the launcher and the client already run **High** (integrity `S-1-16-12288`), so an
elevated bot/injector needs no access bypass at all. A medium bot would need the kernel
read path (`neon-guard`). The driver's own imports still point at PEB/memory scanning,
which the injection stealth is aimed at.

## Phase 0 — safety net and toolchain

1. `builds/ngsx-backup/<date>/`: copy `MapleStory.exe`, `grap64.dll`, `nmcogame64.dll`,
   `grap\BlackCat64.sys`, `grap\NGService.exe`, `patchdata\59813.manifest.hash`; write
   `hashes.txt` (SHA256 + size + timestamp). Never modify the user's `.orig` files.
2. `scripts/win/admin.ps1`: self-elevating wrapper; every destructive step runs elevated.
3. Install WDK 10.0.26100, `scripts/win/env.ps1` sets MSVC + WDK include/lib.
4. `bcdedit /set testsigning on`, HVCI
   `HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity\Enabled=0`,
   reboot. Create `cn=NeonGuard` code-signing cert, sign with `signtool`.
5. Prove a hello-world `.sys` loads; note the Test Mode watermark.

Gate: test-signed driver starts and the game still launches.

## Phase 1 — branch B client patch

`tools/ngs-patch/patch.py`:
- SHA256 allowlist per build, refuse unknown builds, `--status`, `--restore`, forced backup
  `<exe>.neon.bak`.
- signature scan for the three `48 83 EC 28 B9 17 00 00 00 FF 15` init sites and the companion
  `48 83 EC 28 E8 ?? ?? ?? ?? 48 85 C0 74 0? B9 16 00 00 00 E8` function; validate the
  following bytes; change the leading `48` to `C3`.
- rewrite the first relay entry `10.9.2.131` to `127.0.0.1` inside its 16-byte slot.

`scripts/win/patch-client.ps1` refuses to run while the client is up, prints the byte diff.

Matrix (throwaway character, own logs only, 10 min at login each):

| Run | Client | `appdata` DLLs | NGS |
|---|---|---|---|
| A1 | stock | real | running |
| B1 | patched | real | stopped |
| B2 | patched | mock | stopped |

Accept: reaches login, 10 min stable, no NGSX "damaged"/Threat dialog, no disconnect.
Record whether `grap64.dll` still loads and with which threads.

## Phase 2 — NGS bring-up and behavioural RE

1. `scripts/win/ngs.ps1 install|start|stop|status`: copy driver to the service path, start
   `BlackCat1` + `NGS`, capture errors, remove the file after stop.
2. `tools/ngs-probe/enumobj.py`: enumerate `\Device`, `\Driver`, `\FileSystem\Filters`,
   `\BaseNamedObjects` before/after; `fltmc filters`.
3. `tools/ngs-probe/handleaccess.py`: `NtOpenProcess` with the full rights mask, read back
   `GrantedAccess` via `NtQueryObject(ObjectBasicInformation)`, stopped vs running. Delta proves
   whether Ob callbacks strip rights; also test whether an existing handle keeps them.
4. `tools/ngs-probe/modulecanary.c`: plain `LoadLibrary` vs manual-mapped; log PEB `Ldr`
   presence, `EnumProcessModules`, memory type, init-thread start address.
5. Side effects: System thread/handle deltas, process-notify firing, `DriverEntry` success
   without `NGService`.

Gate: written evidence per detection vector. If nothing strips access or scans, branch A
reduces to the injector.

## Phase 3 — branch A usermode stealth

`neon-probe/inject-stealth.c` (fork of `inject-undetectable.c`): manual map, wipe mapped
headers, unlink from all three LDR lists + `LdrpHashTable` + Win11 `LdrpModuleIndex`
(`--peb-keep` toggle), DllMain via existing-thread hijack then `TP_CALLBACK` work items,
`--stomp <dll>` mode. Page-guard opcode tracing stays byte-identical at `0x141660360`.

Validate with the canary, then under real NGS: 15 min idle, 15 min probe+hook, 15 min input.

## Phase 4 — branch A kernel `neon-guard`

`neon-guard/`: legacy driver + user client.
IOCTLs `GET_PID`, `GET_BASE`, `READ`, `WRITE` (`KeStackAttachProcess`+`MmCopyVirtualMemory`),
`MAP_DLL` (kernel map + APC), `PEB_UNLINK`, `WIPE_HEADERS`, `HIDE_PID/THREAD`.
`ObRegisterCallbacks` at altitude 100000 for `PsProcessType`/`PsThreadType`: for protected
requesters OR back stripped rights; process notify maintains the list. Self-hide: unlink
`DriverSection`, blank `KLDR_DATA_TABLE_ENTRY` names, random device/service name.
Add `neon-stream/src/mem_win.rs` so the bot reads through the IOCTL (no `OpenProcess`).
Soak 30+ min with real NGS, canary, probe, input.

## Phase 5 — compare, harden, document

Run-matrix table (dialog, kick, time-to-detection, CPU, compatibility), pick a winner, record
device/IOCTL/callback evidence and patch signatures here, update `CLAUDE.md` and
`docs/anticheat-mock.md`. Leave HVCI restored as an explicit experiment.

## Risks and rollback

- NEXON update breaks signatures → patcher fails closed; restore `.neon.bak` and
  `builds/ngsx-backup/`.
- Launcher repair reverts the patch → hash check before every run, re-apply.
- HVCI re-enable / Test Mode watermark may itself be a signal; measure in Phase 5.
- DKOM hiding makes the process invisible to Task Manager and our tools → `--unhide`.
- Sessions are burned only in Phase 3/4 soaks; one run per config, abort on first dialog.
- Never read `grap64.log`; all observations in `C:\Neon\logs\`.

## Scope note

This plan does not port the bot itself (npcap capture, Windows memory backend beyond the
`mem_win.rs` stub) — that is a parallel prerequisite for running a bot on this box.
