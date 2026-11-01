# Telemetry

Anonymous gameplay stats and crash reports from the demo and the game, used to balance stages,
fix crashes and publish aggregate numbers ("stage 7's hairpin ended 1 in 3 runs"). itch.io has
no analytics, so this is the only way to know how the game plays. On by default, with a
first-launch notice and opt-outs in Settings > Privacy.

## Pipeline

1. **Game → server.** `src/telemetry.rs` records game events through the engine's tracer
   (`engine/src/trace.rs`, `ashlar-trace`), which also sends crash, hang and performance
   reports, to MarlStudio's server (`api.marlstudio.com`, `~/codes/synthai/backend`). Endpoint
   and write key are compiled into packaged builds (`PACENOTE_TRACE_ENDPOINT`,
   `PACENOTE_TRACE_KEY`, see "Building with the key"); the same variables at run time win.
   Without both, the tracer is off: nothing is recorded, no notice is shown.
2. **Server → export.** Events land in the backend's `events` table (props as JSON). Export them
   as JSON lines (see `tools/telemetry/README.md`).
3. **Export → analysis.** `tools/telemetry/analyze.py` turns the export into a Markdown report,
   SVG charts and crash maps, and `web/src/data/stats.json` for the website.

## Envelope

| Field | Value |
|---|---|
| `app.name` | `pacenote` (the server's app slug; `GameInfo.id`) |
| `app.version` | `CARGO_PKG_VERSION` of this crate |
| `app.build` | the git sha, **packaged builds only** (`packaging/common.sh` sets `PACENOTE_BUILD_SHA`). A dev `cargo run` sends none, and the server treats it as test data |
| `app.process` | `gui` |
| `install_id`, `session_id` | random, made on the machine; "Forget me" makes a new install id |
| `device` | OS, OS version, arch, CPU model and cores, RAM, locale (SDK) |

## Consent, the notice, forget, DO_NOT_TRACK

- **Notice.** The first time the main menu opens with the tracer on, a "Privacy / Anonymous stats"
  screen says what is shared and how to turn it off. The SDK records from the start but sends
  **nothing** until the player closes the notice with *OK* or opens *Privacy settings* from it
  (`notice_shown`, remembered). Escape leaves it unanswered: nothing is sent, and it shows again
  next launch. (`GameShowsTraceNotice` tells the engine not to count the main menu as the notice.)
- **Settings > Privacy.** "Share anonymous stats" → usage + performance; "Send crash reports" →
  crash (crashes, freezes, handled errors). Switching one off deletes what's waiting and stops it at
  once (`set_consent`, saved by the SDK; mirrored in `Profile.share_stats` /
  `send_crash_reports`).
- **Forget me** (same tab): deletes everything stored locally, asks the server to delete the
  install's data (`POST /v1/forget`, retried next launch if offline) and issues a new install id.
- **`DO_NOT_TRACK`** or **`TRACER_DISABLED`** set to anything but empty, `0` or `false` turns the
  SDK off for that run: no folder, no file, no request, no notice. Both are honoured.
- **Inspect.** `PACENOTE_TRACE_INSPECT=1` also writes everything accepted for sending to
  `<data dir>/trace/tracer/outbox.jsonl`.

## Lifecycle

- The tracer starts in `TracePlugin::finish` (before the first frame) and wraps the panic hook.
- Hang watchdog and frame times from the main loop (`First`…`Last`), armed after the first
  in-game frame. While the loading screen is up (`TraceQuiet`) there are no hang turns and no
  `perf.hitch`: long frames are what loading is.
- Handled errors: `telemetry::handled(world, &e, "ghost.save" | "board.save")`.
- On quit the session is ended and sent with at most 1.5 s of waiting (`shutdown_within`); what
  can't go out goes with the next launch.

## Events

All in category `game` (plus the engine's `perf.hitch {ms}` in `perf`), declared in
`telemetry::CATALOG`. Props are bools, ints, floats or strings from the fixed lists below; never
names, addresses, lobby codes, file paths or free text. Every send is checked against the
catalog in debug builds and tests.

| Event | When | Props |
|---|---|---|
| `session.settings` | once per session | `camera` (`cockpit`/`bonnet`/`chase`), `units` (`metric`/`imperial`), `steering_speed` (float), `traction_assist`, `abs`, `codriver` (bools), `codriver_timing` (`early`/`normal`/`late`), `codriver_pace` (`calm`/`normal`/`fast`), `call_order` (`direction_first`/`grade_first`), `subtitles` (bool), `quality` (`low`/`medium`/`high`/`ultra`), `channel` (`demo`/`full`) |
| `run.start` | the countdown ends | `stage`, `mode` (`solo`/`team`), `codriver`, `camera`, `weather` (`clear`/`overcast`/`fog`/`rain`), `time_of_day` (`morning`/`noon`/`afternoon`/`dusk`), `traction_assist`, `abs`, `demo` |
| `run.end` | the run ends | `stage`, `result` (`finish`/`retired`/`quit`/`restart`), `time_ms` (clock incl. penalties), `penalty_s`, `progress_m`, `hits`, `broken` (saplings, posts and signs knocked down), `recovers`, `codriver`, `mode`, `demo` |
| `run.crash` | a hit of severity ≥ 0.04 | `stage`, `s_m` (metres along the road), `corner` (index, −1 on a straight), `grade`, `severity` (float, 1 = a big crash), `speed_kmh` (closing speed the severity implies) |
| `run.recover` | the recover key | `stage`, `s_m` |
| `play_together.connect` | once per host / join attempt | `result` (`ok`/`failed`/`timeout`/`refused`), `role` (`host`/`join`), `latency_ms` (only with `ok`, when known) |
| `play_together.end` | the partner goes, or the session is left | `duration_s`, `reason` (`left`/`partner_left`/`timeout`/`error`), `role` |
| `demo.store_click` | the demo's store button | `where` (`menu`/`end`), `store` (`itch`/`steam`) |

## Building with the key

`packaging/common.sh` (telemetry builds):

- `PACENOTE_TRACE_ENDPOINT` defaults to `https://api.marlstudio.com`.
- `PACENOTE_TRACE_KEY`: the environment wins; else the `PACENOTE_WRITE_KEY=` line of
  `~/codes/synthai/backend/infra/secrets/keys.env` (`PACENOTE_KEYS_FILE` overrides the path).
  Only that line is read. The key is never printed: the log says `telemetry: key set (N chars)`
  or `key not set`, and a build without a key works and sends nothing (with a warning).
- `PACENOTE_BUILD_SHA` = the git sha (the envelope's `build`).

### Creating the app and key (once)

On a machine with the backend's deploy SSH key (`~/codes/synthai/backend/infra`):

```sh
cd ~/codes/synthai/backend/infra
source lib.sh
# Creates the app on the server and appends its write key to secrets/keys.env, without
# printing it.
key=$(backend_cli app create pacenote "Pacenote" | sed -n 's/^write key: //p')
[ -n "$key" ] && ( umask 077; printf 'PACENOTE_WRITE_KEY=%s\n' "$key" >> secrets/keys.env ) && echo "saved (${#key} chars)"
unset key
```

Then add `mk pacenote "Pacenote" "$PACENOTE_WRITE_KEY"` next to the other `mk` lines in
`infra/deploy-backend.sh`, so a redeploy keeps the app with the same key. Check:
`curl -s https://api.marlstudio.com/v1/health`, then package (`EDITION=demo
packaging/macos/build.sh` prints `telemetry: key set (…)`), run the build once, close the notice,
quit, and look for the session on the dashboard (`/apps/pacenote`).

### Testing without the production server

- `PACENOTE_TRACE_INSPECT=1` with any endpoint and key: `outbox.jsonl` shows every item.
- A local `ashlar-trace-server` (ashlar repo): `ashlar-trace-server app create pacenote Pacenote`,
  `TRACER_BIND=127.0.0.1:18090 ashlar-trace-server serve`, then run the game with
  `PACENOTE_TRACE_ENDPOINT=http://127.0.0.1:18090` (plain http only for loopback) and the printed
  key. The `sessions` and `events` tables in its `tracer.db` show what arrived.

## `web/src/data/stats.json` (made by `analyze.py`)

```json
{
  "generated_at": "2026-11-01T12:00:00Z",
  "sample": true,
  "totals": { "installs": 0, "runs": 0, "finishes": 0, "retirements": 0, "km_driven": 0, "crashes": 0 },
  "stages": [
    { "id": 1, "name": "Pinewater", "runs": 0, "finish_rate": 0.0, "retire_rate": 0.0,
      "median_time_s": null, "best_time_s": null, "crashes": 0,
      "top_corner": { "index": 3, "turn": "left", "grade": 2, "start_m": 812, "crashes": 0, "retirements": 0 } }
  ],
  "corners_worst": [
    { "stage": 7, "stage_name": "…", "index": 4, "turn": "right", "grade": 1, "start_m": 1520, "crashes": 0, "retirements": 0 }
  ],
  "codriver": { "on_finish_rate": 0.0, "off_finish_rate": 0.0 }
}
```

`sample: true` means the numbers are made up for layout and must be labelled as such wherever
they show. Rates are 0..1. `top_corner` may be `null`.
