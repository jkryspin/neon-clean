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

* ~~Player removal~~: identified on 2026-10-26 as incoming `0x0209` (13 bytes, char id then zeros); evidence in `neon-protocol/opcodes.toml`. (`neon-stream` still expires players after 15 s without a packet; `neon-world` removes them on `0x0209`.)
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

## Incoming cipher: what the client's receive code shows (2026-10-26, partial; no decrypt found yet)

Read from process memory (`/proc/PID/mem`, no debugger attach; build-specific addresses, Classic World v1.0.1), disassembled
offline with `objdump -D -b binary -mi386:x86-64 --adjust-vma=...`:

* The client calls `ws2_32!recv` from four places (import slot `0x14334be40`): `0x1416616f5` / `0x141661751` (the handshake
  reader: reads a 2-byte length, compares it with a maximum, then the body; unencrypted `0x13` hello) and
  `0x141663580` (the main receive loop, function `0x141663490`) and `0x141677b84`.
* The main loop allocates a **1460-byte (0x5b4) buffer** per `recv`, appends what arrived to an accumulation buffer at
  `this+0x70` (`0x141673a40`) and loops. **No decryption happens there**: it is done later on the accumulated stream
  (where is not known: `0x141663c00` was read and is socket close/reset logic, not the cipher; the consumer of the accumulation buffer at `this+0x70` has not been found). So "block" in the earlier notes is the
  cipher's own 1456/1460-byte block, not a TCP segment.
* Tested and rejected on `neon-calib/captures/connection_start.pcap`: the per-block constant K is not a byte of the receive IV
  chain (`09 27 cf 40`, advanced with the verified send-side shuffle) at any chain offset for any byte position (best 3 hits of
  55 reliable blocks, chance level), whether the blocks are counted on the raw stream or from any start offset 0-29. Only block 0
  matches (K = 0x09 = iv[0]; its plaintext reads `0b 00 00 00 01 00 00 00 17 00 00 00 ...`). The stream also begins with six
  zero bytes of junk and does not frame as `[a][b]` headers from offset 0 (it is the pre-game stage), so the pcap may not
  represent the in-game stream.
* Practical impact: in 3,318 real in-game packets only 0.09% fell into the rare-opcode tail (possible decode noise), so the
  heuristic decoder is good enough for the world model; text-heavy packets (NPC dialogs) are the ones it can mis-decode,
  and `actlab` found no reproducible opcode for an NPC dialog, which fits that.
* Next step if exact incoming decryption is wanted: find the reader of the `this+0x70` accumulation buffer (it needs no
  debugger: dump code from `/proc/PID/mem` and disassemble offline), then port the algorithm.

### Receive path, one level further (2026-10-26 18:30; same method: `/proc/PID/mem` reads, `objdump`, no attach)

* The function after the accumulation append is `0x141663cd0` (called at `0x141663685`). It loops over the accumulation buffer (`this+0x70`)
  and feeds it to the **packet framer** object at `this+0xb0` through `0x14075c780(framer, buf, &out_state, flag)`.
* `0x14075c780` is a plain framing state machine, not the cipher. Framer fields: `+8` state (0 = need header, 1 = header read,
  2 = packet complete), `+0x18` bytes collected, `+0x1c` first header u16, `+0x20` body length, `+0x24` adjust. It reads a 4-byte header
  (u16 `a`, u16 `b`; with the flag set the length is `a ^ b`, matching the header rule already used), and a header length at or above
  `0xff00` means an extended header (8 bytes, 32-bit length). It copies the body with `0x14075d9e0` (a buffer-grow + `memcpy`), and when the
  body is complete (`state 2`) the finished packet buffer is moved out with `0x14075ba10` (move + length bookkeeping). Nothing in these three
  functions transforms the bytes.
* So the decryption happens after the framer returns state 2: in whatever consumes the finished packet object (the handler
  `0x1416636a0` and the dispatch behind it). That consumer is the next thing to read; the block constant K and the first 1456-byte
  block structure are produced there, not in the framer.

## SOLVED (2026-10-26 18:40): the incoming cipher is exact and cheap

Following the receive path (framer `0x14075c780` -> `0x14075ccb0` decrypt -> IV advance `0x140cec340` -> dispatch `0x1416667a0`):

