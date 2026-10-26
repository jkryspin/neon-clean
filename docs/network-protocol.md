# Network source: mobs and other players

Memory scanning never found a usable mob or player pool (everything led to render or UI data), so
`neon-stream --net` reads them from the game's own TCP connection instead. It is **passive**: it
captures packets with `sudo -n tcpdump` and never touches the game process.

```bash
nice -n 19 ./neon-stream/target/release/neon-stream --net --chains neon-calib/chains.json
./neon-stream/target/release/neon-stream --chains neon-calib/chains.json --net-replay capture.pcap   # offline check
```

Requires passwordless `sudo tcpdump`. Game connection: server ports 8500-8700 (seen: 8578/8579),
plain TCP, not TLS. Verified 2026-10-24 against the live client (Classic World v1.0.1, Windia).

## What is on the wire

Every server->client message is `[4-byte header][payload]`.

* **Length:** read the header as two little-endian u16 halves `a`, `b`; `len = a ^ b` (any length).
  A real packet always has `len >= 2` (opcode). Length 0 appears at every offset inside
  constant-filled bodies, so it must never extend a framing chain (`neon-stream` regression test).
* **Small packets:** the most-common-byte constant fails on short or number-heavy packets, so the first
  block uses the opcode rule instead (every opcode seen has a high byte 0..4: try `raw[1]-h` for h in 0..4 and
  take the candidate giving the most zero bytes). This fixed 405 of 3,835 packets in a test window.
* **Obfuscation:** inside each cipher block (first block 1456 bytes, then 1460) every byte is
  `plain + K (mod 256)` for ONE constant K per block. `K` was not derived from the handshake IV
  (several shuffle variants were tried and failed), so the decoder estimates it as the most common
  byte of the block (zeros dominate game data). This is a heuristic: it is wrong on blocks that are
  not zero-dominated (e.g. one packet full of 0xFD bytes decoded with K off by 3). Packets that
  decode to garbage are dropped by range checks.
* **Joining mid-connection:** the framer finds its own alignment (longest chain of >= 6 consistent
  headers) and re-finds it after any break. The server sends the full mob list only when a map is
  entered; mobs that were already there show up first as movement updates (template unknown).

## Opcodes (offsets into the decoded payload; little endian)

| Opcode | Meaning | Fields |
|---|---|---|
| `0x03c9` (313 B) | mob spawn | oid u32@3, **template id u32@8**, x i16@206, y i16@208 |
| `0x03dc` (83 B) | mob update/move | oid u32@2, x i16@30, y i16@32 (start of the move) |
| `0x03d4` (16 B) | mob removed | oid u32@2 |
| `0x044d` (14 B) | mob damaged | oid u32@2, attacker char id u32@6, damage u32@10 (not used yet) |
| `0x022d` (~950 B) | player spawn | char id u32@6, name length u16@18, name@20 |
| `0x029d` (41-250 B) | player move | char id u32@2, x i16@10, y i16@12 |
| `0x0471` (115 B) | **drop spawn** (sent twice, flag byte 1 then 0; dedupe by oid) | oid u32@4, owner char id u32@25, x i16@30, y i16@32, source mob oid u32@42 |
| `0x0472` (11 B) | drop removed | oid u32@2, reason u8@6 (2 picked up, 5 pet loot, 0 expired), picker char id u32@7 |
| `0x007e` | **own stat update** (classic stat bitmask) | mask u32@5; values from @9 in ascending bit order. Sizes: LEVEL `0x10` 4, STR..LUK `0x40..0x200` 2, HP/MAXHP/MP/MAXMP `0x400..0x2000` 4, AP `0x4000` 2, SP `0x8000` 2, EXP `0x10000` 8, MESO `0x40000` 8 (2026-10-25: 555 -> 566 mesos after an 11-meso pickup). JOB `0x20` is read as u16 but its size is unverified, so the walk stops after it; FAME is unknown. |
| `0x008b` type 0 | pickup message | mode u8@4: 0 item (id u32@5, quantity u32@9), 1 mesos (amount u32@6) |
| `0x0072` | inventory change | seen with every pickup; not decoded |
| `0x0281` / `0x0282` | pet spawn / move | pet name e.g. "Snail", "Black Pig" (not mobs) |
| `0x00ae` | chat / megaphone | text |
| `0x00a7` (8 B) | **party: member left**, mode u8@2 = `0x10` | char id u32@3 (our own id when we leave) |
| `0x00a7` (586 B, 1 member) | **party created**, mode `0x0e` | party id u32@3; the member list (see below) |
| `0x00a7` (1267 B, 2 members) | **party: member joined**, mode `0x13` | joiner's name (u16 length @3, name), party id u32 after it, then the member list |

