#!/usr/bin/env python3
"""Pacenote telemetry analysis: run events -> stats.json (website), a Reddit-ready report and
charts (SVG + PNG). See docs/TELEMETRY.md for the events and the stats.json schema.

  python3 -I tools/telemetry/analyze.py --jsonl export.jsonl [--jsonl more.jsonl] --out DIR
  python3 -I tools/telemetry/analyze.py --sqlite tracer.db [--app pacenote] --out DIR
  python3 -I tools/telemetry/analyze.py --simulate 400 --out DIR      # made-up data, labelled

Options: --stages web/src/data/stages.json (road geometry), --stats PATH (also write stats.json
there, e.g. web/src/data/stats.json), --since YYYY-MM-DD, --demo-only, --min-runs N (default 25:
stages with fewer runs are listed but not ranked), --no-png.

Standard library only; PNGs need `rsvg-convert` (librsvg) on PATH.
"""
import argparse, json, math, os, random, shutil, sqlite3, statistics, subprocess, sys
from collections import Counter, defaultdict
from datetime import datetime, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))

# Palette: validated (dataviz validate_palette.js, dark, surface #121417): amber + blue.
SURFACE = "#121417"
INK, INK_2, MUTED = "#ecebe7", "#b8bec6", "#7d858f"
GRID, ROAD = "#262b31", "#3b424b"
AMBER, BLUE = "#b97f0c", "#5f8fd6"
FONT = "Inter, 'Helvetica Neue', Arial, sans-serif"


# ------------------------------------------------------------------ input