* **Per packet constant:** `plain = cipher - iv[0]` for every byte of the body (the chunk loop of 1456/1460 bytes passes the same IV pointer to
  every chunk, which is why K looked like "per block"; it is per packet). Mode 2 of the decryptor (`0x140762db0`, `psubb`) is the one in
  use on this connection. Mode 1 (`0x140ce9590`) is AES-256-OFB with the 4-byte IV repeated 4 times and a static key; it is selected by a flag at
  `this+0x48` that is zero here, and was never exercised, so it is only noted.
* **IV advance:** after each packet `iv = shuffle(iv)` with the same shuffle as the send side (`outgoing::shuffle_iv`; the client calls
  `0x140cec340(&iv, 4, NULL)` with the default state `0xc65053f2`).
* **Every header checks the chain:** the header half `a` (little-endian u16 at the start of the 4-byte header) satisfies
  `(iv[3] << 8 | iv[2]) == a ^ 0xfffe`. So mid-connection, `iv[2..4]` of the first frame come from its header and `iv[0..2]` (65,536 candidates) are fixed
  by requiring the next 11 headers to follow the chain: unique with 12 frames. Verified on a live capture of a test account: 373 consecutive
  frames, 0 header mismatches over the whole stream; the old heuristic had the wrong K in 23 of them (6%), including frames whose opcode it read wrongly.
* Why the earlier test failed ("K is not a byte of the receive IV chain"): the IV advances once per *packet*, not per cipher block, and the
  connection-start capture's stream begins with pre-game bytes that do not frame like in-game traffic.
* Implementation: `neon-stream/src/recv.rs` (`find_iv`, `RecvCipher`), used by `NetDecoder`; heuristic fallback until synced and after lost bytes.
  Tests use a committed fixture of real headers (`neon-stream/tests/fixtures/recv_headers.json`: 60 header halves, their K, two tiny frames, no names).

## SOLVED (2026-10-26 18:50): the client->server cipher too, passively

* Framing: 4-byte header, two **big-endian** u16 `a`, `c`; body length = `swap16(a ^ c)`. `a = ((iv[2] << 8) | iv[3]) ^ 0xdf00`: the same header trick as incoming, so the
  send IV is learned mid-connection from 12 headers (65,536 candidates, unique). The IV advances by the same shuffle after every packet.
  Verified on a live capture: 270 consecutive frames, the whole chain reproduced.
* Body: **AES-256-OFB**. First block = the 4-byte IV repeated four times; each 1456/1460-byte chunk restarts from that same block.
  Key: the 8 words at stride 16 of the client's static table (`0x143bb4e80`, 128 bytes, the key schedule `0x140ce9d80` reads offsets
  0, 0x10, ... 0x70; each word is a single byte 0..255): `0f 1b c5 46 f3 be ff 75` as words, i.e. the 32 bytes in `neon-stream/src/send.rs` (`KEY`).
  (Reading 32 consecutive bytes of the table, as first assumed, gives the wrong key.)
* Proof: the twelve 2-byte frames of a 2-minute capture (one every 10 s, twelve different IVs) all decrypt to the same plaintext `86 87`, which chance cannot produce;
  every other frame decrypts to a structured packet (repeating counters, small ids).
* **The wire plaintext is not the client's internal packet object.** The internal object (read at `+8` with gdb; the registry's outgoing entries and
  `docs/outgoing-opcodes.json`) has opcode `0x02f0` for a keepalive; on the wire the same packet is `86 87`. Wire opcodes are a different numbering
  (the client maps internal opcodes before sending; the mapping is not known yet), and payloads can differ. So the registry's `out` entries describe the client's
  internal view; wire-level outgoing opcodes are logged separately (`*.wire`, printed as `wire op` by `neon-stream`, split off by `stamp.py`).
  Wire opcodes seen in the first capture (jib01 standing in Amherst, 2 min): `0x7cec` x213 (lengths 48/12/111: periodic, counter-like), `0x0009c` x12 (149 bytes, 10 s),
  `0x8786` x12 (2 bytes, 10 s keepalive), `0x00c5` x12 (4 bytes, `c5 00 NN 01` with NN counting 0, 1, 2, ...), others once.
* Implementation: `neon-stream/src/aes.rs` (AES-256, FIPS-197 vector test), `neon-stream/src/send.rs` (framer, IV sync, decrypt; tests with real header fixtures
  `tests/fixtures/send_headers.json`), used by `NetDecoder` for outbound segments. Nothing in this path sends anything.
* Consequence for Stage B (headless without the client): both directions are now readable, so a protocol-faithful client is a matter of writing, not of cryptography;
  that step stays gated on written Nexon approval (`docs/headless-plan.md`).

