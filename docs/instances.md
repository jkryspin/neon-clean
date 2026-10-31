# Several accounts at once (instances)

An instance is one game account with its own Wine prefix, its own headless X display and its own bot session.
Clients never share a keyboard, a mouse or a screen, so any number can run next to each other and next to the
desktop client.

| Piece | Where |
|---|---|
| Registry (no secrets) | `neon-instances.json`: email, character, display, channel; optional `prefix` and `charm` (run an existing prefix headless, e.g. the main account's `NexonLauncherSetup-91dd6ddf36` with `nexon_launcher_headless.charm`, a copy of its charm with `--disable-gpu`), `plan` (default plan for `instance-bot.sh start`), `mp_key`, `hp_cell` |
| Passwords | `~/.config/neon/accounts.json`, mode 600, never in the repo |
| Prefix | `~/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/NexonLauncher-NAME` |
| Display | `Xvfb :<display>` (1600x1000) |
| Bot session | `--session NAME`: `neon-status.NAME.json`, `logs/NAME.log` |
| Max HP memory | `data/seen-max.NAME.json` (`--seen-file`) |

## New account

```bash
scripts/instance-prefix.sh jib03           # once: a prefix without the main account's login (about 1.1 GB)
scripts/instance-launch.sh jib03           # Xvfb, launcher (signs in the first time), PLAY, login, channel
scripts/create-character.sh jib03          # on an empty character screen: STR-first character, into the game
```

## Run a bot on it

```bash
DISPLAY=:103 NEON_STREAM_ARGS="--seen-file data/seen-max.jib03.json" \
  scripts/run-bot.sh --session jib03 --pid <game pid> -- --plan plans/maple-island-to-perion.json --hp-key Insert --mp-key Home
```

`instance-launch.sh` prints the game pid last. To watch a client: `DISPLAY=:103 import -window root shot.png`.

## What makes it work

- **Input:** XTEST events go to the instance's own X server, so the bot's keys never reach another client.
- **Rendering:** Xvfb has no GPU, so the client renders in software. `LP_NUM_THREADS=2` caps it at about 2 cores; uncapped
  it took 10. The launcher's embedded browser needs `--disable-gpu` (written into the instance's `.charm`), or it draws a
  blank page.
- **Window layout:** without a window manager the client window is the bare 1366x768 game view. UI coordinates are
  measured on the desktop's 1422x900 frame; `shop::Win` maps them onto either layout.
- **Network:** the packet capture sees every client on the machine. Each stream keeps only the TCP connection that its
  own game process holds (`net::game_ports_of`, looked up from `/proc/<pid>` every second).
- **Game data:** the 700 MB of `.wz` files are symlinked to the main prefix. Nothing that identifies the machine is changed.

## Limits

- Memory: each client takes about 1.5 GB. With two clients running, 2 GB of 15 GB was left.
- The launcher sign-in is typed from the credentials file. A CAPTCHA or an email code stops the script and needs a person.

## Two clients at once (measured 2026-10-26)

shurella2 (:104) and jib01 (:101) ran together with their bots for about 6 minutes, each with `--party-dir run/party`.

| | Per client | Per bot (loader + stream) |
|---|---|---|
| CPU | about 200% of one core (software rendering, `LP_NUM_THREADS=2`) | about 3-12% |
| Memory | 1.3-1.6 GB RSS, growing during the run | about 30 MB |

The 16-core, 16 GB machine had 3.4 GB available with two clients and the desktop session open, so a third client is
the practical limit here, not five. `scripts/instance-load.sh [--watch N]` prints these numbers per instance.

- Each bot read its own character (different maps, positions, HP); both published their claims to `run/party/`.
- Key isolation, checked on screen afterwards: `E` sent to :101 opened the equipment window on jib01 only; shurella2 on :104
  showed no change.
- jib01 ran out of potions (none on that character), fell to 33% HP, tried to retreat and died. I started it without
  checking its potions first; run `instance-bot.sh` only on characters that carry potions.