def load_jsonl(paths):
    out = []
    for p in paths:
        with open(p, encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                e = json.loads(line)
                props = e.get("props") or {}
                if isinstance(props, str):
                    props = json.loads(props)
                out.append({"install": e["install_id"], "session": e.get("session_id", ""), "ts": int(e["ts"]),
                            "version": e.get("version", ""), "name": e["name"], "props": props})
    return out


def load_sqlite(path, app):
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    rows = db.execute(
        "SELECT e.install_id, e.session_id, e.ts, e.version, e.name, e.props FROM events e "
        "JOIN apps a ON a.id = e.app_id WHERE a.slug = ? AND e.category = 'game' ORDER BY e.ts", (app,))
    return [{"install": i, "session": s, "ts": ts, "version": v, "name": n, "props": json.loads(p) if p else {}}
            for i, s, ts, v, n, p in rows]


def dedup(events):
    seen, out = set(), []
    for e in sorted(events, key=lambda e: (e["install"], e["session"], e["ts"])):
        k = (e["install"], e["session"], e["ts"], e["name"], json.dumps(e["props"], sort_keys=True))
        if k not in seen:
            seen.add(k)
            out.append(e)
    return out


def simulate(n_installs, stages, seed=7):
    """Made-up runs for layout and testing: crash odds rise with corner tightness."""
    rnd = random.Random(seed)
    by_id = {s["id"]: s for s in stages}
    demo = [1, 6, 9]
    t0 = int(datetime(2026, 11, 1, tzinfo=timezone.utc).timestamp() * 1000)
    ev = []
    for k in range(n_installs):
        inst = f"sim-{k:05d}"
        skill = rnd.betavariate(2.2, 2.0)
        codriver = rnd.random() < 0.8
        ts = t0 + rnd.randrange(0, 14 * 86_400_000)
        for r in range(rnd.choice([1, 2, 3, 4, 6, 9])):
            sid = rnd.choice(demo) if rnd.random() < 0.85 else rnd.randrange(1, 21)
            st = by_id[sid]
            ts += rnd.randrange(60_000, 900_000)
            sess = f"{inst}-{r // 3}"
            props = {"stage": sid, "mode": "solo", "codriver": codriver, "camera": "cockpit", "demo": True}
            ev.append({"install": inst, "session": sess, "ts": ts, "version": "0.1.0", "name": "run.start", "props": props})
            dmg, hits, recovers, pen, result, prog = 0.0, 0, 0, 0, "finish", st["length_m"]
            for ci, c in enumerate(st["corners"]):
                p = 0.012 * (7 - c["grade"]) ** 1.6 * (1.25 - skill) * (0.75 if codriver else 1.15) * (1.3 if c["crest"] else 1.0)
                if rnd.random() < p:
                    sev = min(1.3, rnd.paretovariate(1.8) * 0.1 * (1.6 if c["grade"] <= 2 else 1.0))
                    hits += 1
                    dmg += sev
                    retired = dmg >= 1.0
                    ev.append({"install": inst, "session": sess, "ts": ts + ci * 4000, "version": "0.1.0", "name": "run.crash",
                               "props": {"stage": sid, "s_m": int(c["start"] + rnd.random() * (c["end"] - c["start"])), "corner": ci,
                                         "grade": c["grade"], "severity": round(sev, 3), "speed_kmh": int(15 + sev ** 0.5 * 64),
                                         "retired": retired}})
                    if retired:
                        result, prog = "retired", int(c["start"])
                        break
                    if sev > 0.15 and rnd.random() < 0.5:
                        recovers += 1
                        pen += 5
            if result == "finish" and rnd.random() < 0.08:
                result, prog = rnd.choice(["quit", "restart"]), int(st["length_m"] * rnd.random())
            speed = 16 + 12 * skill + rnd.gauss(0, 1.5)
            time_ms = int((prog / speed + pen + hits * 2.5) * 1000)
            ev.append({"install": inst, "session": sess, "ts": ts + 300_000, "version": "0.1.0", "name": "run.end",
                       "props": {"stage": sid, "result": result, "time_ms": time_ms, "penalty_s": pen, "progress_m": prog,
                                 "hits": hits, "engine_pct": int(min(dmg, 1) * 60), "steering_pct": int(min(dmg, 1) * 40),
                                 "suspension_pct": int(min(dmg, 1) * 30), "recovers": recovers, "codriver": codriver, "mode": "solo", "demo": True}})
        if rnd.random() < 0.25:
            ev.append({"install": inst, "session": f"{inst}-0", "ts": ts + 1, "version": "0.1.0", "name": "demo.wishlist", "props": {"where": "end"}})
    return ev


# ------------------------------------------------------------------ analysis

def analyze(events, stages, min_runs):
    by_id = {s["id"]: s for s in stages}
    ends = [e for e in events if e["name"] == "run.end"]
    # A restart straight off the line isn't a run.
    runs = [e for e in ends if not (e["props"].get("result") in ("restart", "quit") and e["props"].get("progress_m", 0) < 50)]
    crashes = [e for e in events if e["name"] == "run.crash"]
    installs = {e["install"] for e in events}
    per = defaultdict(list)
    for e in runs:
        per[e["props"]["stage"]].append(e["props"])
    crash_per = defaultdict(list)
    for e in crashes:
        crash_per[e["props"]["stage"]].append(e["props"])

    def corner_rows(sid):
        st = by_id.get(sid)
        if not st:
            return []
        c = Counter(p["corner"] for p in crash_per[sid] if p.get("corner", -1) >= 0)
        r = Counter(p["corner"] for p in crash_per[sid] if p.get("corner", -1) >= 0 and p.get("retired"))
        rows = []
        for k, n in c.items():
            if k < len(st["corners"]):
                cc = st["corners"][k]
                rows.append({"index": k, "turn": cc["turn"], "grade": cc["grade"], "start_m": round(cc["start"]), "crashes": n, "retirements": r[k]})
        rows.sort(key=lambda x: (-x["crashes"], -x["retirements"], x["index"]))
        return rows

    stage_rows, worst = [], []
    for st in stages:
        sid = st["id"]
        rs = per.get(sid, [])
        n = len(rs)
        fin = [p for p in rs if p["result"] == "finish"]
        times = sorted(p["time_ms"] / 1000 for p in fin)
        corners = corner_rows(sid)
        stage_rows.append({
            "id": sid, "name": st["name"], "runs": n,
            "finish_rate": round(len(fin) / n, 3) if n else 0.0,
            "retire_rate": round(sum(p["result"] == "retired" for p in rs) / n, 3) if n else 0.0,
            "median_time_s": round(statistics.median(times), 1) if times else None,
            "best_time_s": round(times[0], 1) if times else None,
            "crashes": len(crash_per.get(sid, [])),
            "top_corner": corners[0] if corners else None,
            "ranked": n >= min_runs,
        })
        if n >= min_runs:
            for c in corners[:3]:
                worst.append({"stage": sid, "stage_name": st["name"], **c, "runs": n})
    worst.sort(key=lambda x: (-x["retirements"] / x["runs"], -x["crashes"] / x["runs"]))
    for w in worst:
        w.pop("runs")

    def rate(sel):
        return round(sum(p["result"] == "finish" for p in sel) / len(sel), 3) if sel else 0.0
    allp = [e["props"] for e in runs]
    on = [p for p in allp if p.get("codriver")]
    off = [p for p in allp if not p.get("codriver")]
    km = sum(p.get("progress_m", 0) for p in allp) / 1000
    totals = {"installs": len(installs), "runs": len(runs), "finishes": sum(p["result"] == "finish" for p in allp),
              "retirements": sum(p["result"] == "retired" for p in allp), "km_driven": round(km, 1), "crashes": len(crashes)}
    extra = {
        "codriver_runs": [len(on), len(off)],
        "recovers": sum(p.get("recovers", 0) for p in allp),
        "wishlist_clicks": sum(e["name"] == "demo.wishlist" for e in events),
        "grade_crash_share": dict(sorted(Counter(p.get("grade", 0) for e in crashes for p in [e["props"]] if p.get("corner", -1) >= 0).items())),
        "first_ts": min((e["ts"] for e in events), default=None), "last_ts": max((e["ts"] for e in events), default=None),
    }
    return {
        "totals": totals, "stages": stage_rows, "corners_worst": worst[:10],
        "codriver": {"on_finish_rate": rate(on), "off_finish_rate": rate(off)},
    }, extra, crash_per


# ------------------------------------------------------------------ charts

def esc(s):
    return str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def svg_doc(w, h, body, title):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" role="img" aria-label="{esc(title)}">'
            f'<rect width="{w}" height="{h}" fill="{SURFACE}"/>{body}</svg>')


