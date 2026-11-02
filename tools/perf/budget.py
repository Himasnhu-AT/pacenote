#!/usr/bin/env python3
"""The Low budget check (docs/ASSETS_BRIEF.md §1): a 60 s autopilot drive through town and onto
the ring at Low, measured, compared with the budgets. Exit 1 if any is over.

  python3 -I tools/perf/budget.py [--quality Low|Medium|High] [--secs 60] [--out DIR] [--no-build]

Runs the dev build through engine/scripts/play.py (offscreen, governor off as in all scripted
runs, so the quality stays where it's set). On the M5 Pro the numbers are optimistic for a base
M1 (brief §1): treat a pass here as necessary, not sufficient.
Measured: load time to control (game log), memory footprint (max), GPU ms per frame (p95),
frame time (95th percentile of the drive's per-second p95s, from the session log), visible meshes (a draw
call proxy before batching). Not measured yet: resident triangles, draw calls after batching.
"""
import glob, json, os, re, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ENGINE = os.path.join(os.path.dirname(ROOT), "engine")
BUDGETS = {  # brief §1: (Low, High)
    "load_s": (8.0, 5.0),
    "footprint_mb": (1500.0, 2500.0),
    "gpu_ms_p95": (10.0, 12.0),
    "frame_ms_p95": (16.6, 16.6),
    "meshes_visible": (800.0, 2500.0),
}


def opt(args, name, default):
    return args[args.index(name) + 1] if name in args else default


def main():
    args = sys.argv[1:]
    quality = opt(args, "--quality", "Low")
    secs = float(opt(args, "--secs", "60"))
    out = os.path.abspath(opt(args, "--out", os.path.join("/tmp", f"pacenote-budget-{quality.lower()}")))
    os.makedirs(os.path.join(out, "data"), exist_ok=True)
    with open(os.path.join(out, "data", "settings.json"), "w") as f:
        json.dump({"core": {"quality": quality}}, f)
    steps = [
        {"sleep": 9},
        {"cmd": "open.tour"},
        {"cmd": "autopilot.set", "params": {"on": True, "pace": 0.6}},
        {"sleep": 2},
        {"cmd": "perf.report"},
        {"sleep": secs},
        {"shot": "end"},
        {"cmd": "render.stats"},
        {"cmd": "perf.report"},
    ]
    with open(os.path.join(out, "steps.json"), "w") as f:
        json.dump(steps, f)
    cmd = ["python3", "-I", "scripts/play.py", os.path.join(out, "steps.json"), "--manifest", os.path.join(ROOT, "Cargo.toml"), "--bin", "pacenote", "--out", out]
    if "--no-build" in args:
        cmd.append("--no-build")
    cmd += ["--", "--open"]
    env = dict(os.environ, SMTP_USER="", SMTP_PASS="")
    run = subprocess.run(cmd, cwd=ENGINE, capture_output=True, text=True, env=env)
    lines = [l for l in run.stdout.splitlines() if l.startswith("{")]
    results = [json.loads(l).get("result", {}) for l in lines]
    reports = [r for r in results if isinstance(r, dict) and "metrics" in r]
    stats = [r for r in results if isinstance(r, dict) and "meshes_visible" in r]
    if not reports or not stats:
        print(run.stdout[-2000:], run.stderr[-2000:])
        sys.exit("no perf report: did the run fail? see " + out)
    m = reports[-1]["metrics"]
    log = open(os.path.join(out, "game.log"), encoding="utf-8", errors="replace").read()
    load = re.findall(r"ready in ([0-9.]+) s", log)
    # Frame time over the drive only: perf.report's window starts at launch (loading frames
    # would dominate), so take the last `secs` per-second samples from the session log.
    logs = sorted(glob.glob(os.path.join(out, "data", "logs", "session-*.jsonl")), key=os.path.getmtime)
    drive = []
    if logs:
        rows = [json.loads(l) for l in open(logs[-1], encoding="utf-8") if l.strip().startswith("{")]
        drive = [r["frame_ms"]["p95"] for r in rows if r.get("kind") == "perf" and "frame_ms" in r][-int(secs):]
    frame_p95 = sorted(drive)[int(0.95 * (len(drive) - 1))] if drive else m.get("frame_p95_ms", {}).get("p95")
    got = {
        "load_s": float(load[-1]) if load else None,
        "footprint_mb": m.get("footprint_mb", {}).get("max"),
        "gpu_ms_p95": m.get("gpu_frame_ms", {}).get("p95"),
        "frame_ms_p95": frame_p95,
        "meshes_visible": stats[-1].get("meshes_visible"),
    }
    col = 0 if quality.lower() == "low" else 1
    over = 0
    print(f"budget check, quality {quality}, {secs:.0f} s tour (town -> east road -> ring), {out}")
    for k, v in got.items():
        b = BUDGETS[k][col]
        ok = v is not None and v <= b
        over += not ok
        print(f"  {'ok  ' if ok else 'OVER'} {k:15} {('-' if v is None else f'{v:.1f}'):>9}  budget {b:g}")
    print(f"  entities {stats[-1].get('entities')}, meshes total {stats[-1].get('meshes_total')}")
    sys.exit(1 if over else 0)


if __name__ == "__main__":
    main()
