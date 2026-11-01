# Steam: free demo and store page

How Pacenote goes on Steam: a **Coming soon** store page for the full game (wishlists open), and a
**free demo** (its own Steam app) that shows on that page. Everything Valve-facing below was
checked against the Steamworks docs on 30 Oct 2026; Valve changes rules, so recheck the linked
pages before each step.

Contents: [Builds](#builds) · [Launch checklist](#launch-checklist) · [Store assets](#store-assets) ·
[Store page copy](#store-page-copy-draft) · [System requirements](#system-requirements) ·
[Day of launch](#day-of-launch) · [Shipped asset paths](#shipped-asset-paths) · [Risks](#risks-and-what-to-test-on-windows)

## Builds

| | Demo | Full game |
|---|---|---|
| Cargo features | `--no-default-features --features demo,telemetry` | `--no-default-features --features telemetry` |
| Build env | `PACENOTE_STEAM_URL` (full game's store page), `PACENOTE_TRACE_ENDPOINT`, `PACENOTE_TRACE_KEY` | `PACENOTE_TRACE_ENDPOINT`, `PACENOTE_TRACE_KEY` |
| Windows | `packaging/windows/build.sh` | `EDITION=full packaging/windows/build.sh` |
| macOS | `packaging/macos/build.sh` | `EDITION=full packaging/macos/build.sh` |
| Upload | `packaging/steam/upload.sh demo` | `packaging/steam/upload.sh full` |
| Output | `dist/steam/demo/{windows,macos}/` | `dist/steam/full/{windows,macos}/` |

Without `PACENOTE_STEAM_URL` the demo's wishlist buttons open a Steam search for "Pacenote". Set
it once the app id exists: `PACENOTE_STEAM_URL=https://store.steampowered.com/app/<FULL_APPID>/Pacenote/`.
The demo and the full game share one data folder (`%APPDATA%\pacenote`, `~/Library/Application
Support/pacenote`), so settings, times and ghosts from the demo carry over.

Both scripts:
- run cargo through `memgate --profile release` and use `CARGO_TARGET_DIR` if set, else
  `../pacenote-build/target-ship` (never the shared Shroud target dir);
- refuse to build if Cargo.toml lacks a feature they ask for (`ALLOW_MISSING_FEATURES=1` drops it
  for a test build);
- rewrite the build machine's paths out of the binary (`--remap-path-prefix`);
- copy `assets/` (cooked files only), `CREDITS.txt` and `THIRD-PARTY-NOTICES.html` (open-source
  licences, required by MIT/Apache; regenerate with `packaging/licenses/notices.sh` after
  dependency changes);
- write a tester zip with a `.sha256` next to the Steam folder;
- are safe to re-run (`--skip-build` re-packages what the target dir holds).

### Windows (cross-compiled on the Mac)

`cargo xwin` builds `x86_64-pc-windows-msvc` with the static C runtime (no Visual C++
redistributable), the GUI subsystem (no console window behind the game) and the icon and version
info linked in from a `.res` that `llvm-rc` makes. All three come from linker flags in the script,
so there's no `build.rs`. The script checks the PE header (x64, GUI subsystem, version info, no
`vcruntime140.dll` import). The link prints `LNK4099 Cannot use debug info for libcmt.lib…`
warnings: harmless (Microsoft's static CRT points at PDBs that aren't shipped). Needs `rustup target add x86_64-pc-windows-msvc`,
`cargo install cargo-xwin`, `brew install llvm` (for `llvm-rc`).

The exe is not Authenticode-signed. Steam doesn't require it; SmartScreen only checks files
downloaded by a browser, so it only matters for the tester zip.

Folder (the Windows depot): `Pacenote.exe`, `assets/`, `CREDITS.txt`, `THIRD-PARTY-NOTICES.html`.
Steamworks launch option: executable `Pacenote.exe`, OS Windows, working dir empty (the install
folder).

### macOS

`Pacenote.app` (`Pacenote Demo.app` for the demo), universal (arm64 + x86_64) by default,
`--arch arm64` for one build. `Info.plist` has `NSMicrophoneUsageDescription` (voice chat) and
macOS 12 as the minimum. Bundle ids: `com.himanshuat.pacenote` and `com.himanshuat.pacenote.demo`
(never change them after release: macOS keys the microphone permission on them).
Steamworks launch option: executable `Pacenote.app` (or `Pacenote Demo.app`), OS macOS.

#### macOS signing (optional, recommended)

Without `SIGN_ID` the app is signed ad hoc. That runs when Steam installs it (Steam doesn't
quarantine its downloads), but a tester who downloads the zip from a browser gets a Gatekeeper
warning. To sign and notarize:

1. A **Developer ID Application** certificate in the login keychain (Apple Developer Program,
   USD 99/year). `security find-identity -v -p codesigning` lists it. This Mac has one
   (`Developer ID Application: Himanshu . (38QXSRV6AP)`, used for Shroud and Bay).
2. A notarytool profile, once:
   `xcrun notarytool store-credentials xcnotarize --apple-id <id> --team-id 38QXSRV6AP`
   (asks for an app-specific password; it's stored in the keychain, never in the repo).
3. `SIGN_ID="Developer ID Application: Himanshu . (38QXSRV6AP)" packaging/macos/build.sh --notarize`

Signing uses the hardened runtime with `packaging/macos/entitlements.plist`, which grants
`com.apple.security.device.audio-input`. Without that entitlement a hardened app never gets the
microphone prompt, and voice chat records silence.

### steamcmd

Not installed here. Either download the Steamworks SDK (partner site, Steamworks SDK zip;
`sdk/tools/ContentBuilder/builder_osx/steamcmd.sh`) or Valve's standalone
`https://steamcdn-a.akamaihd.net/client/installer/steamcmd_osx.tar.gz` into a folder of its own,
and run it once to update itself. Then `STEAMCMD=/path/to/steamcmd.sh packaging/steam/upload.sh …`.

`upload.sh` reads the app and depot ids from the environment or `packaging/steam/ids.env`
(gitignored; copy `ids.env.example`). It writes `app_build_<APPID>.vdf` and
`depot_build_<DEPOTID>.vdf` from the templates into `dist/steam/.steampipe/<edition>/`, then runs
`steamcmd +login <builder> +run_app_build …`. steamcmd asks for the password and the Steam Guard
code itself; nothing is stored in the repo. Try `--preview` first (builds manifests, uploads
nothing). `--setlive <branch>` puts the build live on a beta branch; the public (`default`) branch
is set live by hand in Steamworks (App Admin → SteamPipe → Builds), never from the script.

Use a separate Steam account for uploading, with only the permissions it needs (Steamworks →
Users & Permissions: "Edit App Metadata" and "Publish App Changes To Steam" for these two apps),
so the main account's password never goes into steamcmd.

## Launch checklist

Rough timeline: 30-day wait after the first app fee, store page review 3–5 business days, at
least 2 weeks Coming soon before release, build review before release. Start 6–8 weeks before
the date you want the demo out.

### 1. Steamworks account (week 0)

- [ ] Sign up at `partner.steamgames.com` (Steam Direct). Needs: legal name or company,
      address, bank details, tax interview (W-8BEN for an individual outside the US, or W-8BEN-E
      for a company; it decides the US withholding rate), and identity verification.
- [ ] Pay the **Steam Direct fee: USD 100 per app** (the full game). It isn't refundable; it is
      paid back once the game reaches USD 1,000 adjusted gross revenue. The demo is created from
      the full game's page and has no fee of its own.
- [ ] **30-day wait** between paying the first app fee and being allowed to release
      (Valve checks who you are). Use the time for the store page and builds.
- [ ] Add the builder account (above) under Users & Permissions.

Docs: <https://partner.steamgames.com/steamdirect>, <https://partner.steamgames.com/doc/gettingstarted/appfee>

### 2. Create the apps

- [ ] The fee gives you the **full game app** (App ID). Name: Pacenote.
- [ ] Demo: full game's App Landing Page → "All associated packages, DLC, demos and tools" →
      **Add Demo**. It gets its own App ID; its type is Demo with the base game's App ID set.
- [ ] In each app: SteamPipe → Depots: add a **Windows** depot (OS: Windows, 64-bit) and a
      **macOS** depot (OS: macOS). Note the depot ids in `packaging/steam/ids.env`.
- [ ] Installation → General: launch options as in [Builds](#builds) (one per OS).
- [ ] Packages: the demo's free package and the full game's package are created by Steamworks;
      check they include both depots.
- [ ] Set `PACENOTE_STEAM_URL` to the full game's store URL and rebuild the demo.

Docs: <https://partner.steamgames.com/doc/store/application/demos>, <https://partner.steamgames.com/doc/sdk/uploading>

### 3. Full game store page → Coming soon (wishlists open)

- [ ] Store page: description, about, features, tags, system requirements, languages
      (English: interface, subtitles; "full audio" no: the co-driver uses the OS voice), assets,
      trailer. Copy draft below.
- [ ] **Content survey** (Store Admin → Content Survey): no violence beyond vehicle crashes, no
      gore, no sexual content, no gambling, no purchases. Answer **user-generated content /
      unmoderated communication: yes**: players talk to each other over voice chat. Mention
      that voice is peer-to-peer between two players who share an address, with push-to-talk.
- [ ] Mark at least 4 screenshots "suitable for all ages".
- [ ] "Mark as ready for review". Store page review: typically **3–5 business days**; submit 7+
      days before you want it public.
- [ ] After approval: **Publish Coming Soon**. Wishlists open from that moment.
- [ ] Release date: "Coming soon" or a quarter/year rather than a fixed date until you're sure.
- [ ] Pricing: proposed in Steamworks (Pricing → Propose). Valve suggests regional prices from
      the USD one. A suggestion for a two-player game with 20 stages: USD 9.99–14.99; check
      similar small co-op games. Set it before release; it can change later.

Docs: <https://partner.steamgames.com/doc/store/coming_soon>, <https://partner.steamgames.com/doc/store/releasing>

### 4. Demo

- [ ] Build and upload: `packaging/windows/build.sh && packaging/macos/build.sh && packaging/steam/upload.sh demo`.
- [ ] SteamPipe → Builds: set the build live on `default`.
- [ ] Store presence: **assets only** (recommended): the demo shows as a button on the full
      game's page and has no page of its own. Capsules and library assets must say it's a demo
      (the drafts in `packaging/steam/store-assets/demo/` do). A separate demo page would need
      its own description, trailer and 5+ demo screenshots.
- [ ] Full game → Special Settings → Associated Demos: show the demo as the **green box**
      ("Download Pacenote Demo"), not the small sidebar link.
- [ ] Content survey for the demo (same answers).
- [ ] Demo landing page checklists → mark the store page and build ready for review.
- [ ] After review: **Release Demo…**.
- [ ] Within 2 weeks of the demo going out: demo landing page → **send the wishlist
      notification** (one email/push to everyone who wishlisted the full game; once only; it
      uses the 2-week email cooldown of the full game).

### 5. Steam Next Fest

- Each game can take part **once**. Eligible: full game page public (Coming soon), a public demo
  by the start, the game not released before the Fest ends.
- Dates on Valve's page: **February 2027: 22 Feb – 1 Mar**; **June 2027: 14–21 Jun**. Registration
  is on the full game's landing page; each edition's page lists its deadline.
- Demo build review: submit **5 weeks** before the Fest if the demo goes out before the press
  preview (11 days before the Fest), or **3 weeks** before if it goes out just before the Fest.
- Advice: put the demo out now and build wishlists with it, and enter **June 2027** (the
  February one is too close to also do the store page, review and polish). Have a stable demo
  with the guide role working over the internet by then: Next Fest players will try co-op with
  friends, and LAN-only / port-forwarding (an engine limit today) will be the first complaint.
  Stream during the Fest (Steamworks broadcast) and update the demo before it, not during.

Docs: <https://partner.steamgames.com/doc/marketing/upcoming_events/nextfest>, <https://partner.steamgames.com/doc/marketing/upcoming_events>

### 6. Wishlists: where to see them

- Steamworks → Sales & Activation Reports → the app → **Wishlists**: adds, deletes, purchases
  and gifts per day, updated daily for the day before. Regional breakdown under the regional
  sales report.
- Store page traffic: App Landing Page → Marketing & Visibility → **Traffic Breakdown** (also
  shows visits from wishlist emails).
- Steam emails wishlisters on release (and on discounts of 20%+ lasting over 8 hours), with a
  2-week cooldown per app between emails.
- The demo's own `demo.wishlist` telemetry event counts clicks on the in-game buttons
  (`docs/TELEMETRY.md`), which Steam's reports don't show.

Docs: <https://partner.steamgames.com/doc/marketing/wishlist>

## Store assets

Drafts made from in-game screenshots with the logo set in Archivo (OFL, the website's display
face): `packaging/steam/store-assets/` (`full/`, `demo/`, `screenshots/`, `contact-sheet.jpg`,
regenerate with `packaging/steam/store-assets/make.sh`). **They are drafts for a designer**: the
logo is plain type, the shots are unretouched gameplay frames. The capsules and the library hero are cut from 1080p
frames (the hero is upscaled 2×, so it's soft): recapture key shots at 4K, HUD off, for the
final art. Screenshot order in `screenshots/` is the suggested upload order (the guide's map is
second: it's what sets the game apart). The app icon (`packaging/icon-1024.png`) is a draft too. Capsules may hold only the game's
title (and "Demo" on the demo's): no quotes, awards, prices or slogans.

| Asset | Size (px) | Required | Notes |
|---|---|---|---|
| Header capsule | 920 × 430 | yes | top of the store page, recommendations |
| Small capsule | 462 × 174 | yes | search and lists; logo should nearly fill it |
| Main capsule | 1232 × 706 | yes | home page carousel |
| Vertical capsule | 748 × 896 | yes | sale pages |
| Screenshots | ≥ 1920 × 1080, 16:9 | 5 minimum | gameplay only; mark 4+ all-ages |
| Page background | 1438 × 810 | no | ambient; Steam makes one from the last screenshot if empty |
| Library capsule | 600 × 900 | yes (for the library) | title only |
| Library header | 920 × 430 | no (header capsule used) | |
| Library hero | 3840 × 1240 | yes | **no text**; keep the subject in the centre 860 × 380 |
| Library logo | 1280 wide and/or 720 tall, transparent PNG | yes | logo only; position set in the preview tool |
| Shortcut icon | 256 × 256 or 512 × 512, ICO or PNG | yes | `packaging/windows/pacenote.ico` |
| App icon | 184 × 184 JPG | yes | |
| Mac icon | ICNS | for mac shortcuts | `packaging/macos/AppIcon.icns` |
| Trailer | up to 1920 × 1080, 30/60 fps, H.264 + AAC 44.1/48 kHz, .mp4/.mov, 5000+ kbit/s | 1 to release | first seconds must work without sound; Steam cuts a 6 s micro-trailer from it |

The demo needs its own capsules and library set (marked "Demo"); screenshots are only needed if
it gets a separate page.

Trailer (not made here): 60–90 s. Open on the split: the driver's windscreen and the guide's map
side by side with the call on screen ("left 4, don't cut"), then a crest, a crash and the
damage readout, the 20 stages in quick cuts (snow, dust, dusk), the two players' voices.
Capture with the engine's play script or OBS at 1080p60.

Sources: <https://partner.steamgames.com/doc/store/assets/standard>,
<https://partner.steamgames.com/doc/store/assets/libraryassets>,
<https://partner.steamgames.com/doc/store/assets/community>, <https://partner.steamgames.com/doc/store/trailer>

## Store page copy (draft)

**Short description** (≤ 300 characters; this is 243):

> A rally game for two. The driver sees only the road. The guide sees only the map and the
> corners coming up. Talk each other through 20 stages of gravel, tarmac, mud and snow over
> built-in voice chat, or drive alone with an AI co-driver.

**About this game:**

> One of you drives. The other reads the road.
>
> In Pacenote the driver has the wheel, the windscreen and nothing else: no map, no corner
> arrows. The guide has the stage map, the car's position and a list of every corner ahead,
> graded from 1 (hairpin) to 6 (flat out), but can't touch the controls. The only way through is
> to talk: "left four into right two, don't cut, crest."
>
> Twenty fixed stages through pine forest, birch hills, dry scrub, highland moor and snow, at
> different times of day and in fog and rain. Every team drives the same twenty, so times compare
> on the leaderboards, and your best run comes back as a ghost to beat.
>
> The car is simulated: weight transfer and tyres that behave differently on each surface. Clip
> a marker post or a sapling and it snaps; hit a big tree and you stop dead. Recover to the
> road for a time penalty.
>
> No partner tonight? Drive solo with the AI co-driver reading the notes.

**Demo** (in the demo's description field if it gets one, else in the about text):

> The free demo has three stages: Pinewater (gravel, pine forest), Thornback (tarmac, scrub)
> and Frostline (snow). Times and settings carry over to the full game.

**Features:**

- Two players, two roles: driver and guide, on two PCs, with built-in voice chat (push-to-talk
  or open mic).
- 20 stages, 2–6 km each: gravel, tarmac, mud and snow; five landscapes; morning to dusk; fog
  and rain.
- Corner calls graded 1–6 the way real co-drivers grade them, plus crests and narrows.
- Driving model with weight transfer, per-surface grip, four-wheel drive, handbrake turns.
- Posts, signs and saplings that snap when you clip them; shrubs you can cut through.
- Cockpit, bonnet and chase cameras.
- Per-stage team leaderboards and ghost cars.
- Solo practice with an AI co-driver.
- Keyboard or gamepad.

Online play is direct between the two PCs: the driver hosts, the guide joins by address (same
network, or the driver forwards a port). Say so on the page (under the features, or in the
FAQ/"Notes"): players will otherwise expect matchmaking.

**Tags** (up to 20; the first ones weigh most; check each exists in Steam's list when entering
them). The asymmetric driver/guide split may have no tag of its own: say it in the short
description. Suggested, in order: **Racing, Rally, Online Co-Op,
Co-op, Driving, Simulation, Automobile Sim, Physics, Realistic, Atmospheric, Indie, Singleplayer,
Multiplayer, Time Attack, Snow, Nature, Offroad, 3D, First-Person, Third Person.**
Avoid "Split Screen" and "Local Multiplayer" (Steam means one screen; Pacenote needs two PCs).

Not on the page, ever: made-up quotes, awards, review scores or player counts. Real press quotes
can go in the "Reviews" field later.

## System requirements

Measured on this Mac (Apple M5 Pro, 15 CPU cores, 16 GPU cores, 24 GB, macOS 26.6), dev build
(dependencies optimised, the game itself at opt-level 1), offscreen 1920 × 1080, no frame cap.
Windows figures are **estimates**: the game hasn't run on Windows yet. Test on a real PC before
publishing them. The minimum column is a guess: at ~100 fps here, a GPU about a third as fast should still hold 30 fps at 1080p.

| Stage | Look | FPS (chase cam) | Frame time p50 / p95 | FPS (cockpit) | Peak RSS |
|---|---|---|---|---|---|
| 1 Pinewater | pine forest, morning | 98 | 9.9 / 13.0 ms | 112 | 575 MB |
| 3 Birchmoor | birch hills, noon | 119 | 8.4 / 10.0 ms | 138 | 574 MB |
| 4 Low Fen | fog | 182 | 5.4 / 6.0 ms | 195 | 602 MB |
| 5 Sunscar | scrub, noon | 115 | 8.4 / 10.6 ms | 130 | 561 MB |
| 7 High Cairn | moor, dusk | 123 | 8.1 / 9.5 ms | 140 | 619 MB |
| 9 Frostline | snow forest | 109 | 9.2 / 9.8 ms | 128 | 635 MB |
| 12 Saw Mill | pine, dusk, tarmac | 107 | 9.3 / 10.5 ms | 98 | 670 MB |
| 15 Dust Devil | scrub, dusk | 117 | 8.6 / 9.8 ms | 111 | 673 MB |

`perf.stats` over the control port, sampled after driving ~2 s at four points along each stage;
peak RSS from memgate. RSS leaves out most GPU allocations: `HANDOFF.md` measured a ~2.0 GB
memory footprint on stage 1 including GPU memory. Release builds are faster than these dev
builds. About 30,000 entities per stage. Loading a stage takes ~4–10 s here.

| | Minimum | Recommended |
|---|---|---|
| **Windows** OS | Windows 10 64-bit | Windows 11 64-bit |
| Processor | 4 cores, e.g. Intel Core i5-6500 / AMD Ryzen 3 1200 | 6 cores, e.g. Intel Core i5-10400 / AMD Ryzen 5 3600 |
| Memory | 8 GB RAM | 16 GB RAM |
| Graphics | DirectX 12 or Vulkan, 4 GB, e.g. GeForce GTX 1050 Ti / Radeon RX 570 | 6 GB, e.g. GeForce GTX 1660 Super / Radeon RX 5600 XT |
| Storage | 500 MB | 500 MB |
| Network / other | Broadband for team play; a microphone for voice; headphones for open mic | |
| **macOS** OS | macOS 12 Monterey | macOS 14 or later |
| Processor | Apple M1, or Intel with a Metal GPU | Apple M1 Pro or newer |
| Memory | 8 GB | 16 GB |
| Graphics | Metal | |
| Storage | 500 MB | 500 MB |

Install size (demo 0.1.0): Windows folder 153 MB (exe 92 MB + assets 61 MB), macOS app 208 MB (universal). Downloads are smaller: Steam compresses (the zips are 97 MB and 129 MB).

## Day of launch

For the demo going live (and, later, the full release):

- [ ] The build on `default` is the one tested on a real Windows PC and a Mac (fresh install,
      no old data folder).
- [ ] `PACENOTE_STEAM_URL` points at the real store page; the in-game wishlist button opens it.
- [ ] Telemetry endpoint and key were set at build time; one test run shows up on the server.
- [ ] Store page: all required assets uploaded, trailer first, screenshots marked all-ages,
      system requirements, languages, the "direct connection / port forwarding" note.
- [ ] Demo green box on the full game's page (Associated Demos).
- [ ] Press **Release Demo…**; check the store page in a logged-out browser and in the Steam
      client; install the demo from Steam on both OSes and drive a stage.
- [ ] Send the **demo wishlist notification** (within 2 weeks; once).
- [ ] Announce: Steam event/announcement on the full game's page (event cover 800 × 450,
      header 1920 × 622), the website, socials, the Reddit post with the telemetry stats later.
- [ ] Watch for 48 h: Steam discussions, crash reports on the trace server, refund-free demo
      reviews don't exist but discussions do. Hotfix through `upload.sh demo` + set live.
- [ ] Day 2+: Wishlists report (it lags one day).

## Shipped asset paths

The engine (`engine/src/paths.rs`, `assets_dir`) looks for `assets/` in this order:
`PACENOTE_ASSETS` env → `<bundle>/Contents/Resources/assets` (macOS app) →
`GameInfo::assets_fallback` if that folder exists → `./assets` (working directory) → the fallback
path even if missing.

- **macOS app:** found in `Contents/Resources/assets`. Correct.
- **Windows:** the fallback is the Mac path of the source tree (compiled in by
  `concat!(env!("CARGO_MANIFEST_DIR"), "/assets")`), which doesn't exist there, so it uses
  `./assets` in the **working directory**. Steam starts the game in its install folder by default,
  so it works through Steam, and a double-click in Explorer works too. Started any other way (a
  shortcut with another "Start in" folder, a launcher, a command prompt in another folder) it
  finds no assets.

Changes wanted (not made: `src/` and the engine belong to their owners):

1. **Engine, `paths.rs::assets_dir`:** after the bundle check, try `<folder of the exe>/assets`
   (`std::env::current_exe()?.parent()?.join("assets")`), before `assets_fallback`. Fixes
   Windows (and Linux) wherever the game is started from. Add a test beside
   `game_folder_beats_the_working_directory`.
2. **Pacenote `src/lib.rs::info()`:** compile the fallback only into dev builds, so ship builds
   don't carry the build machine's path (`/Users/<name>/…`) and can't find a stray source tree:
   ```rust
   let info = GameInfo::new("pacenote", "Pacenote").tagline("One drives. One reads the road.");
   #[cfg(feature = "dev")]
   let info = info.assets_fallback(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"));
   info
   ```
   The engine's play script builds with default features (`dev`), so its runs keep working.
3. **Pacenote `src/meta/screens.rs:898`:** the credits screen falls back to the same compiled-in
   `CARGO_MANIFEST_DIR` path when `AssetsDir` is missing; the same `#[cfg(feature = "dev")]`
   treatment (or no fallback) removes the last build path from the binary. Today the Windows exe
   contains `/Users/himanshu/codes/game/pacenote/assets` twice (from 1 and 3); every other path is
   rewritten to `/src`, `/cargo`, `/rustup` by the build scripts.

## Risks and what to test on Windows

The Windows build links and its folder is right, but it has **never run**. Test on a real
Windows 10 and 11 PC (not the AWS build box without asking):

1. Launches from Steam and from Explorer; no console window; the icon shows on the exe and the
   taskbar.
2. Graphics: wgpu picks DX12 (or Vulkan); try an NVIDIA, an AMD and an Intel iGPU. Loading time
   (shader compilation happens on first run; macOS took ~4 s).
3. **Co-driver voice (`src/codriver/speech.rs`):** each line starts `powershell` with
   System.Speech. Risks:
   - From a GUI-subsystem exe every spawn **opens a console window for a moment** (also the
     `powershell -?` probe at first use, and `cmd /C start` in `demo::open_store`). Fix: on
     Windows set `CommandExt::creation_flags(0x0800_0000)` (`CREATE_NO_WINDOW`) on those
     `Command`s. For the store link, `explorer.exe <url>` or a `steam://store/<appid>` URL also
     avoids `cmd` (and `&` in a URL breaks `cmd start`).
   - PowerShell takes ~0.3–1 s to start and load System.Speech, per line: calls come late at
     speed (`calls::LEAD_S`). Fix: one long-lived PowerShell reading lines from stdin, or SAPI
     through the `windows` crate.
   - Some locked-down or "N" editions lack System.Speech or block PowerShell: then subtitles
     only (already handled: `Backend::None`).
4. **Microphone:** Windows Settings → Privacy → Microphone → "Let desktop apps access your
   microphone" off = cpal gets silence or an error. Test voice both ways; consider a hint in
   the voice settings when the level stays at zero.
5. **Firewall:** hosting as driver opens a listening UDP port; Windows Defender Firewall asks
   on first host. If the player clicks Cancel, the guide can't join. Document it in the FAQ.
6. Saves/settings in `%APPDATA%\pacenote\data`; uninstall from Steam leaves them (expected).
7. Telemetry reaches the server from Windows (OS/CPU/GPU fields filled).
8. Gamepad (XInput) and keyboard layouts (AZERTY: WASD bindings).

macOS:
- A signed (hardened) build without the audio-input entitlement gets no mic: the entitlement
  is in `entitlements.plist`; test voice from a signed build after install from Steam.
- `say` speaks through the OS, not the game's mixer: the master volume doesn't affect it.

Both: the dependency licence file must be regenerated when dependencies change
(`packaging/licenses/notices.sh`); Valve can't check it, but the licences require it.
