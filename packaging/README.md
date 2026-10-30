# Packaging

Ship builds for Steam. The whole process (accounts, store page, uploads, launch) is in
`docs/STEAM.md`.

| Path | What |
|---|---|
| `common.sh` | Shared setup: version, edition → cargo features, target dir, path remapping, memgate |
| `windows/build.sh` | Windows x64 folder, cross-compiled on the Mac with cargo-xwin |
| `windows/pacenote.ico` | Exe and shortcut icon (from `icon-1024.png`) |
| `macos/build.sh` | `Pacenote.app` / `Pacenote Demo.app`, universal; optional Developer ID signing + notarization |
| `macos/Info.plist.in`, `entitlements.plist`, `AppIcon.icns` | Bundle metadata (mic usage text), hardened-runtime entitlements (mic) |
| `steam/upload.sh` | Fills the VDF templates and runs `steamcmd +run_app_build` |
| `steam/*.vdf.in`, `ids.env.example` | SteamPipe app/depot build templates; app and depot ids |
| `steam/store-assets/` | DRAFT capsules, library art, screenshots (`make.sh`, `contact-sheet.jpg`) |
| `licenses/notices.sh` | Regenerates `THIRD-PARTY-NOTICES.html` (Rust crate licences; ships with the game) |
| `icon-1024.png` | DRAFT app icon source |

```sh
packaging/windows/build.sh && packaging/macos/build.sh        # demo
EDITION=full packaging/windows/build.sh && EDITION=full packaging/macos/build.sh
packaging/steam/upload.sh demo --preview
```

Output goes to `dist/steam/<edition>/` (gitignored).
