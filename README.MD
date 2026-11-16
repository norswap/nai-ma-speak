# Nǎi Mā Speak

Chat translation between Chinese and English for World of Warcraft, made for playing on a Taiwanese server without reading Chinese. 奶媽 (nǎi mā, "wet nurse") is what Chinese-speaking players call a healer.

It reads Traditional and Simplified Chinese, and writes your replies in Traditional Chinese with Taiwanese wording, or in Simplified.

This is a fork of [cherryroots/Translate](https://github.com/cherryroots/Translate). It has two parts:

- **`addon/Translate`**: the in-game addon, listed in the game as "Nai Ma Speak". One folder loads in both WoW: Forever and retail (`## Interface: 16001, 120105`).
- **`companion`**: a small Rust app that runs next to the game, translates, and draws the result over your chat window.

> **Tested only on macOS with WoW: Forever.** Retail WoW, Windows and Linux are untested since the fork: the addon declares the retail interface and the companion compiles for the other systems, but nobody has run them.

## How it works

WoW addons cannot reach the internet, so the two parts talk through the screen:

1. **Game to companion.** The addon draws each new chat line as a small strip of colored blocks in the top-left corner of the screen (256 x 32 pixels). The companion finds the strip, decodes it and translates the Chinese lines.
2. **Overlay.** The companion shows the lines in a see-through, click-through window. It can sit exactly on one of your chat tabs, taking its place, size and text size, and it draws lines the way the game does: same colors, wording, class-colored names, chat font and text shadow. A Chinese line appears at once, marked "(translation pending)", and turns into English when the translation arrives.
3. **Replies.** You type English, and the Chinese is put on your clipboard to paste into the game's chat box. The companion only ever writes to the clipboard.

The addon never learns what the companion did, and the companion never presses a key or sends chat for you.

Reading the strip is screen capture: it only reads pixels and never sends input to the game, but Blizzard's terms do not clearly allow or forbid it. Use it at your own judgment.

## Features

- **A copy of a chat window.** Pick a chat window in the addon's options and the overlay shows every line that window shows, including system messages and what other addons print, with their color tags. Without one, the overlay gets player chat and the game's own messages as they arrive.
- **Translation backends.** Google Translate's free endpoint (no setup, no key), Claude with a Claude Console API key, or any OpenAI-compatible endpoint: OpenAI, or a local model through Ollama or LM Studio.
- **Slang hints.** The chat models are told the WoW slang found in each line (缺補, 坦, 副本...), from a list of mainland and Taiwanese terms. Google Translate takes no hints.
- **In-game buttons.** "Translate" and "Settings" buttons on the chat window open the reply box and the companion's window.
- **Scrollback.** With the overlay on a chat tab, the mouse wheel over that tab scrolls the overlay.
- **Chat history.** The companion keeps the recent lines in a tab where text can be selected and copied.

## Install

There is no release of this fork yet, so both parts come from this repository.

### The addon

Copy or link the `addon/Translate` folder into the AddOns folder of the client you play, keeping the folder name `Translate`:

```
Forever: World of Warcraft/_forever_/Interface/AddOns/Translate/
Retail:  World of Warcraft/_retail_/Interface/AddOns/Translate/
```

Restart the client if "Nai Ma Speak" does not appear in the addon list. If retail lists it as out of date, the retail patch has moved past `120105`: check the number with `/dump select(4, GetBuildInfo())` and update the `## Interface:` line in `Translate.toc`.

### The companion

It needs Rust 1.95 or newer.

- **macOS:** run `./macos-app.sh <folder>` inside `companion/`. It builds `Nai Ma Speak.app` into that folder. macOS asks for the Screen Recording permission the first time, so the companion can read the strip. Run `./macos-signing-cert.sh` once beforehand to sign builds with a local certificate, so the permission survives rebuilds.
- **Windows and Linux:** run `cargo build --release` inside `companion/`; the program is `target/release/translate-companion`. Linux also needs the PipeWire, X11 and Wayland development packages listed in `.github/workflows/build.yml`.

The companion keeps `config.json` and `translate.log` next to the program (next to the `.app` on macOS), so give it a folder of its own. `config.json` holds your API keys in plain text.

Run WoW in **windowed** or **windowed (fullscreen)** mode: overlays cannot draw over exclusive fullscreen, and screen capture may see a black screen there.

## Using it

### In the game

The two buttons sit on the main chat window. Their place (on the tab row, top right, bottom right, or hidden) is set under Options > AddOns > Nai Ma Speak, along with which chat window the overlay copies.

- **Translate** opens a box above the chat window. Type English and press Enter: the box closes at once and a note on the overlay says "Translating", then "Copied, paste it". Paste into the game's chat box and send. If the translation fails or is too long for one chat message (255 bytes), the note says so, and Translate reopens the box with your text.
- **Settings** brings the companion's window over the game, and sends it away again.

The same from the chat box, where `/nms` works in place of `/tr`:

| Command | Does |
| --- | --- |
| `/tr` | Open or close the translate box |
| `/tr hello` | Translate and copy, then reopen the chat box on the channel it was last on |
| `/tr /g hello` | The same, reopening the chat box on that target: `/p`, `/s`, `/y`, `/ra`, `/i`, `/o`, `/2`, `/w Name`, `/r` |
| `/tr /options` | Open the addon's options |
| `/tr /all` | Toggle lines without Chinese in the overlay |
| `/tr /strip` | Toggle the pixel strip |
| `/tr /test` | Say a Chinese test line in `/say` to check the strip and overlay |
| `/tr /test 10` | Send that many test lines to the overlay only |
| `/tr /probe` | Report which client features the addon found |
| `/tr /help` | List the commands |

### The companion window

Settings take effect as you change them and are saved by themselves. **Undo** puts them back as they were when the companion was opened, **Restart** starts it again, and **Hide** hides the window while the overlay stays; the game's Settings button or the app's icon brings it back.

- **Backend:** what translates, and which Chinese replies are written in. "Test translation" sends a fixed line and shows the answer and how long it took.
  - *Google Translate* needs nothing, but it is an unofficial endpoint: it is rate-limited and may stop working.
  - *Claude* needs a key from the [Claude Console](https://console.anthropic.com). Haiku is the default, and enough for chat lines.
  - *OpenAI-compatible* has presets for OpenAI and for Tencent's translation model [HY-MT1.5-7B](https://huggingface.co/tencent/HY-MT1.5-7B-GGUF) on a local [Ollama](https://ollama.com) server (`ollama pull hf.co/tencent/HY-MT1.5-7B-GGUF:Q4_K_M`, 4.6 GB, free and offline). "Custom" takes any base URL and model; a bare address like `http://localhost:1234` gets `/v1` added.
- **Overlay:** whether to show the Chinese under each translation, when to hide the overlay, how many lines to keep, and where it sits. "Anchor to chat tab" puts it on one of the game's chat tabs; a spare, empty tab works best. Unticked, you unlock the overlay, drag it into place and set its size and text size yourself. Keep it off the top-left corner, where the strip is.
- **Status:** whether the strip was found, and which chat tabs the game reported.
- **Chat history** and **Translate:** the recent lines, and a field for translating a reply outside the game.

Chat lines are sent to whichever backend you choose. Only a local model keeps them on your computer.

## Troubleshooting

- **The overlay stays empty.** Open the Status tab: it says whether the pixel strip was found. The strip must be visible in the top-left corner of the game, and on macOS the companion needs the Screen Recording permission. `/tr /probe` in the game reports what the addon found, and `/tr /test` sends a line through.
- **To check a screenshot for the strip,** run `translate-companion --decode screenshot.png`.
- **The companion froze or misbehaved.** `translate.log`, next to `config.json`, records crashes and stalls.

## Known limits

- Chat text inside instances may be a secret value under Midnight rules. The addon cannot read those lines, so they are not shown or translated.
- Each strip frame holds 376 bytes, enough for a chat message (255 bytes at most) with its speaker and colors. Longer lines, such as some system messages, are cut.
- The strip carries about 1,500 bytes a second. In a flood of chat, lines queue in the addon, up to 200, and the oldest are dropped beyond that.
- Icons inside chat lines (raid markers and the like) are left out of the overlay.
- A translation keeps the colors of the original line only where they can be placed: a line in one color, item links in order, and words that come back unchanged.
- The buttons sit on the main chat window and its docked tabs, not on a chat window floating on its own.
- The strip assumes the UI is drawn at the screen's physical resolution. If the companion never finds it, send a screenshot through `--decode`.

## Icon

`assets/icon.svg` is the source for the icons. `companion/icon.png` is the window icon, `companion/icon.ico` is built into the Windows exe, and `addon/Translate/icon.tga` is shown in the AddOns list through `## IconTexture`. To change the icon, edit the SVG and re-export those files at the same sizes.

## Making a release

Run the **Build** workflow from the Actions tab with a tag such as `v0.2.0` in the *release* box, or push a `v*` tag. The workflow builds everything and publishes the release with notes generated from the merged pull requests. The tag sets the version: a `v0.2.0` run first commits "Release v0.2.0" to main with `## Version` in `Translate.toc` and the companion version in `companion/Cargo.toml` and `Cargo.lock` set to 0.2.0, then builds and tags that commit. Prefer the release box: a pushed tag stays on the commit you tagged, before the version commit.

The workflow still names its files after the original project (`Translate-addon.zip`, `translate-companion-*`), and its macOS build is a bare program, not the app bundle that `macos-app.sh` makes.

## License and credits

MIT, see [LICENSE](LICENSE). The original project is by cherryroots. Slang terms from two other MIT-licensed addons, the bundled Marcellus font and the Chinese conversion library are credited in [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES). Contributions are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md).
