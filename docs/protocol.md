# FLVSTX plugin ⇄ agent protocol

Transport: WebSocket on `ws://127.0.0.1:<port>` (default port 7878). The **agent sidecar is the server**
(`node agent/dist/index.js --port 7878`); the plugin or the CLI connects as a client. One client per
agent process. All frames are JSON text.

The plugin is the source of truth for the session. The agent never keeps notes; it reads and writes
through RPCs that the client answers by calling `flvstx_core::ops::dispatch`.

## Client → Agent

| type | fields | meaning |
|---|---|---|
| `user_message` | `text`, `context` (compact session summary string), `session_id?` | a chat turn from the user |
| `rpc_result` | `id`, `ok`, `result?`, `error?` | answer to an agent `rpc` |
| `cancel` | | abort the current turn |
| `ping` | | keep-alive |

## Agent → Client

| type | fields | meaning |
|---|---|---|
| `ready` | `backend` (`sdk` \| `cli`), `version` | sent once after connecting |
| `assistant_delta` | `text` | streamed text fragment |
| `assistant_message` | `text` | a complete assistant text block |
| `tool_call` | `name`, `input` | the agent is invoking a composer tool (for UI chips) |
| `tool_result` | `name`, `summary` | short result text (for UI chips) |
| `rpc` | `id`, `method`, `params` | run a session operation and reply with `rpc_result` |
| `done` | `session_id`, `cost_usd?`, `turns` | the turn finished |
| `error` | `message` | the turn failed |
| `pong` | | |

## RPC methods (see `crates/flvstx-core/src/ops.rs`)

`get_session`, `get_notes`, `set_key_tempo`, `set_form`, `set_section`, `copy_section`, `set_chords`,
`suggest_chords`, `set_notes`, `generate`, `generate_all`, `humanize`, `analyze`, `set_lyrics`, `lock`,
`clear`, `transpose`, `undo`, `redo`, `export`, `list_scales`.

Every mutating RPC snapshots the session for undo. Errors are returned as `{ok:false, error}` and
surfaced to the model as tool errors so it can correct itself.