Coordinates are the same space as the player position read from memory (ground of Henesys is
y=274; Henesys Hunting Ground I platforms are y = 215, -85, -325, -565).

Mob template ids are the small decimals documented at meowdb.com/msclassic (2 Snail, 3 Blue Snail,
4 Shroom, 5 Red Snail, 6 Stump, 7 Slime, 8 Pig, 9 Orange Mushroom, 10 Ribbon Pig, 13 Green Mushroom,
14 Bubbling; full list in `neon-calib/mob-ids.json`).

## Party list (0x00a7)

Captured 2026-10-26 with shurella2 (Swordsman 10, ch 35) creating a party and inviting KryJib01 (Beginner 8, ch 39).
A member record is `char id u32, name length u16, name, job u16`; the **level** is a u32 at `name_end + 8` and the
**zero-based channel** a u32 at `name_end + 12` (34 and 38). A block of about 500 bytes follows each member (avatar
data, not decoded), so the parser (`net::parse_party_members`) finds records by scanning for that shape. Maps and the
leader id sit after the last member and are not decoded. Real packets are in `tests/fixtures/party_packets.json`.
Seen live: `NetTracker.party` held `['shurelia2']` right after a create and was empty after a leave. Not captured: a
member left by someone else, a kick, a disband by the leader with others in it, a party of three or more.

## Drops are per owner

