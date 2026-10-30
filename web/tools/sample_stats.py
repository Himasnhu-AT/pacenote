#!/usr/bin/env python3
"""Write a SAMPLE src/data/stats.json for layout work (made-up numbers, "sample": true).

The real file comes from tools/telemetry/analyze.py in the game repo; that overwrites this one.
The numbers here are invented but shaped by the real stages (tighter, slipperier, longer stages
finish less often), and every corner index points at a real corner in stages.json, so the
"deadliest corners" maps draw on the right place.

  python3 -I tools/sample_stats.py            # from web/
"""
import json, os, random

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "src", "data")
rng = random.Random(14)  # fixed: the sample doesn't change between runs

stages = json.load(open(os.path.join(DATA, "stages.json")))["stages"]
slip = {"Tarmac": 0.0, "Gravel": 0.06, "Mud": 0.12, "Snow": 0.15}

rows, worst = [], []
tot_runs = tot_fin = tot_ret = tot_crash = 0
km = 0.0
for st in stages:
    corners = st["corners"]
    tight = sum(1 for c in corners if c["grade"] <= 2)
    runs = int(rng.uniform(380, 900) * (1.6 if st["id"] <= 3 else 1.0))  # the demo's first stages get more runs
    risk = 0.05 + 0.025 * tight + slip[st["surface"]] + st["length_m"] / 60000
    retire = min(0.45, max(0.03, risk * rng.uniform(0.7, 1.1)))
    quit_ = rng.uniform(0.06, 0.14)
    finish = max(0.2, 1 - retire - quit_)
    fins = round(runs * finish)
    rets = round(runs * retire)
    # stage pace: ~19 m/s on tarmac, slower on loose surfaces and tight roads
    pace = 19.5 - slip[st["surface"]] * 25 - tight * 0.25
    best = st["length_m"] / (pace * 1.18)
    median = best * rng.uniform(1.22, 1.38)
    crashes = int(runs * (0.9 + risk * 4) * rng.uniform(0.8, 1.2))
    # weight crashes to tight corners
    weights = [(7 - c["grade"]) ** 2 * (1.4 if c["narrows"] else 1.0) * (1.3 if c["crest"] else 1.0) for c in corners]
    picks = rng.choices(range(len(corners)), weights=weights, k=crashes)
    per = {}
    for i in picks:
        per[i] = per.get(i, 0) + 1
    top = max(per, key=per.get) if per else None
    corner_rows = []
    for i, n in per.items():
        c = corners[i]
        rt = int(n * (0.18 if c["grade"] <= 2 else 0.07) * rng.uniform(0.6, 1.3))
        corner_rows.append({"stage": st["id"], "stage_name": st["name"], "index": i, "turn": c["turn"], "grade": c["grade"],
                            "start_m": round(c["start"]), "crashes": n, "retirements": rt})
    worst += corner_rows
    tc = next((r for r in corner_rows if r["index"] == top), None)
    rows.append({
        "id": st["id"], "name": st["name"], "runs": runs,
        "finish_rate": round(finish, 3), "retire_rate": round(retire, 3),
        "median_time_s": round(median, 1), "best_time_s": round(best, 1), "crashes": crashes,
        "top_corner": None if tc is None else {k: tc[k] for k in ("index", "turn", "grade", "start_m", "crashes", "retirements")},
    })
    tot_runs += runs; tot_fin += fins; tot_ret += rets; tot_crash += crashes
    km += (fins * st["length_m"] + (runs - fins) * st["length_m"] * 0.45) / 1000

worst.sort(key=lambda r: (-r["retirements"], -r["crashes"]))
out = {
    "generated_at": "2026-10-30T12:00:00Z",
    "sample": True,
    "totals": {"installs": 2140, "runs": tot_runs, "finishes": tot_fin, "retirements": tot_ret,
               "km_driven": round(km), "crashes": tot_crash},
    "stages": rows,
    "corners_worst": worst[:10],
    "codriver": {"on_finish_rate": 0.71, "off_finish_rate": 0.58},
}
with open(os.path.join(DATA, "stats.json"), "w") as f:
    json.dump(out, f, indent=1)
print(f"wrote sample stats: {tot_runs} runs, worst corner stage {worst[0]['stage']} #{worst[0]['index']}")
