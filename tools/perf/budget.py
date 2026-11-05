#!/usr/bin/env python3
"""Perf budget check (docs/ASSETS_BRIEF.md §1, festival TECH_PASS §4): one offscreen run of the
open world, several scenarios, each measured over a window with many samples, compared with the
budgets. Exit 1 if any p95 is over.

  python3 -I tools/perf/budget.py [--quality Low|Medium|High] [--secs 20] [--samples 8]
        [--scenarios tour,town,mountain,pass,event] [--event-id 103] [--event-race pro|none]
        [--pose name=x,z,heading ...] [--wait 45] [--features F] [--manifest PATH] [--out DIR] [--no-build] [--json]

Works from any checkout or worktree: the engine is found through the `engine` path dependency
in the manifest's Cargo.toml (not a sibling folder). `--features` goes to the build (worktrees
sharing a target dir pass their `wt_*` tag). Runs the dev build through engine/scripts/play.py
(offscreen, governor off as in all scripted runs, so the quality stays where it's set).

Scenarios (all in free roam, chase camera, one launch):
  tour      autopilot on `open.tour` (town street, east country road, the ring)
  town      parked by the town square (-285, -1150), looking across it and up the street
  mountain  parked at the mountain hairpin (-1890, 240)
  pass      parked on the high pass (1461, 1600)
  event     an open-world event (`events.start`, default 103: a 4.4 km Sprint; a Race with 5 AI
            cars unless --event-race none; the entry fee is given with `career.give`), the
            autopilot driving it

How it measures: before and after each window the script calls `perf.report`, which writes a
timestamped row into the session log (`<out>/data/logs/session-*.jsonl`). The engine also writes
one `perf` row a second there; the rows between a scenario's two markers are its samples:
  frame ms  median of the per-second p50s, p95 of the per-second p95s (the whole frame; the dev
            build is CPU-bound, so this is the CPU number)
  gpu ms    median and p95 of the per-second GPU frame time
  memory    footprint (MB), median per scenario, max over the run
Draw calls: `render.stats` `draws` (after batching) sampled `--samples` times across the window:
median, p95, min-max. Load time: the game log's `ready in N s`. A window that starts before the
world finished loading is flagged (raise --wait).

On the M5 Pro the numbers are optimistic for a base M1 (brief §1): a pass here is necessary,
not sufficient. Results also go to <out>/budget.json.
"""
import glob, json, os, re, statistics, subprocess, sys, tomllib
from datetime import datetime

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
BUDGETS = {  # brief §1: (Low, Medium/High)
    "load_s": (8.0, 5.0),
    "footprint_mb": (1500.0, 2500.0),
    "gpu_ms_p95": (10.0, 12.0),
    "frame_ms_p95": (16.6, 16.6),
    "draws_p95": (800.0, 2500.0),
}
POSES = {
    "town": (-285.0, -1150.0, 330.0),
    "mountain": (-1890.0, 240.0, 20.0),
    "pass": (1461.0, 1600.0, 200.0),
}


def opt(args, name, default):
    return args[args.index(name) + 1] if name in args else default


def engine_dir(manifest):
    """The `engine` dependency's path from Cargo.toml (absolute, or relative to the manifest)."""
    with open(manifest, "rb") as f:
        cargo = tomllib.load(f)
    for table in ("dependencies", "dev-dependencies"):
        dep = cargo.get(table, {}).get("engine")
        if isinstance(dep, dict) and "path" in dep:
            p = os.path.join(os.path.dirname(os.path.abspath(manifest)), os.path.expanduser(dep["path"]))
            if os.path.isfile(os.path.join(p, "scripts", "play.py")):
                return os.path.normpath(p)
            sys.exit(f"engine dependency at {p} has no scripts/play.py")
    sys.exit(f"no `engine = {{ path = ... }}` dependency in {manifest}")


def pct(xs, q):
    """Nearest-rank percentile (q in 0..1) of a non-empty list."""
    s = sorted(xs)
    return s[min(len(s) - 1, max(0, round(q * (len(s) - 1))))]


def summary(xs):
    xs = [x for x in xs if x is not None]
    if not xs:
        return None
    return {"median": statistics.median(xs), "p95": pct(xs, 0.95), "min": min(xs), "max": max(xs), "n": len(xs)}