Only drops from *my own* kills ever spawned for me (2 spawn packets in a 45 s fight, while 317 removals of
other players' drops arrived with no preceding spawn). Treat the drop list as "what I can loot".
My character id is in my own `0x022d` spawn (name `shurelia2` -> `0x51a5a`).

## Not identified yet

* Player removal (players are expired after 15 s without a packet).
* **Stat-update bits other than HP/EXP:** the sizes of LEVEL/STR/DEX/INT/LUK/AP/SP/MESO are unverified, so a
  packet mixing them with HP or EXP is only decoded up to the first unknown bit. (EXP verified over three kills:
  55 -> 59 -> 70 -> 78 on the wire and on screen; the 15-byte variants carried HP 141 and 138.)
* AP and max HP/MP: expected in the stat packet at login or level-up (needs one to capture and to verify the sizes).
* Item identity of a drop (the `07 00 00 00` at offset 21 is unexplained) and whether it is mesos.
* Mob movement beyond its start position (updates carry further fragments).
* Packets sent *to* the server (the client->server direction is not decoded).

## Validation

* 12 unit tests in `neon-stream/src/net.rs` use real captured packets (`tests/fixtures/net_packets.json`).
* Replay of a 45 s capture: 7,743 packets framed with 0 resyncs, 315 mob spawns, 270 kills,
  75 player spawns.
* Live: while standing in the hunting ground the stream listed 9 mobs of the correct documented
  types at positions consistent with the screenshot, and 25 players whose names (MiMITou,
  xNinjaSinx, ...) are visible on screen.


# Outgoing packets (client -> server): status

**Header and per-packet IV are solved; the payload key is not.**

## Solved (verified)

* Framing `[4-byte header][payload]`; header `[a>>8, a, c>>8, c]` big-endian, `c = a ^ byteswap(len)`.
* **IV schedule.** The send IV is 4 bytes. Its initial value is bytes 9..12 of the plaintext server handshake
  (`01 00 01 00 00 00 01 00 30 | 2d ef c2 e1 | 09 27 cf 40 01 04` -> IV `2d ef c2 e1`; note the one-byte offset from the
  naive "recvIV then sendIV" reading). After every packet the IV is advanced by the **classic MapleStory shuffle**
  (`IV' = shuffle(IV)`, constants `f2 53 50 c6`, rotate-left 3, using the 256-byte table at `MapleStory.exe` file
  offset `0x3bb3d00`: `ec 3f 77 a4 45 d0 71 bf ...`). Implemented in `neon-calib/tools/mcrypt_check.py` (after fix) and
  re-derived in the notes below.
* **Header seed.** `a = ((iv[2] << 8) | iv[3]) ^ 0xdf00`. Checked on all 93 client packets of a capture that begins at
  the handshake: the XOR between `a` and `(iv[2]<<8|iv[3])` is the single constant `0xdf00` for every frame (pings
  included), so the IV chain is exact.
* **Live IV location.** The current send IV is the 4 bytes at process address `0x2c44c90` (natural order,
  `iv[0..3]` = `c90..c93`) in the client's heap; it is rewritten about 1 ms before each packet leaves. Found by freezing
  the process (SIGSTOP), snapshotting writable memory three times and looking for a 16-bit value whose changes track the
  header seeds (`neon-calib/tools/ivsnap.py`, `ivdiff.py`), then confirmed by sampling the bytes at 1 kHz against a
  simultaneous capture: 18/25 packets matched, the rest being packets sent from another thread. Eight bytes
  after it (`c94..c97`) change every ~100 ms and are unrelated. The address is for this client build/run; re-find it
  with the tools if it moves.
* The capture that starts at the connection handshake (`neon-calib/captures/connection_start.pcap`, used by `mcrypt_check.py`) contains
  real session traffic: it is git-ignored, keep it local.
* Measured traffic: ~4 client packets/s, so own-position updates would be 2-3 Hz at best, against ~20 Hz from memory.

## Not solved: the payload key / mode

The payload is uniformly random per packet (top byte share ~2.5%). With the exact IV for every packet, the following
all failed to produce a plausible plaintext:

* AES-128/256, OFB with the IV repeated to 16 bytes, for every 4-byte-aligned 16/32-byte window of all readable memory
  (`aesscan.c`; 12 IV layouts including reversed, zero-padded, 0xff-padded).
* A mode-flexible oracle on the 21 two-byte keep-alives of the capture (`aesscan2.c`): for the right key and layout
  `AES_K(layout(IV_n))[0:2] ^ ct_n` must be the same for most pings. 0 hits over ~108 million windows.
* Plain XOR/add of IV bytes with ciphertext (nothing constant).
* Residual IV blocks: a 16-byte block `v v v v` (the IV expanded) with `shuffle(v)` equal to the IV in memory, or any element
  of its chain, in three whole-heap snapshots: none. The cipher input is built transiently.

What the process shows about the cipher:

* The client ships its own OpenSSL 3 (`libcrypto-3-x64.dll`), and the running process has ~1,000 AES-NI instruction
  sites in unnamed executable mappings (aesenc/aesenclast/aeskeygenassist), next to the host's libcrypto/gnutls. So the
  game very likely calls OpenSSL's EVP AES, whose context (key schedule) lives in a heap object that is freed or rebuilt
  per call, which is why no key window matches at scan time.
* `MapleStory.exe` is Themida-protected (`.themida`, `.boot`, `.vm_sec` sections) and contains no AES S-box or AES-NI
  opcodes on disk, so the cipher routine cannot be located statically; only the shuffle table sits in plain `.data`.

So the remaining ways forward need a debugger or a hook on the client's OpenSSL calls (breakpoint on `EVP_CipherInit_ex`
or the AES-NI sites). That is invasive for a process with an attached security module and is **not** done here; the
header/IV decoding above is as far as passive, read-only analysis goes.

