# Pacenote: game brief

One page on what Pacenote is, who it's for and what it needs next. Written 5 Nov 2026 from
the code and the docs. For depth, see `GAME_OVERVIEW.md` (full state), `OPENWORLD.md` (world),
`HANDLING.md` (driving) and `festival/` (progression and economy).

## Pitch

**A rally festival in one open valley, where the road is read to you.** Drive a 4×4 km
valley, start events from markers in the world, and race point-to-point routes against the
clock or AI rivals. A co-driver calls the pace notes ("left four into right two, don't cut,
crest").

## At a glance

| | |
|---|---|
| Genre | Open-world rally / racing |
| Platform | Steam: macOS (Apple Silicon first) and Windows |
| Business | Free demo to build wishlists, then a paid game |
| Players | Solo. Two-person driver + navigator over LAN, on the legacy stages only for now |
| Controls | Keyboard or gamepad |
| Tech | Own engine "Hull" (Rust, Bevy 0.20); custom deterministic 60 Hz car sim |
| Team | One owner, with AI agents doing much of the work (`TEAM.md`) |

## Audience

Players who like Forza Horizon's "drive anywhere, always something to do" feel but want
driving that is harder to master. Rally fans (DiRT Rally, WRC) who want something more
relaxed than a stage-only sim.

## Pillars

1. **Driving that feels earned.** Weight transfer, gravel slides, crests that go light.
   Mistakes cost time, not restarts. Upgrades change the physics, not just stat bars.
2. **Roads worth learning.** Narrow rally roads, real hairpins, pace notes taken from the
   road's actual curvature.
3. **Read the road.** Pace notes are the hook: an AI co-driver in events, a friend as
   navigator in co-op. *(Not settled yet: see Open question.)*

## Core loop

Drive the valley → find an event or roadside challenge → race it (Time Trial against your
ghost, or Race against 5 AI rivals) → earn Notes (money) and xp, plus Rhythm bonuses for
style → buy and upgrade cars at the Service Park → take on harder events and classes.

## What exists today

- **World:** 21 roads (~38 km), the town of Fernhollow, 40 traffic cars, day/night, minimap
  and route planning.
- **Events:** 5 routes (2 Rally, 3 Sprint), each as Time Trial or Race at 4 AI difficulties.
- **Challenges:** 13 roadside challenges (Radar Post, Pace Section, Slide Section, Yump
  Board), rated 1–3 stars.
- **Cars and progression:**
  - 8 cars and 7 upgrade parts.
  - PI (performance index) classes D–S.
  - Driver Grade 1–30 and a saved career.
- **Feel:** damage, exhaust pops and flames, recorded engine audio, 11 style skills that
  chain into Rhythm.

## Out of scope (for now)

- A huge map or hundreds of cars: Pacenote competes on feel, not size.
- Licensed cars, brands or real locations.
- Online matchmaking: comes after co-op works in the open world.

## Open question

**What is the hook?** The original game was "one drives, one reads the road." The open world
moved away from that: pace notes only matter inside events, and co-op is stuck on the old
stages.

The 5 Nov review recommends putting pace notes back at the centre:
- open-world co-op with a navigator;
- a "recce" drive where you write your own notes;
- challenges with the map turned off;
- a daily route that everyone drives.

## Next priorities

1. Decide the hook and rewrite the website and store copy to match. They still sell the old
   20-stage game.
2. Playtest with 5–8 people.
3. Bring driver + navigator co-op into the open world.
4. Fix the gravel steering overshoot on keyboard (`HANDLING.md`).
5. Add a 10-minute onboarding drive.
6. Make progression unlock things, and stop one short race being the best way to earn money.
7. Add more content: a new event type, more events and challenges, and things to discover.