def steps_for(scenarios, secs, samples, event_id, race):
    """The play.py steps: per scenario its setup, then a window between two perf.report markers."""
    steps = [{"cmd": "ui.setting", "params": {"key": "camera_view", "value": 2}}]
    gap = secs / samples

    def window(name):
        w = [{"cmd": "perf.report"}]
        for _ in range(samples):
            w += [{"sleep": gap}, {"cmd": "render.stats"}]
        return w + [{"cmd": "perf.report"}, {"shot": name}]

    for name in scenarios:
        if name == "tour":
            setup = [{"cmd": "open.tour"}, {"cmd": "autopilot.set", "params": {"on": True, "pace": 0.6}}, {"sleep": 4}]
        elif name in POSES:
            x, z, h = POSES[name]
            setup = [{"cmd": "autopilot.set", "params": {"on": False}}, {"cmd": "art.teleport", "params": {"x": x, "z": z, "heading": h}}, {"sleep": 6}]
        elif name == "event":
            p = {"id": event_id}
            if race != "none":
                p["race"] = race
            # The countdown (3 s), then the autopilot drives the event.
            setup = [{"cmd": "autopilot.set", "params": {"on": False}}, {"cmd": "career.give", "params": {"notes": 100000}}, {"sleep": 1}, {"cmd": "events.start", "params": p}, {"sleep": 6}, {"cmd": "autopilot.set", "params": {"on": True, "pace": 0.7}}, {"sleep": 3}]
        else:
            sys.exit(f"unknown scenario {name}: tour, town, mountain, pass, event")
        steps += [{"cmd": "metrics.label", "params": {"key": "scenario", "value": name}}] + setup + window(name)
    return steps + [{"cmd": "autopilot.state"}]


