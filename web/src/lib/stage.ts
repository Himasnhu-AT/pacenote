// Stage geometry and pace notes, shared by the build (SVG) and the browser (the live call sheet).
//
// The data is the game's own: src/data/stages.json is written by `examples/stage_dump.rs`
// (a point every 10 m as [x, z, y], corners with grades 1 to 6, crests). The notes and their
// wording are a port of the game's co-driver (src/codriver/calls.rs, src/guide/mod.rs
// `upcoming_in`), so the site reads a stage the way the game does.

export type Turn = 'left' | 'right';
export type Corner = { start: number; end: number; turn: Turn; grade: number; angle: number; crest: boolean; narrows: boolean };
export type Stage = {
  id: number;
  name: string;
  biome: string;
  surface: string;
  weather: string;
  time: string;
  length_m: number;
  step_m: number;
  points: [number, number, number][];
  corners: Corner[];
  crests: number[];
};

/** A pace note: a corner or a lone crest. */
export type Note = { at: number; end: number; corner: number | null };
/** One breath of the co-driver: notes that follow within CHAIN_GAP are read together. */
export type Call = { at: number; end: number; text: string; notes: Note[]; grade: number | null };

export const CHAIN_GAP = 30; // calls.rs
export const LEAD_MIN = 70;
export const LEAD_MAX = 280;
export const LEAD_S = 4.5;

export const slug = (s: Pick<Stage, 'name'>) => s.name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '');

/** guide/mod.rs upcoming_in, from the start: corners and crests in road order. */
export function notes(st: Pick<Stage, 'corners' | 'crests'>): Note[] {
  const { corners, crests } = st;
  const out: Note[] = [];
  let ci = 0;
  let ki = 0;
  for (;;) {
    const c = corners[ci];
    // Crests that fall inside a corner are part of its call already.
    while (ki < crests.length) {
      const k = crests[ki];
      const near = corners.slice(Math.max(0, ci - 1), Math.max(0, ci - 1) + 3);
      if (near.some((cc) => k >= cc.start - 30 && k <= cc.end)) ki++;
      else break;
    }
    const k = crests[ki];
    if (c && k !== undefined && k < c.start) { ki++; out.push({ at: k, end: k, corner: null }); }
    else if (c) { ci++; out.push({ at: c.start, end: c.end, corner: ci - 1 }); }
    else if (k !== undefined) { ki++; out.push({ at: k, end: k, corner: null }); }
    else break;
  }
  return out;
}

const GRADE_WORD = ['hairpin', 'hairpin', 'two', 'three', 'four', 'five', 'six'];
export const gradeWord = (g: number) => GRADE_WORD[Math.max(0, Math.min(6, g))];
export const gradeName = (g: number) => (g <= 1 ? 'Hairpin' : g >= 6 ? 'Six, flat' : `Grade ${g}`);

/** calls.rs Planner::phrase */
export function phrase(n: Note, corners: Corner[]): string {
  if (n.corner === null) return 'over crest';
  const c = corners[n.corner];
  let s = `${c.turn} ${gradeWord(c.grade)}`;
  if (c.end - c.start > 70) s += ' long';
  if (c.crest) s += ' over crest';
  if (c.narrows) s += ', narrows';
  if (c.grade <= 2) s += ', careful';
  return s;
}

/** Rounded like a co-driver: 10 m steps under 200, 50 above (calls.rs distance). */
export const distance = (d: number) => {
  const step = d < 200 ? 10 : 50;
  return String(Math.round(d / step) * step);
};

/** The whole stage as the co-driver chains it (no distances: those depend on where the car is). */
export function calls(st: Pick<Stage, 'corners' | 'crests'>): Call[] {
  const ns = notes(st);
  const out: Call[] = [];
  for (let i = 0; i < ns.length; ) {
    const first = ns[i];
    const group = [first];
    let last = first;
    i++;
    while (i < ns.length && ns[i].at - last.end <= CHAIN_GAP) { last = ns[i]; group.push(last); i++; }
    const text = group.map((n) => phrase(n, st.corners)).join(', into ');
    const grades = group.filter((n) => n.corner !== null).map((n) => st.corners[n.corner!].grade);
    out.push({ at: first.at, end: last.end, text, notes: group, grade: grades.length ? Math.min(...grades) : null });
  }
  return out;
}

