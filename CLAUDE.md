# Translate (fork of cherryroots/Translate)

WoW chat translation: a Lua addon (`addon/Translate`) draws Chinese chat lines as a pixel strip, and a Rust companion (`companion/`) reads the strip off the screen, translates it and shows an overlay.

## Commands

Run from `companion/`:

- `cargo test --locked` — must stay green after every commit
- `cargo clippy --all-targets`
- `cargo build --release --locked` — output: `target/release/translate-companion`
- `./macos-app.sh [folder]` — builds `Translate.app` (opens without a Terminal window) into the folder, default `target/release`

Needs Rust 1.95 or newer.

## Local install (this Mac)

- The companion runs as `/Applications/wow-translate/Translate.app`; install with `./macos-app.sh /Applications/wow-translate`. Its `config.json` sits next to the app (holds API keys; never commit it).
- For logs, run `/Applications/wow-translate/Translate.app/Contents/MacOS/translate-companion` from Terminal.
- The addon is symlinked from `addon/Translate` into `/Applications/World of Warcraft/_forever_/Interface/AddOns/Translate`.
- macOS ties Screen Recording permission to the exact binary, so a rebuilt companion may need it granted again.

## Rules

- Read-only toward the game: never send chat or press keys for the player. Replies go through the clipboard.
- Use Claude Console API keys only; never wire in the Claude Code subscription login.