def main():
    args = sys.argv[1:]
    quality = opt(args, "--quality", "Low")
    secs = float(opt(args, "--secs", "20"))
    samples = max(1, int(opt(args, "--samples", "8")))
    wait = float(opt(args, "--wait", "45"))
    for i, a in enumerate(args):
        if a == "--pose" and i + 1 < len(args):
            name, xzh = args[i + 1].split("=", 1)
            POSES[name] = tuple(float(v) for v in xzh.split(","))
    scenarios = [s for s in opt(args, "--scenarios", "tour,town,mountain,pass,event").split(",") if s]
    manifest = os.path.abspath(opt(args, "--manifest", os.path.join(ROOT, "Cargo.toml")))
    engine = engine_dir(manifest)
    out = os.path.abspath(opt(args, "--out", os.path.join("/tmp", f"pacenote-budget-{quality.lower()}")))
    os.makedirs(os.path.join(out, "data"), exist_ok=True)
    for old in glob.glob(os.path.join(out, "data", "logs", "session-*.jsonl")):
        os.remove(old)
    with open(os.path.join(out, "data", "settings.json"), "w") as f:
        json.dump({"core": {"quality": quality}}, f)
    steps = [{"sleep": wait}] + steps_for(scenarios, secs, samples, int(opt(args, "--event-id", "103")), opt(args, "--event-race", "pro"))
    with open(os.path.join(out, "steps.json"), "w") as f:
        json.dump(steps, f)
    cmd = ["python3", "-I", "scripts/play.py", os.path.join(out, "steps.json"), "--manifest", manifest, "--bin", "pacenote", "--out", out]
    if "--features" in args:
        cmd += ["--features", opt(args, "--features", "")]
    if "--no-build" in args:
        cmd.append("--no-build")
    cmd += ["--", "--open"]
    env = dict(os.environ, SMTP_USER="", SMTP_PASS="")
    run = subprocess.run(cmd, cwd=engine, capture_output=True, text=True, env=env)
    # play.py prints one reply per command/shot step and cuts long ones at 2000 chars: only the
    # short ones (render.stats, events.start) are read here.
    replies = []
    for line in run.stdout.splitlines():
        if line.startswith("{"):
            try:
                replies.append(json.loads(line))
            except json.JSONDecodeError:
                replies.append({"truncated": True})
    logs = sorted(glob.glob(os.path.join(out, "data", "logs", "session-*.jsonl")), key=os.path.getmtime)
    if not logs:
        print(run.stdout[-2000:], run.stderr[-2000:])
        sys.exit("no session log: did the run fail? see " + out)
    rows = []
    for l in open(logs[-1], encoding="utf-8"):
        if l.strip().startswith("{"):
            try:
                rows.append(json.loads(l))
            except json.JSONDecodeError:
                pass
    markers = [r for r in rows if r.get("kind") == "perf.report"]
    perf = [r for r in rows if r.get("kind") == "perf"]
    if len(markers) < 2 * len(scenarios):
        print(run.stdout[-2000:], run.stderr[-2000:])
        sys.exit(f"{len(markers)} perf.report markers for {len(scenarios)} scenarios: did the run stop early? see {out}")
    draws = [r["result"]["draws"] for r in replies if isinstance(r.get("result"), dict) and "draws" in r["result"]]
    events = [r.get("result") or r.get("error") for r in replies if (isinstance(r.get("result"), dict) and "started" in r["result"]) or "events" in str(r.get("detail", {}).get("id", ""))]

    log = open(os.path.join(out, "game.log"), encoding="utf-8", errors="replace").read()
    load = re.findall(r"ready in ([0-9.]+) s", log)
    ready_ts = None
    m = re.search(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?)\S*\s[^\n]*ready in [0-9.]+ s", log)
    if m:
        ready_ts = datetime.fromisoformat(m.group(1)[:26] + "+00:00").timestamp() * 1000
    level = re.search(r"open world level: (\S+)", log)

    results = {"quality": quality, "secs": secs, "samples": samples, "load_s": float(load[-1]) if load else None, "level": level.group(1) if level else None, "scenarios": {}}
    for i, name in enumerate(scenarios):
        a, b = markers[2 * i], markers[2 * i + 1]
        win = [r for r in perf if a["t"] < r["t"] <= b["t"]]
        sc = {
            "seconds": round(b["t"] - a["t"], 1),
            "frame_ms_p50": summary([r["frame_ms"].get("p50") for r in win if "frame_ms" in r]),
            "frame_ms_p95": summary([r["frame_ms"].get("p95") for r in win if "frame_ms" in r]),
            "gpu_ms": summary([r.get("gpu", {}).get("frame_ms", {}).get("now") or None for r in win]),
            "footprint_mb": summary([r.get("ram", {}).get("footprint_mb", {}).get("now") for r in win]),
            "draws": summary(draws[i * samples:(i + 1) * samples]),
            "before_ready": ready_ts is not None and a.get("ts", 0) < ready_ts,
        }
        if name == "event":
            sc["event"] = events[-1] if events else None
        results["scenarios"][name] = sc
    foot = [r.get("ram", {}).get("footprint_mb", {}).get("max") for r in perf]
    results["footprint_max_mb"] = max([v for v in foot if v is not None], default=None)
    with open(os.path.join(out, "budget.json"), "w") as f:
        json.dump(results, f, indent=1)
    if "--json" in args:
        print(json.dumps(results, indent=1))

    col = 0 if quality.lower() == "low" else 1
    over = 0

    def check(label, v, key):
        nonlocal over
        ok = v is not None and v <= BUDGETS[key][col]
        over += not ok
        return f"{'ok' if ok else 'OVER'} {label}"

    def num(s, k):
        return "-" if not s else f"{s[k]:.1f}"

    print(f"budget check, quality {quality}, {len(scenarios)} scenarios x {secs:.0f} s, {samples} draw samples each, {out}")
    print(f"  level {results['level']}")
    for label, v, key in (("load_s", results["load_s"], "load_s"), ("footprint_mb (max)", results["footprint_max_mb"], "footprint_mb")):
        print(f"  {check(label, v, key):24} {('-' if v is None else f'{v:.1f}'):>8}  budget {BUDGETS[key][col]:g}")
    print(f"  {'scenario':9} {'frame ms med/p95':>17} {'gpu ms med/p95':>15} {'draws med/p95 (min-max)':>25} {'mem MB':>7}  checks")
    for name, sc in results["scenarios"].items():
        fr, g, d, mem = sc["frame_ms_p95"], sc["gpu_ms"], sc["draws"], sc["footprint_mb"]
        frame = f"{num(sc['frame_ms_p50'], 'median')}/{num(fr, 'p95')}"
        gpu = f"{num(g, 'median')}/{num(g, 'p95')}"
        dr = "-" if not d else f"{d['median']:.0f}/{d['p95']:.0f} ({d['min']:.0f}-{d['max']:.0f})"
        checks = [check("frame", fr and fr["p95"], "frame_ms_p95"), check("gpu", g and g["p95"], "gpu_ms_p95"), check("draws", d and d["p95"], "draws_p95")]
        flag = "  ! window started before the world was ready (raise --wait)" if sc["before_ready"] else ""
        if name == "event" and not (isinstance(sc.get("event"), dict) and sc["event"].get("started")):
            flag += f"  ! event did not start: {sc.get('event')}"
        print(f"  {name:9} {frame:>17} {gpu:>15} {dr:>25} {num(mem, 'median'):>7}  {', '.join(checks)}{flag}")
    print(f"  budgets ({'Low' if col == 0 else 'Medium/High'}): frame p95 {BUDGETS['frame_ms_p95'][col]} ms, gpu p95 {BUDGETS['gpu_ms_p95'][col]} ms, draws p95 {BUDGETS['draws_p95'][col]:g}")
    sys.exit(1 if over else 0)


if __name__ == "__main__":
    main()
