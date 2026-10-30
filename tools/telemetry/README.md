# Telemetry analysis

`analyze.py` turns Pacenote's run events into:

- `stats.json` for the website (`--stats web/src/data/stats.json`), schema in `docs/TELEMETRY.md`;
- `report.md`: a Reddit-ready summary (title ideas, headline numbers, per-stage table, deadliest
  corners, method note, posting checklist);
- charts as SVG and PNG (`finish_rates`, `codriver`, `crashmap_stageNN` for the three most-driven
  stages), dark, sized for Reddit (1200 px wide).

Standard library only. PNGs need `rsvg-convert` (`brew install librsvg`).

## 1. One-time setup (the user)

1. On the backend (`~/codes/synthai/backend`, api.marlstudio.com), create the app and a write
   key: `backend app create pacenote "Pacenote"` (prints `tk_…`; write keys only allow ingest
   and ship inside the game).
2. Build releases with the endpoint and key compiled in:
   `PACENOTE_TRACE_ENDPOINT=https://api.marlstudio.com PACENOTE_TRACE_KEY=tk_… ./packaging/…/build.sh`
   (keep the key out of git; it's not secret, but there's no reason to publish it).
3. The backend keeps raw events for its retention window (90 days by default): export at least
   monthly and keep the files. `analyze.py` merges any number of exports and drops duplicates.

## 2. Export events

From a machine that can reach the database (credentials stay with the backend's infra):

```sql
\copy (
  SELECT json_build_object('install_id', e.install_id, 'session_id', e.session_id, 'ts', e.ts,
                           'version', e.version, 'name', e.name, 'props', e.props::json)
  FROM events e JOIN apps a ON a.id = e.app_id
  WHERE a.slug = 'pacenote' AND e.category = 'game'
  ORDER BY e.ts
) TO 'pacenote-events-2026-11.jsonl'
```

A local `ashlar-trace-server` (SQLite) can be read directly with `--sqlite <data>/tracer.db`.

## 3. Analyse

```sh
python3 -I tools/telemetry/analyze.py --jsonl exports/*.jsonl --out out/2026-11 \
    --stats web/src/data/stats.json --demo-only --since 2026-11-01
```

Then rebuild the website (it reads `stats.json`). `--min-runs 25` (default) keeps stages with
fewer runs out of rankings and charts; they still appear in the report's table, starred.

`--simulate 600` makes labelled sample data from the real stage geometry, to test the pipeline
and the site layout. Everything it produces says SAMPLE: never post it.

## Testing end to end locally

```sh
# ashlar repo: build and start a local server, create the app (prints a key)
cargo build -p ashlar-trace-server
TRACER_DATA_DIR=/tmp/pn-trace target/debug/ashlar-trace-server app create pacenote Pacenote
TRACER_DATA_DIR=/tmp/pn-trace TRACER_BIND=127.0.0.1:8097 TRACER_RATE_PER_MIN=0 target/debug/ashlar-trace-server serve
# pacenote: play with it (events are only sent after the main menu showed, so start from the menu)
PACENOTE_TRACE_ENDPOINT=http://127.0.0.1:8097 PACENOTE_TRACE_KEY=tk_… cargo run
python3 -I tools/telemetry/analyze.py --sqlite /tmp/pn-trace/tracer.db --min-runs 1 --out /tmp/pn-report
```

## Reading the numbers honestly

- Co-driver on vs off is a correlation: players choose to turn it off.
- Demo players drive three stages, mostly once or twice: early runs dominate. Say "demo runs".
- `speed_kmh` on crashes is the closing speed implied by the damage, not the car's speed.
- Perf events (`perf.hitch`) during the loading screen are expected; the report ignores them.
