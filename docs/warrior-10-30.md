# Warrior 10 -> 30 (meowdb leveling guide)

The guide (meowdb.com/msclassic/guides/warrior-leveling-guide-1-30) as a plan and a build:

- `plans/warrior-10-30.json`: job advancement, Fixing Blackbull's House, Rain-Forest East of Henesys to 20, Tree
  Dungeon IV to 30, Camila's Gem at 24, potion resupply from Luna, back to Dances with Balrog at 30.
- `builds/warrior.json`: AP per level (STR, DEX at 15 / 19-20 / 29-30), SP per level (61 Warrior points:
  Power Strike 20, Slash Blast 19, Precise Strikes 15, Improved HP Recovery 3, Max HP Increase 3, Iron Body 1; the
  Beginner's 9 into Three Snails / Recovery / Nimble Feet), quick slots and how each skill is used in fights.

```bash
scripts/run-bot.sh --session NAME -- --plan plans/warrior-10-30.json --build builds/warrior.json \
  --hp-key Insert --mp-key Home --auto-revive --max-deaths 3
```

`maple-island-to-perion.json` ends where this plan starts (Warriors' Sanctuary, level 10).

## What is verified and what is not

| Part | State |
|---|---|
| Mesos, SP and item pickups from the server | Decoded from a live capture (KryJib01, 2026-10-25), unit-tested with those packets |
| Job from the server | Decoded as u16 (unverified size: the walk stops after it). Check on the first advancement |
| Stat window DEX "+" | Measured (18 px below STR) on KryJib03's window |
| Skill window rows, "+" and icons | Measured on the Beginner tab (rows 40 px apart, six fit). The Warrior tab's position (one tab right of Beginner) and its row order (skill id order, from String.wz) are not seen yet: `show_tab` refuses to click unless a fourth row is there |
| Skills onto quick slots | Same click-pick-up / click-drop as potions (verified for potions), not yet for skills |
| Slash Blast / Power Strike in fights | Unit-tested decision; not run live |
| Routing around strong maps | Unit-tested and checked on the real portal graph (Perion -> Henesys tops out at level 25 instead of 37) |
| Equip step, shop scrolling, shop by NPC id | Unit-tested logic; the clicks are not verified live |
| Weapon purchases | **Not filled in.** Shop lists are server data (not in the client files), so the rows must be read off the screen once. Run with `NEON_DEBUG_DIR=...`: each shop's list is saved when it opens. Then put `{"slot": N, "qty": 1}` in the plan's River step |
| MP potions | Not bought: only Red Potion's row (0 at Luna) is known. Power Strike and Slash Blast stop at their MP floors and the basic attack takes over |

## First live run checklist

1. A level-10 character at Dances with Balrog, potions on Ins/Home, in a quiet channel; watch it.
2. The advancement: the log should show `job is already 100 (server)` or the step finishing, then
   `skill point on power_strike` and `power_strike put on the shift quick slot`.
3. If `skill tab 1 did not show`, take a screenshot with the skill window open and correct `SKILL_FIRST_TAB` /
   `SKILL_TAB_PITCH` in `neon-loader/src/stats.rs`.
4. Watch the first Slash Blast: it costs HP as well as MP.