def text(x, y, s, size=16, fill=INK, anchor="start", weight=400):
    return f'<text x="{x}" y="{y}" font-family="{FONT}" font-size="{size}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}">{esc(s)}</text>'


def footer(w, h, note):
    return text(40, h - 24, note, 13, MUTED)


def chart_finish_rates(stats, sample):
    rows = [s for s in stats["stages"] if s["ranked"]] or [s for s in stats["stages"] if s["runs"] > 0]
    rest = sum(1 for s in stats["stages"] if s["runs"] > 0) - len(rows)
    rows.sort(key=lambda s: s["finish_rate"])
    w, bar, gap, top, left = 1200, 28, 12, 120, 260
    h = top + len(rows) * (bar + gap) + 100
    plot = w - left - 250
    b = [text(40, 52, "How many runs reach the finish, by stage", 28, INK, weight=700),
         text(40, 84, "Share of runs that finished (the rest retired, quit or restarted)", 16, INK_2)]
    for g in (0, 0.25, 0.5, 0.75, 1.0):
        x = left + plot * g
        base = top + len(rows) * (bar + gap)
        b.append(f'<line x1="{x}" y1="{top - 10}" x2="{x}" y2="{base}" stroke="{GRID}" stroke-width="1"/>')
        b.append(text(x, base + 22, f"{int(g * 100)}%", 13, MUTED, "middle"))
    for i, s in enumerate(rows):
        y = top + i * (bar + gap)
        bw = max(2, plot * s["finish_rate"])
        dim = not s["ranked"]
        b.append(text(left - 14, y + bar - 6, f'{s["id"]:02d}  {s["name"]}', 15, MUTED if dim else INK_2, "end"))
        b.append(f'<rect x="{left}" y="{y}" width="{bw:.1f}" height="{bar}" rx="4" fill="{AMBER}" opacity="{0.45 if dim else 1}"/>')
        label = f'{s["finish_rate"] * 100:.0f}%  ·  {s["runs"]} runs' + ("  (few runs)" if dim else "")
        b.append(text(left + bw + 10, y + bar - 6, label, 13, MUTED if dim else INK_2))
    more = f" {rest} stages with fewer runs not shown." if rest else ""
    b.append(footer(w, h, ("SAMPLE DATA, not real players. " if sample else "") + "Pacenote demo telemetry." + more))
    return svg_doc(w, h, "".join(b), "Finish rate by stage")


