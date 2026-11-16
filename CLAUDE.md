# Nai Ma Speak (fork of cherryroots/Translate)

The app is shown as "Nǎi Mā Speak" (奶媽, healer in Chinese players' slang) in its own window, and written without tone marks wherever people may search for it: file names, the addon list, chat output. The addon folder, its saved variables and the Rust package keep the name Translate.

WoW chat translation: a Lua addon (`addon/Translate`) draws Chinese chat lines as a pixel strip, and a Rust companion (`companion/`) reads the strip off the screen, translates it and shows an overlay.

## Commands

Run from `companion/`:

- `cargo test --locked` — must stay green after every commit
- `cargo clippy --all-targets`
- `cargo build --release --locked` — output: `target/release/translate-companion`
- `./macos-app.sh [folder]` — builds `Nai Ma Speak.app` (opens without a Terminal window) into the folder, default `target/release`

Needs Rust 1.95 or newer.

## Local install (this Mac)

- The companion runs as `/Applications/wow-translate/Nai Ma Speak.app`; install with `./macos-app.sh /Applications/wow-translate`. Its `config.json` sits next to the app (holds API keys; never commit it).
- The companion writes `translate.log` next to `config.json`: panics, a window pass that never finishes ("STUCK") and passes running without pause ("BUSY", with what asked for them).
- To see the companion window without sitting at the screen: `open -n "/Applications/wow-translate/Nai Ma Speak.app" --env TRANSLATE_SHOTS=<folder>` saves a picture of each tab into the folder and quits.
- The addon is symlinked from `addon/Translate` into `/Applications/World of Warcraft/_forever_/Interface/AddOns/Translate`.
- The app is signed with the self-signed "Translate Local Signing" certificate in the login keychain (created by `./macos-signing-cert.sh`), so the Screen Recording permission survives rebuilds. Without that certificate the build is signed ad hoc and needs the permission again each time.

## Rules

- Read-only toward the game: never send chat or press keys for the player. Replies go through the clipboard.
- Use Claude Console API keys only; never wire in the Claude Code subscription login.