A decoded packet must show a small opcode (high byte 0-4), zero-padded fields and, in the migrate-in packet, the
character id (`5a1a0500` for `shurelia2`) before any key is trusted. Both false-positive "keys" found earlier by
character-id matching alone were noise.

## Outgoing plaintext: passive memory search (2026-10-26, negative)

Tried to find stale outgoing plaintext without a debugger: jib01 standing still at (1618, -307), process stopped,
every `rw-p` mapping searched for the position as two i16 (0 hits) and as two i32 (13 hits, all client structures
such as the player record at `0x109248`, none a packet buffer). So packet buffers are not left behind in readable
memory; the plaintext only exists transiently around the cipher call. Reading it needs a breakpoint on the client's
`libcrypto-3-x64.dll` (`EVP_*Update`). On this machine that also needs `ptrace` on a process outside our tree
(`kernel.yama.ptrace_scope=1`, the game runs under the WineCharm flatpak), i.e. root or lowering the system-wide
setting; neither was done.

## Outgoing plaintext: breakpoints on the bundled libcrypto (2026-10-26, negative)

With `sudo gdb -p <pid>` (no sysctl change; `set architecture i386:x86-64` is needed or gdb truncates addresses) on the
jib01 client, breakpoints were set at the exports of the client's own `libcrypto-3-x64.dll` (base from `/proc/PID/maps`,
RVAs from its export table): `EVP_EncryptUpdate` 0x749b, `EVP_CipherUpdate` 0x5d94, `EVP_CIPHER_CTX_new` 0x774d,
`EVP_CipherInit_ex` 0x4606, `EVP_EncryptInit_ex` 0x6c4e, `EVP_aes_128_{ofb,ctr,cbc,ecb}`. Two 30-40 s runs (about 4
client packets/s) produced **zero hits** on any of them, and the client stayed connected and unflagged while attached.
So the packet cipher does not go through that DLL; it runs in the Themida-protected executable's own AES-NI code. The
next step would be tracing that virtualised code, which is a much larger job than a breakpoint and was not attempted.


## Outgoing opcodes: read before encryption (2026-10-26, works)

The cipher is not needed. The client builds each packet in a `COutPacket`-like object and hands it to its send
function; reading that object gives the plaintext. Found with `sudo gdb -p <pid>` on a test client (classic Classic
World v1.0.1 build; **all addresses are for this build**):

* Windows `send` (`ws2_32.dll`, base from `/proc/PID/maps`, RVA `0x12c80`) is called from the game's socket wrapper at
  `0x141665b20`, inside the big writer `0x141663fa0`. That is called from **`0x141660360`**, the send-packet entry:
  `rcx` = packet object, `0x434(rcx)` = opcode (u32), and the packet bytes start at `rcx+8` as
  `[opcode u16 LE][fields...]`. It is plain x86-64 (not virtualised), so no protection was bypassed.
* How it was located: caught the host `sendmsg` on the game socket (fd to port 8578), broke on `send`, recovered real
  return addresses on the Windows stack (a qword in the code section whose preceding bytes are `call`), and walked up
  to the function that takes the packet.
* `neon-calib/tools/outcap.sh PID SECONDS out.json` records every packet's opcode and the wire length of the following
  send. Results: `docs/outgoing-opcodes.json`, rendered by `scripts/opcode-doc.py` into `docs/opcodes.md`. Payload bytes
  are not committed (some packets carry the launcher login token).
* Caveats: the packet's own length is not read yet (the wire length of the next send is only a hint); the breakpoint
  sits in the game process: **after gdb sessions of 60-150 s both test clients (jib01 and the main account shurella2) dropped to the login screen with "Unable to connect to the game server"** (cause not isolated: stalled heartbeat from stopping all threads, or the server reacting to the debugger). Treat every attach as able to cost the session; do not run it on the main account unattended, and prefer a few seconds at a time; one capture of 150 s covered 25 opcodes, mostly
  movement. The 2026-10-26 negative results above stand for the cipher itself.