### Wire opcodes vs internal opcodes: first three pairs (2026-10-26 19:10)

A marks test on jib01 (6 repeats each, random gaps, `neon-calib/tools/marklab.py` over the incoming log and the new `.wire` log) found one wire opcode per action:

| Action | Wire opcode | Internal opcode (registry) | Evidence |
|---|---|---|---|
| ctrl (basic attack) | `0x25d8`, 130 bytes | `Attack 0x00e1` | 6/6; 92% of the bytes after the opcode equal the internal fixture, including the `0a 00 ...` text |
| alt (jump) | `0x5f28` | `MoveSelf 0x00db` | 6/6; same `01 6a 4d e8 f1` prefix, tick u32@7 and x/y fragments |
| none (every 10 s) | `0x8786`, 2 bytes | `KeepAlive 0x02f0` | 12 frames, 12 different IVs, one plaintext |

The payload is the internal packet's payload; only the opcode number differs, and the mapping looks arbitrary (no simple xor or offset). The registry records it as `wire_opcode` on the `out` entries;
`Registry::wire_to_internal` rewrites a wire packet so the same entry decodes it (test with three real packets). Still unmapped, from the first two-minute capture:
`0x7cec` (85% of all wire packets, 12-153 bytes, a counter-like u32@2 and movement-like coordinate pairs: likely the mob-movement report `0x02fe`, unconfirmed),
`0x00ba` (3 bytes), `0x00c5` (4 bytes, a counter), `0x009c` (149 bytes, every 10 s), `0x0070` (48 bytes), `0xa409`, `0xd49e`. Opening the item/equipment/stats/skills/quest windows (clicking the entries of the shortcut menu that `i` opens;
checked with a screenshot) produced no repeating wire or incoming opcode in 4 repeats each: those windows are client-only.

### Correction (2026-10-26 19:25): wire opcodes are scrambled per connection

The table above (keepalive `0x8786`, attack `0x25d8`, move `0x5f28`) is the numbering of **one connection** (jib01). On shurella2 the same keepalive is `0xf5e8`
(138 of them, answering the 138 incoming pings); `0x00c5`, `0x009c`, `0x0070` and `0x00ba` have the same number on both accounts. So a fixed wire-to-internal table is wrong;
the registry no longer has `wire_opcode`. Instead `neon_protocol::WireMap` is the translation of one connection (`{"8786": "02f0", ...}`), written next to the log as
`<log>.wire.map` by `neon-calib/tools/wiremap.py` (which derives only what the traffic itself proves: the keepalive as the 2-byte reply to the ping, the attack by its bytes) and read by Neon View
(live: every 3 s; replay: with the session). Unmapped wire packets are not shown; they are never guessed.

What the first wire captures established (all in `opcodes.toml` summaries or here):
* **Incoming `0x062e` is the server ping** (bare 2 bytes, every 10.0 s; the old heuristic decoder never showed it). The client answers with its keepalive 2-70 ms later: 139 pings, 139 keepalives.
* **The client echoes NPC movement**: the wire packet with 48 + 21n bytes whose u32@2 is an NPC id (jib01: `0x7cec`) repeats the incoming NpcMove `0x0456` payload byte for byte (91% of 2573 reports at offsets 13-44, ids and counts equal: 2573 = 2573).
  It is 85% of all client->server traffic in a town. Its internal opcode is not known, so it has no registry entry yet.
* **A 5-minute exchange**: incoming `0x001d` (22 bytes) is answered within 0.1 s by a 22-byte wire packet (jib01: `0xfc6b`) whose u32@18 equals the `0x001d`'s u32@2 (5 of 5).
* The 1 s ticks `0x003d`, `0x002c`, `0x006e`, `0x023f` I had registered from the old logs never existed (heuristic mis-decryptions; removed).

### Robustness check (2026-10-26 19:35)
`neon-stream --net-replay` now prints `recv_exact`, `recv_heuristic`, `recv_chain_breaks`, `send_decrypted`, `send_chain_breaks`. On a fresh 90 s capture of a hunting character (shurella2): 4,953 incoming
frames, 4,941 decrypted exactly (the other 12 are the frames before the IV is learned), 0 chain breaks, 0 framer resyncs, 3,580 events; on a town capture (jib01, 2 min): 373 incoming (361 exact) and 270 outgoing frames decrypted, 0 breaks.

