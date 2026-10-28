# Working on Pacenote as a team of agents

Read `docs/DESIGN.md` first (what the game is), then `/Users/himanshu/codes/game/engine/CLAUDE.md`
and `/Users/himanshu/codes/game/engine/docs/GAME_GUIDE.md` (how to build games on the engine:
perf rules, pitfalls, measuring). The module docs in `engine/docs/` cover each engine system.

## Worktrees and branches

- Main checkout: `/Users/himanshu/codes/game/pacenote` (branch `main`). The lead integrates
  there; don't commit to `main` yourself.
- Your worktree: `git -C /Users/himanshu/codes/game/pacenote worktree add
  /Users/himanshu/codes/game/pacenote-wt/<stream> -b <stream>`. Work, build and commit only
  there, on your branch. Commit as you go (small commits, clear messages, ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`). Never push.
- Before handing back: `git rebase main` (resolve conflicts), all tests green.

## Keeping merges easy

- Put your work in your own module(s) under `src/` (e.g. `src/guide/`, `src/voice.rs`) with one
  `Plugin`. Touch shared files minimally: one `pub mod x;` line and one `.add_plugins(x::XPlugin)`
  line in `src/lib.rs`. If you need a change in someone else's module, keep it tiny and say
  so in your hand-back.
- Shared types the others need (e.g. the replicated car state, the role resource) go in small
  files named in your hand-back, documented.
- No new heavy crates. `cpal` is already in the dependency tree (via the engine's audio).
  Never add anything that pulls a second Bevy or wgpu. Don't change engine Bevy features.
- The engine (`/Users/himanshu/codes/game/engine`) is owned by the lead: if you need an engine
  change, describe it in your hand-back (or message the lead) instead of editing it.

## Building and running

- Every cargo command through `memgate` (it queues jobs machine-wide): `memgate -- cargo
  build -q`, `memgate -- cargo test -q --lib`. Waiting in the queue is normal.
- The target dir is shared (`.cargo/config.toml`), so Bevy compiles once. Several worktrees
  build the same binary name: copy `target/debug/pacenote` right after your build if you run
  it, or run through the engine's play script with `--manifest`.
- Run offscreen and screenshot (look at every screenshot):
  `python3 -I /Users/himanshu/codes/game/engine/scripts/play.py steps.json --manifest
  <your worktree>/Cargo.toml --bin pacenote --id pacenote --out <dir>`. Steps: `{"cmd": ...,
  "params": {...}}`, `{"shot": "name"}`, `{"sleep": s}`. Input: `input.key {key: "KeyW",
  action: "down"|"up"|"tap"}`. `--stage N` picks a stage (pass after `--` to play.py).
  Inspector: `ecs.query`, `ecs.get` (devkit), `render.stats`, `perf.stats`, `pcg.stats`.
- Two instances (driver + guide) for networking: run two processes on loopback (e.g.
  `--host 7777` and `--connect 127.0.0.1:7777`, see engine `docs/NET.md`).
- Keep debug builds; no `--release` unless the lead asks (disk).

## Code map (main)

| File | What |
|---|---|
| `src/stage.rs` | 20 stage definitions, road growth, terrain shaping, corners/crests (pure, tested) |
| `src/vehicle.rs` | Car simulation (pure, deterministic, tested): `Car`, `Input`, `Spec`, `step`, `Ground` |
| `src/world.rs` | `StageWorld` resource (road, heights, corners, obstacles; implements `Ground`), terrain/road/dressing spawn |
| `src/drive.rs` | Actions, fixed-step sim (`DrivenCar` component), car visuals, cameras, `StageRun` (clock, splits, progress) |
| `src/hud.rs` | Driver HUD |
| `assets_src/SOURCES.md` | Downloaded CC0/CC-BY assets (raw files local only, not in git) |

## Hand-back

Your final message: what you built (files), how to run/see it, test results, screenshots you
checked, known gaps, and anything the lead must change in shared files or the engine.
