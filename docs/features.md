# Features, and how they compare with Violet

This page lists what the Neon system does today, how to drive it, and which of the features on
[violet.cx](https://violet.cx/en) it has, partly has, or deliberately does not have. "Verified" means exercised
against the live client and/or covered by tests; see the last column.

## Running it

```bash
cd neon-web && npm install && npm run dev        # control panel on http://127.0.0.1:3000
```

Open the panel and press **Start bot** (it launches `scripts/run-bot.sh --supervise`), or run the pipeline yourself:

```bash
scripts/run-bot.sh --supervise --session main -- --mobs 2,3,5 --home-map 10001030 --auto-revive --max-deaths 5 --stop-at-level 10
```

`run-bot.sh` starts `neon-stream` (reads the game: memory + passive packet capture) piped into `neon-loader`
(decides and presses keys). Anything after `--` goes to `neon-loader` (`neon-loader --help` lists everything).
Supervised mode restarts the pipeline if it crashes (max 5 per hour), relaunches the game through
`scripts/relaunch.sh` if it is gone, and does not restart after a deliberate stop.
`NEON_WEB_HOST=0.0.0.0` exposes the panel on your network; it is bound to localhost by default because it can start
and stop the bot.

## Feature matrix

| Violet feature | Neon | Notes |
|---|---|---|
| Auto Farm (fights, monitors HP/MP/potions) | **Yes** | Route planning over the real map graph (walk, fall, drop-through, ropes, jumps), mob priority by cost, 64 px punch range from meowdb, flee-to-rope recovery, potion thresholds. `docs/navigation.md` |
| "Follows you across maps" | **Partly** | Grind step `"follow": true` keeps grinding on the map the character is moved to by hand (timer, kill count and level goal carry over; a retreat to `safe_map` is not followed). Unit-tested only |
| Map Rusher (travel to any map) | **Yes** | Portal graph over 334 maps from the client's own data; panel has a searchable map list with each map's mobs. Verified live: Henesys -> Forest West |
| Rusher "best map for your level" | **Yes** | `GET /api/best-maps?level=N&from=MAP` and the panel's "Suggest maps" button: scores each map by EXP-per-HP of mobs weaker than you (levels, HP, EXP from meowdb in `neon-calib/mob-info.json`), penalises mobs that out-level you, and only lists maps reachable by portal, nearest first. For level 7 it ranks Forest West first, matching what worked in play, and shows The Hill East of Henesys (one hop) as a nearby alternative |
| Route safety check | **Yes** (extra) | `GET /api/route?level=&from=&to=` and a banner in the panel list the maps a rush crosses whose mobs out-level the character (for level 7, Henesys to Forest West flags Hunting Ground I and Hill West: the maps that killed it). The bot still only walks; the warning is for you to decide |
| Task Scheduler (+ share codes) | **Yes** | Steps (travel / grind until level, minutes or kills / rest / shop trip), daily time windows with weekdays and overnight spans, repeat, `NEON1:` share codes. 7 unit tests |
| Auto Lie Detector | **Solver in progress** | Detect, pause and alert is built. Phase 6 implements the solver to auto-answer anti-macro checks |
| Party Sync | **Partly** | Party list from the network (`0x00a7`, see network-protocol.md): `GameState.party` holds the members' names, verified live for create and leave; a party member nearby is not a stranger. `--party-dir DIR`: bots on one machine share the mob each is after (lowest session name wins a tie), unit-tested only. No shared loot or coordinated map moves |
| Keep stocked / shop trips, auto sell | **Yes, by UI clicks** | Plan step `shop`: travels to the shop's map, clicks the NPC, sells whole inventory tabs (equip / use / set-up / etc) and buys by shop row, then leaves. Verified live in the Henesys Department Store: 5 Etc stacks sold (110 -> 233 mesos), 3 Red Potions bought (1 -> 4, 233 -> 83 mesos). Purchases are checked by the meso counter changing. Authorised by the account owner; no packets are sent, only mouse clicks on the client's own windows |
| Item filter (keep some items) | **Partly** | `keep` leaves the top N rows of each sell list alone; `keep_items` protects item ids, set from a field in the shop step with a keep/sell preview. No name matching until `neon-calib/items.json` is generated |
| Auto Starforce / Cube / Flame / Inner Ability | **N/A on this client** | Classic World is a v83-style client; its Game Menu has no such windows (checked live). Plan step `ui` (a calibrated click sequence, `repeat` capped at 200) exists for any other window |
| IGN Spoofer | No | Modifies the client's display; out of scope (read-only principle) |
| Rotation Builder | **Partly** | Attack-skill rotation with cooldowns and MP floors, plus timed macros for buffs. No generator from spawn data |
| Multi-client sessions | **Yes** | One status/control/history file set per `--session`; `--pid` selects the game process; session switcher in the panel. Not tried with two live clients |
| Safety: strangers, friend whitelist | **Yes** | Pause when a non-friend comes close (`evasion`); friends list in Settings |
| Safety: GM reactions | **In progress** | Phase 8: pause when GM detected on the map (GM flag in player data) |
| Safety: panic key | **Yes** | Global hotkey (default `Pause`) stops the bot at once and releases every key. Verified live |
| Safety: connection loss | **Yes** | If no packet arrives from the game server for 15 s the bot stops pressing keys and alerts (`alerts.connection_stalled`). No false alarm in live traffic |
| Safety: stop rules, Discord DMs | **Yes** | `--max-deaths`, per-hour revive cap, plan end, target level, stream-lag guard (frames older than 400 ms are never acted on); Discord alerts for level up, death, low HP, stranger, lie detector, stop |
| Macros and keybinds | **Yes** | Timed key macros; a macro key of `mouse:left` / `mouse:right` clicks at the pointer. Mouse macros unit-tested only |
| Live browser control | **Yes** | Dark violet dashboard: start/pause/resume/stop, live settings, plans, level + EXP bar with EXP/hour and ETA, HP/MP, kills/hour and kills by mob (names), potions, deaths, plan step progress, live map (mob names, wanted vs ignored mobs, player names, drops, ropes, sim ghost, fit/follow, hover), charts (EXP, kills, HP), filterable activity feed, level-up log, pipeline health, alert when HP potions are pressed but HP does not rise; History page with ranges and a per-run table. The level is derived from the EXP history in the browser (`neon-web/src/track.js`) because the memory level does not update after a level-up; it can be corrected by hand. Verified end to end against the real pipeline in dry-run |
| Live game view | **Yes** (extra) | `GET /api/stream` (MJPEG, `?fps=1-8&scale=50/75/100`), `GET /api/frame.jpg`, `GET /api/stream/status`. Screen grabs of the client window with ImageMagick `import` (niced, one shared capture loop that runs only while someone watches; nothing is sent to the game). Dashboard card and a "Live game" page with play/pause, rate, quality, screenshot and fullscreen; paused by default on phones. `neon-web/server/stream.mjs` |
| Auto Login & Relaunch | **Partly** | Relaunch of the game through the saved launcher login (`scripts/relaunch.sh`) when supervised; no credential handling |
| Script marketplace | **Partly** | Named settings profiles, `NEONP1:` share codes, and a Library page that keeps named `NEON1:` / `NEONP1:` codes in the browser and applies them to the bot. No online store |
| Lua scripting API, packet hooks | **Rules instead** | A JSON rules engine ("when X, do Y"): 10 triggers (HP/MP, level, map, mobs nearby, empty spot, stranger, timer) and actions (press key, pause, stop, alert, change settings), edge or repeating, with cooldowns; editable live in the panel (Rules page) or `--rules file.json`. No code execution, so no Lua. Incoming packets are decoded in `neon-stream` (`docs/network-protocol.md`); sending packets is not implemented |
| Out packets (`out_packet(...):send()`) | **In progress** | Phase 9: find outgoing payload key, implement packet construction/encryption, add to rules engine |

Extras Violet's page does not list: exact map geometry from the client's data files (platforms, ropes, portals, spawn
lists), EXP/hour and ETA to a target level from the real EXP table, per-mob kill statistics, a per-tick decision
trace (`--trace-file`), and end-to-end latency/FPS shown in the panel.

## Rules

```json
[ {"name":"buff","when":{"kind":"every_ms","ms":60000},"then":[{"kind":"tap","key":"F5"}],"cooldown_ms":60000,"edge":false},
  {"name":"danger","when":{"kind":"hp_below","pct":35},"then":[{"kind":"notify","text":"HP low"},{"kind":"pause"}]},
  {"name":"empty spot","when":{"kind":"no_mobs_for","ms":20000,"within_px":500},"then":[{"kind":"set","settings":{"home_map":10001060}}]} ]
```

Triggers: `hp_below`, `hp_above`, `mp_below`, `level_at_least`, `in_map`, `not_in_map`, `mobs_near`, `no_mobs_for`, `stranger_near`,
`every_ms`. Actions: `tap`, `pause`, `stop`, `notify`, `set` (any partial settings). A rule fires when its condition *becomes*
true (`"edge": true`, the default) or on every cooldown while it stays true (`"edge": false`).

## Control protocol

The panel talks to the bot through two files in the repository root, so anything can drive the bot:

* `neon-status[.<session>].json`: written by the bot ~2x per second (state, stats, progress, config, events, map outline).
* `neon-control[.<session>].json`: watched by the bot, and effectively its *desired state*: a restarted bot re-reads it and picks up the last settings, plan and rules (the Start button clears `stop`/`paused`). `{"seq":N,"paused":bool,"stop":bool,"settings":{...},"plan":{...},"rules":[...]}`.
  `settings` is partial: only the named keys change (`flee_hp_below`, `mobs`, `home_map`, `macros`, `rotation`, ...; see
  `neon-loader/src/control.rs`). `seq` must change for a command to be read. An empty `plan.steps` clears the plan.

HTTP (all JSON, localhost): `GET /api/live|sessions|meta|history|control|profiles`, `POST /api/control|profiles|session/start|session/stop`,
`DELETE /api/profiles?name=`. Session names are `[A-Za-z0-9_-]{1,32}`.

### Plan format

```json
{ "name": "evenings",
  "steps": [ {"kind":"travel","map":10001030},
             {"kind":"grind","map":10001030,"until_level":10,"minutes":180},
             {"kind":"rest","minutes":5} ],
  "repeat": false,
  "windows": [ {"days":[0,1,2,3,4],"start":"18:00","end":"23:30"} ],
  "utc_offset_min": -300 }
```

Run it with `neon-loader --plan plan.json` or from the panel.

### Potions on quick slots (hotkeys)

This client binds F1-F7 to emotes, so the default `F2`/`F3` potion keys heal nothing. Give the loader the potions'
inventory cells, `--hp-key inv:use:0:3 --mp-key inv:use:0:4` (`inv:<tab>:<row>:<col>`, 0-based, tabs
`equip|use|setup|etc`): at start it puts the HP potion on the **Ins** quick slot and the MP potion on **Home** and from
then on just presses those keys. Placing works as click-to-pick-up, click-to-drop (click the item once, it sticks to the
cursor, click the slot); press-and-drag does not register in this client. The slot is checked afterwards; if placing
fails the bot keeps double-clicking the inventory cell instead. `--no-hotkeys` skips the binding; `--hp-key Insert`
uses a slot bound earlier. A key press gives no sign of an empty slot, so two presses in a row that do not raise HP mean
"out of potions" (with `--safe-map` the bot then walks to town and rests). Verified live 2026-10-25: Red Potion on Ins,
Blue Potion on Home, HP refilled by key presses during a Thicket II run.

### Shop step

```json
{"kind":"shop","map":10001042,"npc":[452,518],"sell":["etc"],"keep":0,"keep_items":[4000005],"buy":[{"slot":0,"qty":50}]}
```

`npc` is where to click the NPC in the 1422x900 reference window (Luna of the Henesys Department Store is at 452,518).
`sell` lists inventory tabs, `keep` skips that many top rows of each list (used when an item's id isn't known yet),
`keep_items` protects specific item ids wherever they sit in the list — the bot already knows every slot's item id from
the server's own inventory updates (`neon-stream`'s `0x0072`/`0x01a9` decoding), so a quest item or a kept drop survives
a sell trip regardless of its row. `neon-calib/tools/wzitems.py <String_000.wz> neon-calib/items.json` extracts item
names -> the ids go in `keep_items` (the bot itself only ever sees ids, never names). `buy[].slot` is the row of the
shop's own list (row 0 is Red Potion in Luna's shop). NPC and map ids come from `neon-calib/maps/*.json` (`npcs`) and
`neon-calib/npc-names.json`. Use `--no-evasion` for town trips; towns are full of strangers. Dry runs skip the clicks.

