# Driver + guide over the network

Two machines (or two processes) play one stage. The **driver hosts** a listen server (engine
`net`, UDP) and runs the car simulation; the **guide joins**, receives the stage and the car,
and rides in the passenger seat with the pace notes on a tablet (Settings > Co-driver >
Co-driver view: "In car", the default) or reads a full-screen top-down map ("Map screen"). One
guide per driver.

## Run it

```sh
pacenote --role driver --host 7777 --stage 3        # or just --host 7777
pacenote --role guide --connect 192.168.1.20:7777   # or just --connect ...
```

`--role driver` without `--host` hosts on 7777; `--role guide` without `--connect` joins
127.0.0.1:7777. From the menus (meta): "Host as driver" / "Join as guide" call the same
functions through `src/meta/netlink.rs`. A guide joined from the menu enters the game when the
driver's stage arrives.

Two processes on one machine, offscreen, with screenshots: run `scripts/play.py` twice (driver
first), e.g. `-- --role driver --host 7791` and `-- --role guide --connect 127.0.0.1:7791`.
Several worktrees share one target dir, so copy the binary right after building and run the
copy (the shared `target/debug/pacenote` can be another branch's by the time you start it).

## Files

| File | What |
|---|---|
| `src/net_session.rs` | `Role` (Solo / Driver / Guide), `Session` (peer, names, RTT, link state), `host`, `join`, `leave`, `host_with` / `join_with` (any transport, for tests), the `Hello` name message, guide auto-reconnect |
| `src/guide/sync.rs` | Replicated `NetStage`, `NetRun`, `NetCar` + quantising codecs, `road_hash`, the driver's publishing systems |
| `src/guide/mod.rs` | `GuidePlugin`; guide side: `GuideStage` (road + pace notes regenerated from the id), `GuideFeed` (latest car / run, clock extrapolated), `upcoming` notes, `GuideScreen` (in car / map) |
| `src/guide/seat.rs` | In car: the stage's scenery built on the guide (no `StageWorld`, no `DrivenCar`), the drawn car (`GuideCar` + `art::car::CarPose`: wheels, steering wheel, gauges and the driver's arms animate), the passenger-seat camera (model node `CoDriverEye`, else the driver's eye mirrored), driver silhouette |
| `src/guide/smooth.rs` | `PoseSmoother`: steady pose from the interpolated `NetCar` (extrapolates late packets, filters rotation quantisation), acceleration for the head |
| `src/guide/head.rs` | `HeadMotion` (g-forces on the neck) and `Look` (mouse look within a seated range) |
| `src/guide/tablet.rs` | The tablet: the map + panel rendered into its texture (`TabletImage`), resting on the knees, raised with Space / right mouse, held in the guide's hands |
| `src/guide/coach.rs` | Call timing: grades each note's call (voice onset or `E`), cue bar on the panel, score at the finish |
| `src/guide/map.rs` | The map: 2D ortho camera, road as retained gizmos, highlight, labels, car arrow, controls |
| `src/guide/panel.rs` | The guide's panel: status, clock, speed, NEXT call, coming-up list, splits, overlays |
| `src/guide/notice.rs` | The driver's only hint: "GUIDE CONNECTED 17 MS" top-right |
| `tests/guide_net.rs` | Two headless apps on the engine's `LoopbackNet` |

## What goes over the wire

Snapshots at 30 Hz (`NetConfig::send_interval = 2`), delta-compressed by the engine.

| Component | Contents | Bytes before delta |
|---|---|---|
| `NetStage` | stage id, road hash | ~10, on change |
| `NetRun` | countdown, time, penalty, progress, splits, finished | 8–20, every 6 ticks + on events (split, finish, penalty, countdown second) |
| `NetCar` | pos (1 cm), rot (32-bit smallest-three), vel (5 cm/s), steer, rpm (40), gear, 4 wheel spins (0.1 rad/s), throttle (1/255) | ~31, interpolated on the guide |
| `NetDamage` | body parts knocked off (`vehicle::PARTS` bits), engine / steering / suspension damage (5 % steps) | 4, on change |

Values are quantised before they go into the component, so a parked car compares equal and
isn't resent, and the guide sees exactly what the driver encoded. The guide regenerates the
road from the id (1 ms, no terrain or dressing) and compares `road_hash`; a mismatch shows
"STAGE DIFFERS FROM THE DRIVER'S" in red.

The guide never simulates: it has no `StageWorld` and no `DrivenCar`, so the drive systems and
the driver HUD don't run there, and it never reads driving actions.

## Measured (Apple M5 Pro, debug build)

- Loopback test, car moving every tick: guide downlink 1.2 KB/s.
- Two processes over UDP on localhost: 2.0 KB/s down, RTT 17 ms.
- Stage regeneration on the guide: 1 ms.

## Guide controls

Map screen: mouse wheel or `+` / `-` zoom, left-drag pans (stops following), `F` follows the
car, `M` shows the whole stage, `H` heading-up. In car: the mouse turns the head, hold Space /
right mouse / left trigger to raise the tablet, wheel zooms its map. Both: `E` (pad A) marks a
call when not using voice chat (talking starts a call by itself). All are `engine::actions`
(group "Guide"), rebindable.

## In-car view and the network

The in-car view is drawn from the same `NetCar` as the map, plus `NetDamage`: the guide's car
drops the same body parts as the driver's (`art::props`) and `GuideFeed::damage` carries the
damage for the guide's screens. Adding `NetDamage` changed the protocol id (registered types are
hashed into it), so a build from before it is refused at the handshake instead of mis-reading
snapshots. The guide's machine builds the stage's scenery behind the loading screen, so the
link timeout is 12 s (`NetSessionPlugin`) to ride out that load's long frames on either side.

## Link loss

- Guide leaves or drops: the driver keeps driving; the label turns red "GUIDE DISCONNECTED".
- Driver goes away: the guide keeps its map, shows "LINK LOST RECONNECTING" and retries every
  2 s (`RETRY_SECS`) until `leave`. `NetConfig::drive_app_state` is off so the guide isn't
  thrown to the main menu.
- `leave` returns to `Role::Solo`.

## Known limits

- Names: `Session::name` is set by meta's netlink from the profile; command-line sessions send
  an empty name.
- The map has no terrain shading or hazard marking yet (design: the guide can mark hazards).
- A second guide is refused (`max_clients = 1`); after a crash the old slot frees on the 12 s
  timeout, the retry loop covers it.
- In car: wheel suspension travel and the handbrake aren't replicated (the guide's car shows
  the springs at static load, handbrake off); call timing is shown to the guide only.
- No relay / NAT traversal (engine limit): LAN or a forwarded port.