### Pre-game stage (login) is framed differently (noted 2026-10-26)
In `neon-calib/captures/connection_start.pcap` the bytes after the 21-byte hello do not follow the in-game framing (no 4-byte header whose first half satisfies `a ^ 0xfffe == iv[3]<<8|iv[2]` at any offset
up to 3,000, for any 4-byte window of the hello as IV). They are additive with `K = 0x09 = iv[0]` of the hello's receive IV and look like `[u32 length][body]`
(`14 09 09 09 | ...` is `0b 00 00 00` + 9: length 11). The client->server side of that capture follows the in-game header rule from its first packet, and decrypts exactly (AES-256-OFB): its first
frames use opcode `0x0070` as a login-phase wrapper (a GUID-shaped string appears in the ninth frame; it is a login token, so it is not recorded anywhere in the repo).

### A second connection start, from a relaunch of jib02 (2026-10-26 19:45)
Hello of this connection: send IV `d1 26 32 e1`, receive IV `12 9a db 06` (the first, older capture: `2d ef c2 e1` / `09 27 cf 40`).
* **Client->server decrypts exactly from its very first packet** with the in-game rule (header `a ^ 0xdf00` chain, AES-256-OFB): 67 frames, 0 header mismatches. The first frames are the login-phase `0x0070` wrapper (sub-ids 2, 1, 0x0d ...), a `0x007d`, ...
* **Server->client after the hello is not in-game framing.** After six zero bytes the stream is `plain + K` with `K = recv iv[0] = 0x12` for thousands of bytes (decoded: `0b 00 00 00 | 01 00 00 00 17 00 00 00 96 00 00 00 03 00 "All" 02 00 00 00 85 02 00 00 ...`, a table of
  numbers with the text `All`: a bulk data table, not packets), and later high-entropy bytes that I could not frame. In-game framed packets (`a ^ 0xfffe`) were not found in the first 31 KB; the game packets of this account are on a connection I did not isolate.
* **Wire opcode numbers differ again.** The 2-byte keepalive of this connection is `0x51f8` (three of them in a row); the older capture's was `0x3fd2`; jib01's live one is `0x8786`, shurella2's `0xf5e8`. Four connections, four numbers. For jib01 the keepalive, move and attack numbers (`0x8786`, `0x5f28`, `0x25d8` against internal `0x02f0`, `0x00db`, `0x00e1`) differ from their internal numbers by no common xor or offset, so the numbering is a per-opcode scramble (a table or a seeded generator), not a shift. Its seed is not obviously the hello IVs (two connections is too few to tell).

### Wire opcode numbering across connections (2026-10-26 19:55)
| Connection | keepalive (internal 0x02f0) | attack (0x00e1) | move/jump (0x00db) | hello IVs (send / receive) |
|---|---|---|---|---|
| jib01 (live, mid-stream) | `0x8786` | `0x25d8` | `0x5f28` | not seen |
| jib02 (relaunched, whole connection) | `0x51f8` | `0x90b6` | `0xd34b` | `d1 26 32 e1` / `12 9a db 06` |
| shurella2 (live, mid-stream) | `0xf5e8` | not tried | not tried | not seen |
| first capture (connection start) | `0x3fd2` | not tried | not tried | `2d ef c2 e1` / `09 27 cf 40` |

