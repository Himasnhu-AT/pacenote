# Built: economy, garage, career save, rivals and races (stream F2)

What landed for DECISIONS §2.1 / §2.4 and GAME_SYSTEMS §4-§5. Code is the reference; this is
the map and the deviations.

## Files

| File | What |
|---|---|
| `src/vehicle.rs` | `Spec.grip` (tyre μ ×) and `Spec.shift_time` (E6). A stock Saker's `sim_hash` is unchanged (`meta::ghost` hashes the new fields only when non-default; test `a_stock_cars_sim_hash_is_unchanged`). |
| `src/career/garage.rs` | The 8-car roster (`CARS`), `build_spec` (7 parts), `pi`, `Class`, prices, `can_buy/can_fit/can_sell`, `sale_price`, `service_cost`, `stat_bars`, paints. |
| `src/career/wallet.rs`, `grade.rs` | `Wallet::credit/debit` (never negative); grade curve `xp = 3,000 × (g − 1)²`. |
| `src/career/save.rs` | `career.json`: versioned, atomic, `career.bad.json` on a bad file, sanitised on load. `CareerProgress` = pose, challenges, races, stats, ledger (last 50), last event mode. |
| `src/career/economy.rs` | The only place Notes move: `Payout`/`Charge` → wallet + ledger + `MoneyEarned`; `CareerRequest` handling (buy, fit, select, paint, number, service, sell) with `Refusal` toasts and `LastRefusal`; race and Time Trial pay; Style bank → grade xp; `CarSpec` = the active build; garage paint on the livery; damage/pose tracking; debounced save. Dev: `career.state`, `career.request`, `career.give`. |
| `src/pilot.rs` | `Pilot`: the AI driver, compiled in every build. `autopilot.rs` is now a shell over it. |
| `src/career/racers.rs` | `RaceSim` (pure): field, starts, racecraft, positions, gaps, result. Rules as pure fns (`rubber_band`, `slot_spot`, `start_format`, `pick_cars`). |
| `src/career/race_flow.rs` | The ECS shell: `begin`/`end`, grid placement, pursuit and Recover holds, stepping after `SimulateSet`, `RacerSolids`, `OtherCars`, `RaceResult`, rival bodies. Dev: `race.state` (incl. AI ms). |
| `src/open/events.rs` | Race / Time Trial at the marker: 1-4 difficulty, Enter Race, T Time Trial, holding still = last mode (`events::Choice`, saved). `start_as(world, id, mode)`; `on_route` (pure). Start cylinders hide during events. |
| `src/hud_race.rs` | Position block (P x/6, change, ahead/behind gaps once a second, rival strip, start/hold line) and the results card (order, gaps, purse lines, balance; Enter Retry, F Free roam). Setting HUD › Race positions. |

## Deviations (decided here)

- **Brakes part:** a flat ×1.1-1.3 on the foot brake made stops *longer* (the sim's ABS stops
  shortest at ~4,200 N·m per 1,250 kg × grip). The part now brings the foot brake up to what
  the build can use (`abs_torque`), and scales the handbrake ×1.1/1.2/1.3. Pays off on heavy
  cars and with Tyres; never lengthens a stop (tested).
- **PI:** the formula as written puts the stock Ossian at 742 (the table's 728 left out its
  brakes ×1.15). Still class S. A full Saker build reaches S (840).
- **Upgrade prices** use the car's *stock* class, so they don't jump when a build crosses a class.
- **Racers** live in the `RaceRun` resource (`RaceSim.rivals`, slot order), not as components;
  rival bodies are separate `RivalBody` entities. Deterministic and testable headless.
- **Stalled cars:** a rival stopped behind a stopped car for 3 s "ghosts" through it for 4 s
  (not solid); a rival off the road 3 s, turned round, or without progress for 10 s is put back
  at its progress by the marshals. Neither happens in the Pro route tests.
- **Results** show in the race HUD card (right side); `meta::watch_finish` skips board, ghost
  and the results screen for races (one guarded line in `meta/mod.rs`). E can move them to the
  results screen from `LastRace` + `LastPayout`.
- **Body:** the player always drives the rally-hatch model; the garage paint tints its livery.
  Other body shapes for the player (with a cockpit) need art work. Rivals use the traffic
  bodies (`body_shape`) in their paint.

## Tests

`cargo test --lib career pilot autopilot` (fast). Slow, ignored (load the open world):
`rivals_finish_every_route_at_pro` (5 rivals finish every Rally/Sprint route at Pro, player
parked, no reset), `rivals_are_deterministic` (+ start formats, no overlap on the grid);
tuning aids `pace_sweep`, `pace_trace` (env `RACE_EVENT`, `PACE`, `FROM`, `TO`, `CAR`).