def chart_crash_map(st, crashes, stage_row, sample):
    pts = st["points"]
    xs, zs = [p[0] for p in pts], [p[1] for p in pts]
    minx, maxx, minz, maxz = min(xs), max(xs), min(zs), max(zs)
    w, h, pad, top = 1200, 900, 70, 140
    sc = min((w - 2 * pad) / max(maxx - minx, 1), (h - top - pad - 40) / max(maxz - minz, 1))
    ox = pad + ((w - 2 * pad) - (maxx - minx) * sc) / 2
    oz = top + ((h - top - pad - 40) - (maxz - minz) * sc) / 2

    def at(s_m):
        i = min(len(pts) - 1, max(0, int(round(s_m / st["step_m"]))))
        return ox + (pts[i][0] - minx) * sc, oz + (pts[i][1] - minz) * sc
    path = " ".join(f"{'M' if i == 0 else 'L'}{ox + (p[0] - minx) * sc:.1f},{oz + (p[1] - minz) * sc:.1f}" for i, p in enumerate(pts))
    b = [text(40, 52, f'Where cars crash on stage {st["id"]}, {st["name"]}', 28, INK, weight=700),
         text(40, 84, f'{len(crashes)} crashes in {stage_row["runs"]} runs. Bigger dot = more crashes; amber = run-ending.', 16, INK_2),
         f'<path d="{path}" fill="none" stroke="{ROAD}" stroke-width="7" stroke-linecap="round" stroke-linejoin="round"/>']
    sx, sz = at(0)
    ex, ez = at(st["length_m"])
    b.append(f'<circle cx="{sx:.1f}" cy="{sz:.1f}" r="7" fill="{INK}"/>' + text(sx + 12, sz + 5, "START", 13, INK_2))
    b.append(f'<rect x="{ex - 6:.1f}" y="{ez - 6:.1f}" width="12" height="12" fill="{INK}"/>' + text(ex + 12, ez + 5, "FINISH", 13, INK_2))
    bins = defaultdict(lambda: [0, 0])
    for c in crashes:
        k = int(c["s_m"] // 30)
        bins[k][0] += 1
        bins[k][1] += 1 if c.get("retired") else 0
    mx = max((v[0] for v in bins.values()), default=1)
    for k, (n, ret) in sorted(bins.items(), key=lambda kv: kv[1][0]):
        x, y = at(k * 30 + 15)
        r = 5 + 22 * math.sqrt(n / mx)
        fill = AMBER if ret else BLUE
        b.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r:.1f}" fill="{fill}" fill-opacity="0.85" stroke="{SURFACE}" stroke-width="2"/>')
    tc = stage_row.get("top_corner")
    if tc:
        x, y = at((tc["start_m"] + min(tc["start_m"] + 40, st["length_m"])) / 2)
        call = f'{tc["turn"]} {"hairpin" if tc["grade"] == 1 else tc["grade"]}'
        b.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="38" fill="none" stroke="{INK}" stroke-width="2.5" stroke-dasharray="6 5"/>')
        b.append(text(40, 110, f'Dashed ring: the corner with the most crashes, no. {tc["index"] + 1} ({call}, {tc["start_m"]} m in): '
                      f'{tc["crashes"]} crashes, {tc["retirements"]} ended the run.', 15, INK, weight=600))
    b.append(text(w - 40, 52, "●", 18, BLUE, "end") + text(w - 62, 52, "crash", 14, INK_2, "end"))
    b.append(text(w - 40, 76, "●", 18, AMBER, "end") + text(w - 62, 76, "ended the run", 14, INK_2, "end"))
    b.append(footer(w, h, ("SAMPLE DATA, not real players. " if sample else "") + "Pacenote demo telemetry · real stage geometry"))
    return svg_doc(w, h, "".join(b), f"Crash map, stage {st['id']}")


def chart_codriver(stats, extra, sample):
    w, h = 1200, 420
    on, off = stats["codriver"]["on_finish_rate"], stats["codriver"]["off_finish_rate"]
    n_on, n_off = extra["codriver_runs"]
    b = [text(40, 52, "Does the AI co-driver help?", 28, INK, weight=700),
         text(40, 84, "Share of runs that reach the finish, with the co-driver reading pace notes vs without", 16, INK_2)]
    for i, (label, v, n, col) in enumerate([("Co-driver on", on, n_on, AMBER), ("Co-driver off", off, n_off, BLUE)]):
        x = 40 + i * 580
        b.append(f'<rect x="{x}" y="120" width="540" height="230" rx="10" fill="#1b1f24"/>')
        b.append(f'<rect x="{x}" y="120" width="6" height="230" rx="3" fill="{col}"/>')
        b.append(text(x + 32, 168, label.upper(), 16, INK_2, weight=600))
        b.append(text(x + 32, 270, f"{v * 100:.0f}%", 88, INK, weight=700))
        b.append(text(x + 32, 320, f"finish  ·  {n} runs", 16, MUTED))
    b.append(footer(w, h, ("SAMPLE DATA, not real players. " if sample else "") + "Correlation, not proof: players choose whether to turn it off."))
    return svg_doc(w, h, "".join(b), "Co-driver on vs off")


