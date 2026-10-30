# Telemetry

Anonymous gameplay stats from the demo and the game, used to balance stages and to publish
aggregate numbers ("stage 7's hairpin ended 1 in 3 runs"). On by default; players turn it off
in Settings → Profile ("Share anonymous stats"), and the first launch says so on the main menu.

## Pipeline

1. **Game → server.** The engine's tracer (`engine/src/trace.rs`, `ashlar-trace`) sends
   crash, hang and perf reports plus the game events below to the ashlar trace server at
   `api.marlstudio.com` (`~/codes/synthai/backend`). Endpoint and write key are compiled into
   release builds from `PACENOTE_TRACE_ENDPOINT` / `PACENOTE_TRACE_KEY` at build time (or read
   from the same variables at run time). Without them nothing is sent.
2. **Server → export.** Events land in the backend's Postgres `events` table (props as JSON).
   Export them as JSON lines (see `tools/telemetry/README.md`).
3. **Export → analysis.** `tools/telemetry/analyze.py` turns the export into a Markdown report
   (Reddit-ready), SVG charts and crash maps, and `web/src/data/stats.json` for the website.

## What is sent

No names, no chat, no voice, no IP-derived location. Ids are a random install id and session
id (SDK). Device: OS, arch, CPU/GPU name, RAM, locale (SDK, see ashlar `docs/trace/privacy.md`).

Game events (category `game`; props are ints, floats, bools or short strings):

| Event | When | Props |
|---|---|---|
| `run.start` | the countdown ends | `stage`, `mode` (`solo`/`team`), `codriver` (bool), `camera` (`cockpit`/`bonnet`/`chase`), `demo` (bool) |
| `run.end` | the run ends | `stage`, `result` (`finish`/`retired`/`quit`/`restart`), `time_ms` (clock incl. penalties), `penalty_s`, `progress_m`, `hits`, `engine_pct`, `steering_pct`, `suspension_pct`, `recovers`, `codriver`, `demo` |
| `run.crash` | a hit of severity ≥ 0.04 | `stage`, `s_m` (metres along the road), `corner` (index into the stage's corners, −1 on a straight), `grade`, `severity` (float, 1 = fatal), `speed_kmh` (closing speed the severity implies), `retired` (bool: this hit ended the run) |
| `run.recover` | the recover button | `stage`, `s_m` |
| `demo.wishlist` | a wishlist button in the demo | `where` (`menu`/`end`) |

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