Unit-tested (`neon-loader/src/shop.rs`): keeping by position reproduces the old fixed-row behaviour exactly, and
keeping by id protects an item wherever it moves in the list as others are sold. Not yet verified live.

### Quest step

```json
{"kind":"quest","quest":10001}                      // accept Phil's Call
{"kind":"quest","quest":10001,"complete":true}      // hand it in
```

Only the quest id is needed. `neon-calib/tools/wzquests.py <QuestData_000.wz> neon-calib/quests.json` extracts every
quest from the client (343): start/end NPC, level range, prerequisite quests, kills and items to collect, and the
rewards (mesos, EXP, items). The loader finds the NPC in `neon-calib/maps/*.json` (re-run `wzmap.py` to refresh;
it records `npcs`), travels to that map, walks to the NPC with the platform graph (`walk_to` in the brain), then clicks
it where the camera puts it (`quest::click_point`: the view follows the player, clamped to the map's view rectangle)
and answers the dialog. The panel's "Quest (talk to the NPC)" step lists quests that pay mesos
(`GET /api/quests?level=N`), talk-only ones first.

The conversation itself is driven by pixels (`quest::drive_dialog`, also `neon-loader --quest-dialog` for the dialog open
right now): the green END CHAT button anchors the dialog, then it clicks the first red-arrow quest entry of a list, the
rightmost blue NEXT, the orange ACCEPT or OK, and never DECLINE or END CHAT. A YES/NO prompt (narrow orange + pink
buttons) is refused and closed. The NPC click is tried at several horizontal offsets around where the camera model
puts it (the memory position is not exact), skipping spots near other NPCs. Verified live on Phil's Call in Lith Harbor:
list -> quest page -> ACCEPT -> explanation pages -> OK; Phil's bubble changes from a lightbulb to dots.
Hand-in conversations are not verified yet (the first meso quest, Phil's, completes at level 10).

### Kill step

```json
{"kind":"kill","map":10000011,"mobs":[{"id":6,"count":8},{"id":7,"count":8}]}
```

Fights in the map until each listed mob template has been killed that many times since the step began (the brain's
own kill counter), attacking only the mobs still missing, then restores the mob filter. A whole kill quest is
`quest` (accept), `kill`, `quest` with `"complete":true`, e.g. Olaf's First Training (10000: 8 Stumps + 8 Slimes, 277 mesos).

Quests that pay mesos and are reachable from Henesys at level 8: Phil's Call (10001, Lith Harbor, 245 mesos + 188 EXP;
accept at level 8, hand in at level 10), Olaf's trainings (10000/10009, kill 8+8 mobs, 277 each). Every quest under
level 8 is on Maple Island.

Kill steps can also ask for items (counted from the server's pickup messages since the step began) and keep hunting
the step's mobs until they are in: `"items":[{"id":4000005,"count":10}]` (Fixing Blackbull's House).

### Job advancement

`{"kind":"quest","quest":0,"npc":511,"map":10004003,"yes":true,"until_job":100}` talks to Dances with Balrog,
answers YES, and is done once the server's stat update reports job 100 (it is retried if the job does not change; a
stream that reports no job at all falls back to the conversation's own success).

### Equip step

`{"kind":"equip","item":"inv:equip:0:0","if_bought":true}` double-clicks an Equip-tab cell and checks that the cell
changed. With `if_bought` it is skipped unless the shop step before it bought something.

### Shops by NPC id, long lists

A shop step may name the NPC instead of a click position: `{"kind":"shop","npc_id":507,...}` walks up to River and
clicks where the camera shows him (map and position from the map data, like quest NPCs). Buy rows past the visible six
are reached with the mouse wheel; how far one wheel step moves the list is measured from the row icons, and nothing is
bought when that cannot be told. With `NEON_DEBUG_DIR` set, each shop's list is saved as a screenshot when it opens,
to read rows from.

### Resupply

A plan-wide `"resupply":[shop steps]` (or a grind/kill step's own `"resupply"`, `[]` for none) runs when the potions
run out during a grind or kill step; the interrupted step then starts over. Use `[]` on steps that collect etc-tab quest
items, since a shop trip sells the etc tab.

### Character build (AP, SP, skills)

`neon-loader --build builds/warrior.json` spends points per level as the build says (the meowdb Warrior guide: STR
with DEX at 15, 19-20, 29-30; Power Strike, Slash Blast, Precise Strikes ...) instead of all STR, puts learnt skills
on quick slots (Power Strike on Shift, Slash Blast on Del, Iron Body on End, Recovery on PgUp: click the icon in the
skill window, click the slot) and adds them to the fight: Slash Blast when three or more mobs are within 130 px ahead,
else Power Strike, both with MP floors; buffs on a timer. Points are taken to belong to the newest levels; the server's
unspent AP/SP count limits them when known. The job tab is only used once the server reports the job.
`--assign-stats --build ... --level N --job J` does one pass by hand.

### Routing

Cross-map travel takes the route whose strongest mob (maps passed through) is weakest, then the shortest, over
regular and hidden portals (Tree Dungeon and the Southern Forest dungeons are entered through hidden ones). The bot
says so when even that route passes mobs more than 5 levels above it: Perion to Henesys always crosses Rocky Road
(Wild Boars, level 25).

## Discord alerts

Put the webhook URL in a file (or the `NEON_DISCORD_WEBHOOK` variable) and pass `--discord-webhook-file path`
(`--discord-events level,death,lowhp,stranger,lie,stop,lag`). The URL is never logged or written to the status file;
only `https://discord.com/api/webhooks/...` URLs are accepted; sending runs on a separate thread.

## Lie detector

When an anti-macro check appears, the bot should stop touching the game and get a human. The stream raises
`alerts.lie_detector` when it sees a registered incoming opcode (`--ld-opcode 0x1a3`, repeatable), and `neon-loader`
then pauses, shows a banner in the panel, and sends a Discord alert. The opcode is **not known yet**: run the stream
with `--opcode-log opcodes.json` (a histogram of every incoming opcode with sizes, rewritten every 5 s) while one
appears and look for a new opcode. There is no solver: answering the check is left to a person.

## Death handling

`--auto-revive` clicks "Revive in town" 3 s after dying (at most `max_revives_per_hour`, default 3), then the bot walks
back to `home_map` by itself. `--max-deaths N` ends the run. Both are off by default. The walk back crosses maps with
stronger mobs, so keep the death limit on for unattended runs.