/** How the co-driver would say it from `d` metres out ("150 left four long"). */
export const spoken = (c: Call, d: number) => (d > 40 ? `${distance(d)} ${c.text}` : c.text);

export const lead = (speed: number) => Math.max(LEAD_MIN, Math.min(LEAD_MAX, Math.abs(speed) * LEAD_S));

// ---------- Geometry ----------

export type Frame = { minX: number; minZ: number; w: number; h: number; pad: number };

/** Bounds of the road in plan view (x right, z down: the top-down view, not mirrored). */
export function frame(st: Pick<Stage, 'points'>, padFrac = 0.06): Frame {
  let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
  for (const [x, z] of st.points) {
    if (x < minX) minX = x; if (x > maxX) maxX = x;
    if (z < minZ) minZ = z; if (z > maxZ) maxZ = z;
  }
  const pad = Math.max(maxX - minX, maxZ - minZ) * padFrac;
  return { minX: minX - pad, minZ: minZ - pad, w: maxX - minX + pad * 2, h: maxZ - minZ + pad * 2, pad };
}

export const viewBox = (f: Frame) => `${r(f.minX)} ${r(f.minZ)} ${r(f.w)} ${r(f.h)}`;
const r = (v: number) => Math.round(v * 10) / 10;

/** SVG path for the road between s0 and s1 metres. */
export function roadPath(st: Pick<Stage, 'points' | 'step_m'>, s0 = 0, s1 = Infinity): string {
  const pts = st.points;
  const i0 = Math.max(0, Math.floor(s0 / st.step_m));
  const i1 = Math.min(pts.length - 1, Math.ceil(Math.min(s1, (pts.length - 1) * st.step_m) / st.step_m));
  let d = '';
  for (let i = i0; i <= i1; i++) d += `${i === i0 ? 'M' : 'L'}${r(pts[i][0])} ${r(pts[i][1])}`;
  return d;
}

/** Position and heading (radians, SVG rotation) at s metres along the road. */
export function at(st: Pick<Stage, 'points' | 'step_m'>, s: number) {
  const pts = st.points;
  const f = Math.max(0, Math.min(pts.length - 1.001, s / st.step_m));
  const i = Math.floor(f);
  const t = f - i;
  const a = pts[i], b = pts[i + 1];
  return {
    x: a[0] + (b[0] - a[0]) * t,
    z: a[1] + (b[1] - a[1]) * t,
    y: a[2] + (b[2] - a[2]) * t,
    heading: Math.atan2(b[1] - a[1], b[0] - a[0]),
  };
}

/** Elevation profile as an SVG path in a w×h box (y exaggerated to fill it). */
export function profile(st: Pick<Stage, 'points' | 'step_m'>, w: number, h: number, range?: [number, number]) {
  const ys = st.points.map((p) => p[2]);
  const lo = range ? range[0] : Math.min(...ys);
  const hi = range ? range[1] : Math.max(...ys);
  const span = Math.max(1, hi - lo);
  const n = st.points.length - 1;
  let line = '';
  st.points.forEach((p, i) => {
    line += `${i ? 'L' : 'M'}${r((i / n) * w)} ${r(h - ((p[2] - lo) / span) * h)}`;
  });
  return { line, area: `${line}L${w} ${h}L0 ${h}Z`, lo, hi };
}

export const surfaceName: Record<string, string> = { Gravel: 'Gravel', Tarmac: 'Tarmac', Mud: 'Mud', Snow: 'Snow' };
export const biomeName: Record<string, string> = {
  PineForest: 'Pine forest',
  BirchHills: 'Birch hills',
  Scrub: 'Scrub',
  Moor: 'Moor',
  SnowForest: 'Snow forest',
};
export const timeName: Record<string, string> = { Morning: 'Morning', Noon: 'Noon', Afternoon: 'Afternoon', Dusk: 'Dusk' };

export const km = (m: number) => (m / 1000).toFixed(1);
export const fmtTime = (s: number | null | undefined) => {
  if (s === null || s === undefined) return 'none yet';
  const m = Math.floor(s / 60);
  const rest = (s - m * 60).toFixed(1).padStart(4, '0');
  return `${m}:${rest}`;
};
/** CSS font-stretch for a grade: hairpin is tight, six is wide open. */
export const gradeWidth = (g: number | null) => (g === null ? 100 : Math.round(62 + ((Math.max(1, g) - 1) / 5) * 63));
