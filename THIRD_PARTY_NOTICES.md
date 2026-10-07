# Third-party notices

Tonefold is GPL-3.0-or-later. It includes or downloads the following, each under its own licence.

| Component | Where | Licence |
|---|---|---|
| [nih-plug](https://github.com/robbert-vdh/nih-plug) by Robbert van der Helm, with Tonefold patches | `vendor/nih-plug` | ISC (framework); its VST3 bindings are GPL-3.0, which is why Tonefold is GPL |
| [egui-baseview](https://github.com/BillyDM/egui-baseview), with Tonefold patches | `vendor/egui-baseview` | MIT |
| "Music Composition Agent Skill" by SJY051 | `agent/reference/` | CC BY 4.0; see `agent/reference/NOTICE.md` |
| [GeneralUser GS](https://www.schristiancollins.com/generaluser.php) by S. Christian Collins | included in release packages, with its licence | GeneralUser GS License v2.0 |
| [Claude Agent SDK](https://www.npmjs.com/package/@anthropic-ai/claude-agent-sdk) | installed by npm, not distributed | Anthropic's terms |

Rust and npm dependencies are listed in `Cargo.lock` and `agent/package-lock.json` with their own
licences.