# ------------------------------------------------------------------ report

def clock(s):
    m = int(s // 60)
    return f"{m}:{s - m * 60:04.1f}"


def report(stats, extra, sample, stages):
    t = stats["totals"]
    by_id = {s["id"]: s for s in stages}
    ranked = [s for s in stats["stages"] if s["ranked"]]
    L = []
    if sample:
        L += ["> **SAMPLE DATA.** These numbers are simulated for testing the pipeline. Do not post them.", ""]
    span = ""
    if extra["first_ts"]:
        a = datetime.fromtimestamp(extra["first_ts"] / 1000, timezone.utc).strftime("%-d %b")
        z = datetime.fromtimestamp(extra["last_ts"] / 1000, timezone.utc).strftime("%-d %b %Y")
        span = f"{a} – {z}"
    L += ["# Pacenote demo: what the telemetry says", "",
          f"_{span}. {t['installs']} players, {t['runs']} runs, {t['km_driven']:,.0f} km driven, {t['crashes']} crashes._", "",
          "## Post title ideas", ""]
    runs_of = {s["id"]: s["runs"] for s in stats["stages"]}
    worst = stats["corners_worst"]
    by_crash = max(worst, key=lambda c: c["crashes"] / runs_of[c["stage"]], default=None)
    by_ret = max(worst, key=lambda c: c["retirements"] / runs_of[c["stage"]], default=None)
    call = lambda c: f"{c['turn']} {'hairpin' if c['grade'] == 1 else c['grade']}"
    if by_ret and by_ret["retirements"] / runs_of[by_ret["stage"]] >= 0.05:
        share = by_ret["retirements"] / runs_of[by_ret["stage"]]
        L.append(f"- One {call(by_ret)} on {by_ret['stage_name']} ends {share * 100:.0f}% of all runs on that stage. Here's the crash map from our rally game's demo.")
    if by_crash:
        share = by_crash["crashes"] / runs_of[by_crash["stage"]]
        L.append(f"- {share * 100:.0f}% of drivers crash at the same {call(by_crash)} on {by_crash['stage_name']}. I mapped every crash in our rally demo.")
    if ranked:
        hard = min(ranked, key=lambda s: s["finish_rate"])
        L.append(f"- {t['runs']} demo runs in: {hard['name']} is the hardest stage, {hard['finish_rate'] * 100:.0f}% of runs reach the finish.")
    L.append(f"- Our players crashed {t['crashes']} times in {t['km_driven']:,.0f} km. Where it happens, and whether the AI co-driver helps.")
    L += ["", "## Headline numbers", "",
          f"- Runs finished: {t['finishes']} of {t['runs']} ({t['finishes'] / max(t['runs'], 1) * 100:.0f}%). Retired (car wrecked): {t['retirements']}.",
          f"- Recovers used (R, +5 s): {extra['recovers']}.",
          f"- Co-driver on: {stats['codriver']['on_finish_rate'] * 100:.0f}% finish ({extra['codriver_runs'][0]} runs); off: {stats['codriver']['off_finish_rate'] * 100:.0f}% ({extra['codriver_runs'][1]} runs). Correlation only.",
          f"- Wishlist clicks from inside the demo: {extra['wishlist_clicks']}.", "",
          "## Stages", "", "| Stage | Runs | Finish | Retired | Median time | Best | Worst corner |", "|---|---:|---:|---:|---:|---:|---|"]
    for s in sorted(stats["stages"], key=lambda s: -s["runs"]):
        if not s["runs"]:
            continue
        tc = s["top_corner"]
        corner = f"#{tc['index'] + 1} {tc['turn']} {'hairpin' if tc['grade'] == 1 else tc['grade']} ({tc['crashes']} crashes)" if tc else "—"
        mt = clock(s["median_time_s"]) if s["median_time_s"] else "—"
        bt = clock(s["best_time_s"]) if s["best_time_s"] else "—"
        flag = "" if s["ranked"] else " *"
        L.append(f"| {s['id']:02d} {s['name']}{flag} | {s['runs']} | {s['finish_rate'] * 100:.0f}% | {s['retire_rate'] * 100:.0f}% | {mt} | {bt} | {corner} |")
    L += ["", "\\* fewer runs than the ranking threshold: shown, not compared.", "",
          "## Deadliest corners (ranked stages)", "", "| Stage | Corner | Call | Crashes | Run-ending |", "|---|---|---|---:|---:|"]
    for c in stats["corners_worst"]:
        L.append(f"| {c['stage']:02d} {c['stage_name']} | #{c['index'] + 1} at {c['start_m']} m | {c['turn']} {'hairpin' if c['grade'] == 1 else c['grade']} | {c['crashes']} | {c['retirements']} |")
    if extra["grade_crash_share"]:
        L += ["", "## Crashes by corner grade (1 = hairpin … 6 = flat out)", ""]
        tot = sum(extra["grade_crash_share"].values())
        for g, n in extra["grade_crash_share"].items():
            L.append(f"- Grade {g}: {n} ({n / tot * 100:.0f}%)")
    L += ["", "## Charts", "", "- `finish_rates.png`, `codriver.png`, `crashmap_stageNN.png` (this folder).", "",
          "## How the numbers are made", "",
          "Anonymous, opt-out telemetry from the demo: one event when a run starts and ends, one per crash (where on the road, how hard). "
          "No names, voice or chat. Players can turn it off in Settings > Game. A restart within the first 50 m isn't counted as a run. "
          "Stage maps are the game's real road geometry.", "",
          "## Before posting", "",
          "- Check the subreddit's self-promotion rules (r/rallygames, r/simracing, r/IndieDev, r/gamedev's Screenshot Saturday, r/dataisbeautiful needs [OC] and the data source/tool in a comment).",
          "- Lead with the chart, not the store link; put the Steam link in a comment if the sub asks for that.",
          "- Say how many runs the numbers come from, and that it's the demo."]
    return "\n".join(L) + "\n"


# ------------------------------------------------------------------ main

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--jsonl", action="append", default=[])
    ap.add_argument("--sqlite")
    ap.add_argument("--app", default="pacenote")
    ap.add_argument("--simulate", type=int)
    ap.add_argument("--stages", default=os.path.join(ROOT, "web", "src", "data", "stages.json"))
    ap.add_argument("--out", required=True)
    ap.add_argument("--stats")
    ap.add_argument("--since")
    ap.add_argument("--demo-only", action="store_true")
    ap.add_argument("--min-runs", type=int, default=25)
    ap.add_argument("--no-png", action="store_true")
    a = ap.parse_args()

    stages = json.load(open(a.stages, encoding="utf-8"))["stages"]
    sample = a.simulate is not None
    if sample:
        events = simulate(a.simulate, stages)
    else:
        events = load_jsonl(a.jsonl) if a.jsonl else []
        if a.sqlite:
            events += load_sqlite(a.sqlite, a.app)
        if not a.jsonl and not a.sqlite:
            sys.exit("give --jsonl, --sqlite or --simulate")
    events = dedup(events)
    if a.since:
        cut = int(datetime.fromisoformat(a.since).replace(tzinfo=timezone.utc).timestamp() * 1000)
        events = [e for e in events if e["ts"] >= cut]
    if a.demo_only:
        events = [e for e in events if e["name"] not in ("run.start", "run.end") or e["props"].get("demo")]
    stats, extra, crash_per = analyze(events, stages, a.min_runs)
    stats = {"generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "sample": sample, **stats}

    os.makedirs(a.out, exist_ok=True)
    out_stats = json.dumps(stats, indent=2) + "\n"
    open(os.path.join(a.out, "stats.json"), "w").write(out_stats)
    if a.stats:
        open(a.stats, "w").write(out_stats)
    open(os.path.join(a.out, "report.md"), "w").write(report(stats, extra, sample, stages))
    charts = {"finish_rates": chart_finish_rates(stats, sample), "codriver": chart_codriver(stats, extra, sample)}
    by_id = {s["id"]: s for s in stages}
    top = sorted([s for s in stats["stages"] if s["crashes"]], key=lambda s: -s["runs"])[:3]
    for s in top:
        charts[f"crashmap_stage{s['id']:02d}"] = chart_crash_map(by_id[s["id"]], crash_per[s["id"]], s, sample)
    rsvg = None if a.no_png else shutil.which("rsvg-convert")
    for name, svg in charts.items():
        p = os.path.join(a.out, f"{name}.svg")
        open(p, "w").write(svg)
        if rsvg:
            subprocess.run([rsvg, "-o", os.path.join(a.out, f"{name}.png"), p], check=True)
    print(f"{len(events)} events, {stats['totals']['runs']} runs, {stats['totals']['installs']} installs -> {a.out}"
          + (" (SAMPLE)" if sample else "") + ("" if rsvg or a.no_png else " (no rsvg-convert: SVG only)"))


if __name__ == "__main__":
    main()
