# Nai Ma Speak (fork of cherryroots/Translate)

The app is shown as "Nǎi Mā Speak" (奶媽, healer in Chinese players' slang) in its own window, and written without tone marks wherever people may search for it: file names, the addon list, chat output. The addon folder, its saved variables and the Rust package keep the name Translate.

WoW chat translation: a Lua addon (`addon/Translate`) draws Chinese chat lines as a pixel strip, and a Rust companion (`companion/`) reads the strip off the screen, translates it and shows an overlay.

## Layout

- `addon/Translate/`: `Core.lua` (events, saved settings, slash commands), `Chat.lua` (which lines go to the overlay and how they are worded), `Strip.lua` (the pixel strip and its queue), `Layout.lua` (chat window positions, mouse wheel), `Buttons.lua` (in-game buttons, typed replies), `Options.lua` (options panel).
- `companion/src/`: `strip.rs` (finds and decodes the strip), `protocol.rs` (what the addon's lines mean; the format is documented at its top), `worker.rs` (translation queue), `translate.rs` and `backends/` (prompts, slang, the three services), `app.rs` (the companion window, the overlay, the reply box), `theme.rs` and `fonts.rs` (look), `layout.rs` (placing the overlay on a chat tab), `focus.rs` (is WoW visible), `markup.rs` (WoW color codes), `config.rs`, `diag.rs` (log file).
- The addon and the companion only meet in the strip's line format. Change both sides together.

## Commands

- `cargo test --locked` in `companion/` — must stay green after every commit
- `cargo clippy --all-targets` in `companion/` — no warnings
- `lua5.1 tests/addon_test.lua` from the repository root — the addon's logic against stand-ins for the game's functions. The addon cannot be run outside the game, so this and playing are the only checks it gets.
- `cargo build --release --locked` in `companion/` — output: `target/release/translate-companion`
- `./macos-app.sh [folder]` in `companion/` — builds `Nai Ma Speak.app` (opens without a Terminal window) into the folder, default `target/release`

Needs Rust 1.95 or newer.

## Running it

- `./macos-app.sh <folder>` installs the app into a folder of its own; its `config.json` sits next to the app (holds API keys; never commit it).
- The companion writes `translate.log` next to `config.json`: panics, a window pass that never finishes ("STUCK") and passes running without pause ("BUSY", with what asked for them).
- To see the companion window without sitting at the screen: `open -n "<folder>/Nai Ma Speak.app" --env TRANSLATE_SHOTS=<pictures folder>` saves a picture of each tab and quits. It draws no overlay.
- For a picture of the overlay at work: `open -n "<folder>/Nai Ma Speak.app" --env TRANSLATE_GAME_SHOT=<file.png>` saves the game's window as it is on screen and quits. It uses the app's own Screen Recording permission, so it works where a shell's `screencapture` does not.
- With the certificate from `./macos-signing-cert.sh` in the login keychain, the app is signed the same way each build, so the Screen Recording permission survives rebuilds. Without it the build is signed ad hoc and needs the permission again each time.
- Where the app and the addon are installed on a given computer belongs in `CLAUDE.local.md`, which is not committed.

## Rules

- Read-only toward the game: never send chat or press keys for the player. Replies go through the clipboard.
