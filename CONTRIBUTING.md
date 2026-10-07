# Contributing to Tonefold

Thanks for helping. Bug reports, fixes, new generators, genre knowledge for the composer and docs
are all welcome.

## Before you start

- For anything bigger than a small fix, open an issue or a discussion first so we can agree on the
  shape before you write it.
- Tonefold is GPL-3.0-or-later. By contributing you agree your work is released under that licence.

## Layout

| Path | What it is |
|---|---|
| `crates/tonefold-core` | The engine: theory, generators, humanization, MIDI, rendering. No plugin dependencies. |
| `crates/tonefold-ipc` | The wire between the app and the composer sidecar. |
| `crates/tonefold-plugin` | The app and its egui editor, built as the standalone app and, via nih-plug, the optional CLAP/VST3. |
| `crates/tonefold-cli` | The command line, also used by external AIs through `skill/tonefold-song`. |
| `agent/` | The Claude Agent SDK sidecar: composer, producer and specialist prompts, tools, Ollama bridge. |
| `vendor/` | nih-plug and egui-baseview with Tonefold's patches (host-driven resize, keyboard focus in hosts). |
| `website/` | The project site, served by GitHub Pages. |

## Build and test

You need Rust stable and Node 20+. The app and plugins build on macOS and Windows; `tonefold-core`,
the CLI and the agent also build on Linux.

```sh
cargo test --workspace
cargo build --release -p tonefold-plugin --features standalone --bin tonefold-standalone  # the app
cargo xtask bundle tonefold-plugin --release   # the optional CLAP and VST3
cd agent && npm ci && npm run build && npm test
```

On macOS, `scripts/package-macos.sh` builds the universal app and the `.dmg` (add the targets first:
`rustup target add aarch64-apple-darwin x86_64-apple-darwin`). On Windows, `scripts\install.ps1`
installs the app you built; add `-Plugins` to try the CLAP/VST3 in a DAW.

## Pull requests

- Keep each PR to one change and describe what you heard or saw before and after. For anything
  musical, a short MIDI or WAV export helps a lot.
- Run `cargo fmt`, `cargo test --workspace` and the agent tests before pushing. CI runs the same.
- Add a line to the top section of `CHANGELOG.md` for anything a user would notice.
- Prompt changes in `agent/prompts/` should say which behaviour they fix and how you tested it.

## Reporting bugs

Use the bug template. Include the Tonefold version, whether it was the app or the plugin (and which
DAW), and the log from `~/Library/Application Support/Tonefold` (macOS) or `%LOCALAPPDATA%\Tonefold`
(Windows) if there is one.
