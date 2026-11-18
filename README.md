# Pacenote

An original two-person rally game written in Rust. One player drives and sees the 3D stage but
has no map; the other is the guide, who sees the map and the upcoming corner calls but can't
control the car. The two talk over built-in voice chat. There are 20 fixed stages generated from
seeds, and teams race each other's times.

**Play it:** [himanshuat.itch.io/pacenote](https://himanshuat.itch.io/pacenote)

## Quick start
Needs a Rust toolchain. The engine (Hull) is a git dependency
([himasnhu-AT/hull-dep](https://github.com/himasnhu-AT/hull-dep)), so Cargo fetches it on the first build.
```sh
git clone https://github.com/himasnhu-AT/pacenote && cd pacenote
cargo run      # play
cargo test     # tests
```

## Docs
- [docs/DESIGN.md](docs/DESIGN.md): the full game design
- [docs/GAME_OVERVIEW.md](docs/GAME_OVERVIEW.md): systems, code layout and tooling
- [HANDOFF.md](HANDOFF.md): current state for contributors
- [ATTRIBUTION.md](ATTRIBUTION.md): third-party assets and licences
