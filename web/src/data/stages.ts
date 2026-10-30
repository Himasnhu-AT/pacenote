// Typed access to the game's stage data (build time only: never import this from a client script,
// the JSON is ~450 KB). Regenerate: `memgate -- cargo run -q --example stage_dump > web/src/data/stages.json`.
import raw from './stages.json';
import rawStats from './stats.json';
import { slug, type Stage } from '../lib/stage';

export const stages = (raw as { stages: Stage[] }).stages.slice().sort((a, b) => a.id - b.id);
export const stageBySlug = (s: string) => stages.find((x) => slug(x) === s);

export type StageStats = {
  id: number; name: string; runs: number; finish_rate: number; retire_rate: number;
  median_time_s: number | null; best_time_s: number | null; crashes: number;
  top_corner: { index: number; turn: string; grade: number; start_m: number; crashes: number; retirements: number } | null;
  ranked?: boolean; // analyze.py: false when the stage has fewer runs than --min-runs
};
export type WorstCorner = { stage: number; stage_name: string; index: number; turn: string; grade: number; start_m: number; crashes: number; retirements: number };
export type Stats = {
  generated_at: string;
  sample: boolean;
  totals: { installs: number; runs: number; finishes: number; retirements: number; km_driven: number; crashes: number };
  stages: StageStats[];
  corners_worst: WorstCorner[];
  codriver: { on_finish_rate: number | null; off_finish_rate: number | null };
};
export const stats = rawStats as Stats;
export const statsFor = (id: number) => stats.stages.find((s) => s.id === id);

/** Co-driver comparison, or null when either side has no data yet. */
export const codriverRates = () => {
  const { on_finish_rate: on, off_finish_rate: off } = stats.codriver ?? {};
  return typeof on === 'number' && typeof off === 'number' ? { on, off } : null;
};
