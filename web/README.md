# Pacenote website

Launch site for Pacenote (free Steam demo + wishlists). Astro (static), GSAP + Lenis, no
framework runtime, no trackers. Design record: `DESIGN.md`. All copy: `src/data/site.ts`.

```sh
pnpm install
pnpm build                 # static site in dist/
pnpm preview               # or any static server on dist/ (port 4410)
pnpm check                 # type check
```

Node 22+ and pnpm. Pages: `/` (home), `/stages/` and `/stages/<name>/` (20 stage pages),
`/stats/` (the numbers), `/privacy/`, `404`. `/og/` only exists to render the OG image.

## Data from the game

Everything on the maps and call sheets is the game's own data; the site rebuilds from these
files with no code changes.

- **Stages** `src/data/stages.json`: road geometry for all 20 stages (a point every 10 m,
  corners with grades, crests), from the game repo root:

  ```sh
  memgate -- cargo run -q --example stage_dump > web/src/data/stages.json
  ```

  Regenerate after any change to `src/stage.rs` (seeds, road growth, corner grading).
- **Pace notes** are not stored: `src/lib/stage.ts` ports the co-driver's wording and
  chaining from `src/codriver/calls.rs` and `src/guide/mod.rs` (`upcoming_in`). If the game's
  phrasing changes, update `phrase()` / `calls()` there.
- **Stats** `src/data/stats.json` (schema: `docs/TELEMETRY.md`). Right now it is a **sample**
  (`"sample": true`, made-up numbers from `tools/sample_stats.py`); every place that shows it
  says "Sample data" while that flag is true. For real numbers, from the game repo root:

  ```sh
  python3 -I tools/telemetry/analyze.py --jsonl export.jsonl --out /tmp/pacenote-report \
    --stages web/src/data/stages.json --stats web/src/data/stats.json
  cd web && pnpm build
  ```

  Tested with `analyze.py --simulate` output: unranked stages, an empty worst-corner list and
  missing co-driver rates all render.

## Screenshots

Every image is an in-game capture. Raw 1920×1080 PNGs live in `_source-shots/` (git-ignored);
`python3 -I tools/shots.py` turns them into the WebP masters in `src/assets/shots/`, and Astro
ships AVIF/WebP at responsive sizes. To capture new ones (from the engine repo):

```sh
cd /Users/himanshu/codes/game/engine
SMTP_USER= SMTP_PASS= python3 -I scripts/play.py steps.json \
  --manifest /Users/himanshu/codes/game/pacenote/Cargo.toml --bin pacenote --out <dir> -- --stage 9
```

with steps like `[{"sleep":11},{"cmd":"art.place","params":{"s":400}},{"sleep":3},{"shot":"cockpit"},
{"cmd":"input.key","params":{"key":"KeyC","action":"down"}},{"cmd":"input.key","params":{"key":"KeyC","action":"up"}},{"shot":"bonnet"}]`
(`KeyC` cycles cockpit → bonnet → chase). The guide screen needs two runs: the driver with
`-- --role driver --host 7791 --stage 1` in the background, then the guide with
`-- --role guide --connect 127.0.0.1:7791`. `--menu` captures the main menu. Name the PNG what
`src/data/site.ts` calls it, run `tools/shots.py`, rebuild.

## OG image and icons

`public/og.png` (1200×630) is a render of `/og/`. After changing the hero image, rebuild,
serve `dist/` on 4410 and run the awwwards skill's `scripts/og.mjs http://127.0.0.1:4410/og/`
(or screenshot `/og/` at 1200×630 any other way). Favicons are in `public/` (`favicon.svg`,
`favicon-32.png`, `apple-touch-icon.png`: the livery's orange and white slashes on navy).

## Placeholders to replace before launch

All in `src/data/site.ts` unless noted (each is marked `placeholder`):

| What | Where | Now |
|---|---|---|
| Steam wishlist link | `cta.wishlist.href` | Steam search for "Pacenote" (no app id yet) |
| Steam demo link | `cta.demo.href` | same |
| Production domain | `site.url`, `astro.config.mjs` `site`, `public/robots.txt` | `pacenote.marlstudio.com` |
| Studio name | `site.studio` | Marl Studio |
| Press / contact email | `contact.press` (also used on /privacy) | press@marlstudio.com |
| Discord | `contact.discord.href` (empty = hidden) | none |
| Full game release date, price | `release`, FAQ items marked `placeholder` | "to be announced" |
| Platforms | `release`, FAQ "Which platforms?" | Windows + macOS, Linux to be confirmed |
| Demo contents | FAQ "What does the demo include?" | "a selection of stages" |
| Stats | `src/data/stats.json` | sample (see above) |

## Deploy (Vercel; not done, the owner's call)

It is a plain static site, so any static host works. With Vercel:

1. Replace the placeholders above, at least the domain and the two Steam links.
2. `pnpm build` locally to check.
3. New Vercel project from this repo with **Root Directory** `web`, framework preset
   **Astro**, install `pnpm install`, build `pnpm build`, output `dist` (the preset fills these).
   Or from the CLI: `cd web && pnpm dlx vercel` (preview), then `pnpm dlx vercel --prod`.
4. Add the domain (e.g. `pacenote.marlstudio.com`) in Project → Settings → Domains and the
   CNAME it asks for at the DNS provider.
5. `trailingSlash: 'always'` is set; Vercel serves `dist/404.html` for unknown paths.
6. After deploy: check the OG card (e.g. paste the URL into a Discord or Reddit draft) and
   submit `https://<domain>/sitemap-index.xml` in Google Search Console.

When `stats.json` gets real numbers, commit it and redeploy; nothing else changes.

## Code map

| Path | What |
|---|---|
| `src/data/site.ts` | every visible string, links, placeholders, which captures go where |
| `src/data/stages.ts` | typed access to `stages.json` / `stats.json` (build time only) |
| `src/lib/stage.ts` | geometry (plan view, profile) and the pace-note port of `calls.rs` |
| `src/components/StageMap.astro` | a stage as SVG: road, corners by grade, rings, live layers |
| `src/components/LiveStage.astro` + `src/motion/recce.ts` | the signature: car + lit 500 m + call sheet, idle (hero) or scroll-driven (pinned section), speech on click |
| `src/motion/setup.ts` | loader (start countdown), page transition (orange slash), menu, bars |
| `src/engine/` | shared motion engine (same as Shroud's site): Lenis on the GSAP ticker, reveals, transitions |
| `tools/sample_stats.py`, `tools/shots.py` | sample stats, capture conversion |