On jib02 the attack key press produced one `0x90b6` per press (6/6) and the jump key one `0xd34b` (6/6); `wiremap.py` derived keepalive and attack for that connection by itself (keepalive from the ping reply, attack from the internal fixture's bytes),
so the per-connection map pipeline works on a fresh connection without hand work. The differences wire-internal are not a common xor/offset in either connection. `0x00c5` and `0x009c` are the same on all connections that were looked at.

### Third account check (2026-10-26 19:52)
A fresh 60 s capture of jib02's in-game connection (port 8577): 204 incoming frames, 192 exact (12 before the IV was learned), 0 chain breaks, 20 client frames decrypted, 0 breaks. So the same two rules hold on three different accounts and connections
(jib01, shurella2, jib02). The in-game packets of jib02 are on the same connection as its hello and bulk table; the first 12 frames of any capture started mid-stream use the old heuristic, a capture that starts before the hello can in principle decode them exactly.

### First server round trip seen on the wire: quest accept (2026-10-26 19:50, jib01, one observation)
Accepting Pio's quest "Collecting Recycled Goods" (NPC dialog -> quest link -> ACCEPT) produced, within 0.2 s of the click:
* client->server, wire opcode `0xcc5e` on this connection, 19 bytes: `5e cc | 01 | f4 03 00 00 | 12 00 00 00 | 8d 01 | 12 01 | 00 00 00 00`: an action byte (1 = start), the quest id `0x03f4 = 1012` as u32, the NPC id `0x12` as u32, then our position x = 397, y = 274 (matches the memory reader), then four zero bytes.
* server->client `0x008b` (10 bytes) `8b 00 01 f4 03 00 00 01 00 00` (quest 1012, state 1) and another `0x008b` (17 bytes, kind 0x0d) that carries the text `qid=1012`, and `0x02e0` (16 bytes) `e0 02 0c f4 03 00 00 12 00 00 00 ...` with the quest id at @3 and the NPC id at @7.
Only one accept was done (repeating needs abandoning the quest, which I did not do), so this is a single observation, but the quest id appears in all three replies and in the request, which chance does not explain. Opening and ending the NPC dialog before accepting sent nothing (5/5, above).
**Second observation (jib02, a different connection, 19:55):** the same accept produced a wire packet `0xd5b8` (19 bytes) `b8 d5 | 01 | f4 03 00 00 | 12 00 00 00 | 15 02 | 12 01 | 00 00 00 00` (our position x = 533, y = 274) and the same three replies
(`0x008b` 10 bytes, `0x008b` 17 bytes with `qid=1012`, `0x02e0` 16 bytes) within 0.18 s. The layout of the request is therefore: action byte, quest id u32, NPC id u32, x i16, y i16, 4 zero bytes. Its opcode is `0xcc5e` on jib01 and `0xd5b8` on jib02 (per-connection numbering again).

### Negative results on how the wire numbering is chosen (2026-10-26 20:05)
With three internal opcodes known on two connections (jib01: `0xdb`->`0x5f28`, `0xe1`->`0x25d8`, `0x2f0`->`0x8786`; jib02: `0xdb`->`0xd34b`, `0xe1`->`0x90b6`, `0x2f0`->`0x51f8`), no affine map `wire = A*internal + B (mod 2^16)` and no `xor`/multiply map fits either connection (exhaustive over A, B), and the differences and xors share no constant.
The five wire numbers of jib02 (`0x51f8`, `0x90b6`, `0xd34b`) do not appear as little-endian u16 in the decoded additive-K table that the server sends after the hello (they occur only at chance rate), so that table is not the opcode table.
What remains: the table is sent inside the encrypted in-game frames (which the jib02 pcap does not frame from its start), or it is derived locally from a seed. A capture that starts before the hello **and** is framed from the first server packet is what settles it; the server-side framing of the first in-game frames of a connection is the open problem.

### The start of a server stream, measured (jib02, 2026-10-26 20:15; corrects the section that stood here)
My earlier claim that the first frames are `0x0039`, `0x003a`, `0x0234` was wrong: those were frames the Rust framer found after it synchronised at stream offset **2,956**, not the start. What the stream really looks like (jib02, port 8577, 91 KB):
* bytes 0-20: the hello (send IV `d1 26 32 e1`, receive IV `12 9a db 06`); 21-26: six zero bytes;
* about 27-300: bytes that are `plain + 0x12` (`0x12` = receive iv[0]): a small table of numbers with the text `All` (not packet-framed: the first u32 decodes to 11, and no `a ^ 0xfffe` header of the hello IV chain occurs anywhere before offset 2,956);
* about 300-2,956: high-entropy bytes (about 2.6 KB), neither additive-K nor AES-OFB under either hello IV (tested at every start offset 150-260);
* from 2,956 on: ordinary in-game frames, chained for the whole rest of the connection. The first framed header gives `iv[3] = 0x92`, `iv[2] = 0xcf`, and those two bytes are **not** on the hello receive IV's shuffle chain (5,000 shuffles), nor on the send IV's; they do not occur as bytes in the stream before it.
So the receive IV in force for the in-game frames was changed by something in the unexplained 2.6 KB (or the IV is derived from it), which is where the opcode numbering most likely is too. Next offline tests: (1) treat those 2.6 KB as one or a few frames with a header and a different cipher (RC4/XOR keyed from the hello?), (2) correlate them with the client's first packets (the client side decrypts exactly: its `0x0070` login frames carry sub-ids and a token string that may be an answer to this blob), (3) look in the client's receive path for a second consumer of the pre-game bytes (the code that handles state 0 of the framer is known: `0x14075c780`; the pre-game handler is a different function).

