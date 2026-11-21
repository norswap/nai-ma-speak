# Contributing

Thanks for helping. Bug reports, slang suggestions and pull requests are all welcome.

## Layout

- `addon/NaiMaSpeak`: the WoW AddOn (Lua 5.1). It must load on both WoW: Forever (`## Interface: 16001`) and Retail, and
  must handle Midnight secret values: check `issecretvalue` before reading chat text.
- `companion`: the Rust companion app (eframe/egui, xcap, ureq, arboard, zip).
- `assets`: the icon source and the README's pictures.
- `tests`: the AddOn's tests, run outside the game.

## Building From Source

It needs Rust 1.95 or newer. On macOS, `./macos-app.sh <folder>` inside `companion/` builds `Nai Ma Speak.app` into that
folder; run `./macos-signing-cert.sh` once beforehand to sign your builds with a local certificate, so the Screen
Recording permission survives rebuilds. Elsewhere, `cargo build --release` inside `companion/` makes
`target/release/nai-ma-speak`; Linux also needs the PipeWire, X11 and Wayland development packages listed in
`.github/workflows/build.yml`.

## Making a Release

Run the **Build** workflow from the Actions tab with a tag such as `v0.3.0` in the *release* box, or push a `v*` tag.
The workflow builds everything and publishes the release with notes generated from the merged pull requests. The tag
sets the version: a `v0.3.0` run first commits "Release v0.3.0" to main with `## Version` in `NaiMaSpeak.toc`, the
version in `Core.lua` and the companion version in `companion/Cargo.toml` and `Cargo.lock` set to 0.3.0, then builds and
tags that commit. Prefer the release box: a pushed tag stays on the commit you tagged, before the version commit.

## Before You Open a Pull Request

In `companion/`:

```
cargo test --locked
cargo clippy --all-targets --locked
```

For AddOn changes, run `lua5.1 tests/addon_test.lua` from the repository root, then test in game with `/tr /test` and
`/tr /probe`, and say which client and interface number you used.

Keep one change per pull request. The Build workflow must pass on Windows, macOS and Linux.

## Slang and Glossary Terms

New WoW terms go in the files in `companion/src/slang/`, one `中文=English` per line: `zh_cn.txt` for mainland terms,
`zh_tw.txt` for Taiwanese terms the mainland list lacks, and `places.txt` for place names. A term written the same way
in both scripts belongs in `zh_cn.txt` alone. Keep entries short and use the meaning players actually use (任务 = quest,
not task). Single characters match too often, so use two or more.

## Pixel Block

If you change the block format, change both `addon/NaiMaSpeak/Strip.lua` and `companion/src/strip.rs`, and keep the
tests in `strip.rs` passing; its `encode` is a copy of the AddOn's encoder.
