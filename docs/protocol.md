# FLVSTX plugin ⇄ agent protocol

Transport: WebSocket on `ws://127.0.0.1:<port>` (default port 7878). The **agent sidecar is the server**
(`node agent/dist/index.js --port 7878`); the plugin or the CLI connects as a client. One client per
agent process. All frames are JSON text.

The plugin is the source of truth for the session. The agent never keeps notes; it reads and writes
through RPCs that the client answers by calling `flvstx_core::ops::dispatch`.

The two sides ship separately, so the wire is additive: every field is optional with a default, and
an unknown `type` deserialises to `AgentEvent::Unknown` and is ignored rather than surfaced as an
error (`crates/flvstx-ipc/src/lib.rs`).

## Client → Agent

| type | fields | meaning |
|---|---|---|
| `user_message` | `text`, `context` (compact session summary string), `session_id?`, `model?`, `mode?`, `provider?`, `base_url?`, `think?` | a chat turn from the user |
| `list_models` | `base_url?` | ask an Ollama server what it can run; answered with `models` |
| `plan_decision` | `plan_id`, `decision` (`approve` \| `reject` \| `cancel`), `notes?` | answers a proposed plan; **not** a user message, because the producer is parked inside a tool call |
| `rpc_result` | `id`, `ok`, `result?`, `error?` | answer to an agent `rpc` |
| `cancel` | | abort the current turn (also settles a pending plan as `cancel`) |
| `ping` | | keep-alive |

`mode` picks the persona and tool set: `composer` (default) answers straight away, `producer` plans
first and may write nothing until the plan is approved. The two cannot share one SDK session —
`agents`, `allowedTools`, `hooks` and `systemPrompt` are fixed when a query is created — so switching
mode ends the current session and resumes the other one; the sidecar keeps one session id per mode.

`provider` is where the model runs: `claude` (default — the user's Claude Code login) or `ollama`,
with `base_url` naming the server (`http://localhost:11434`, or another machine). The sidecar keeps
one conversation per provider *and* mode: a transcript written with Claude is not resumed under a
local model, nor the other way round. The client keys its saved session ids the same way
(`Backend::session_key` in `crates/flvstx-ipc`): `composer` / `producer` for Claude,
`ollama:composer` / `ollama:producer` for Ollama.

### Ollama

The SDK drives the Claude Code CLI, and the CLI talks to whatever `ANTHROPIC_BASE_URL` names. For
Ollama the sidecar points it at a loopback shim of its own (`agent/src/ollama-shim.ts`) rather than
at Ollama's Anthropic-compatible endpoint, because that endpoint cannot set the context window —
it silently truncates the ~10k-token prompt (system prompt plus 32 tool schemas) to the server's
2048-token default, and the model, never having seen a tool, invents one — and it rejects the
trailing system-role message the CLI appends. The shim translates each Messages request into a
native `/api/chat` call (`agent/src/ollama-bridge.ts`: system text merged into one system
message, `tool_result` → tool-role messages, `tool_use` → `tool_calls`, thinking both ways,
`options.num_ctx` set) and streams the reply back as Anthropic server-sent events. The context
window is `FLVSTX_OLLAMA_NUM_CTX` (default 16384), capped at what `/api/show` says the model
supports; thinking is off unless the turn says `think: true` (the header's checkbox) or the sidecar
was started with `FLVSTX_OLLAMA_THINK=1`. For Claude, `think: false` disables extended thinking
(the SDK's `thinking: { type: "disabled" }`); absent or true is the CLI's adaptive default. That
setting is fixed when a query is created, so changing it restarts the session like a model change.

## Agent → Client

| type | fields | meaning |
|---|---|---|
| `ready` | `backend` (`sdk` \| `cli`), `version`, `modes?`, `providers?`, `ollama_url?` | sent once after connecting |
| `models` | `provider`, `base_url`, `models`, `error?` | an Ollama server's models (`GET /api/tags`), or why it could not be asked |
| `assistant_delta` | `text`, `agent?` | streamed text fragment |
| `assistant_message` | `text`, `agent?` | a complete assistant text block |
| `thinking_delta` | `text`, `agent?` | streamed reasoning (a thinking model on Ollama, or Claude's summaries) |
| `thinking` | `text`, `agent?` | a complete reasoning block |
| `tool_call` | `name`, `input`, `tool_use_id?`, `agent?` | the agent is invoking a composer tool (for UI chips) |
| `tool_result` | `name`, `summary`, `tool_use_id?`, `ok?`, `agent?` | short result text (for UI chips) |
| `plan_proposed` | `plan_id`, `plan` | the producer wants approval before it writes anything |
| `plan_resolved` | `plan_id`, `decision`, `notes?` | the answer landed; `decision: "stale"` means it was too late |
| `phase` | `phase` (`idle` \| `planning` \| `awaiting_approval` \| `executing`) | where a producer turn is; drives the panel's status, which `turn_active` alone cannot express |
| `todos` | `items` (`content`, `status`) | progress through the approved plan, resent after every write |
| `rpc` | `id`, `method`, `params` | run a session operation and reply with `rpc_result` |
| `done` | `session_id`, `cost_usd?`, `turns`, `mode?`, `provider?` | the turn finished; `cost_usd` is absent for Ollama |
| `error` | `message`, `code?` | the turn failed |
| `pong` | | |

`agent` names the specialist a frame came from, and is absent for the producer and the composer.

### A producer turn

```
user_message {mode: "producer"}  →   phase {planning}
                                     tool_call get_session … (reads only; writes are denied)
                                     plan_proposed {plan_id, plan}
                                     phase {awaiting_approval}      ← nothing is running
plan_decision {approve}          →   phase {executing}
                                     tool_call … (writes now allowed)
                                     done
```

`reject` returns the user's `notes` to the model, which revises and proposes again **in the same
turn** — no new user message, no lost context. Three sent-backs and the producer is told to stop.
`cancel`, a disconnect, or the turn ending settles a pending plan, so a parked plan can never
outlive its turn (`agent/src/producer.ts`, tested in `agent/src/producer.test.ts`).

The approval is enforced by a `PreToolUse` hook, not by the prompt: until the phase is `executing`,
every `mcp__flvstx__*` tool outside the read set is denied with a reason the model can act on.

### Specialists

In producer mode the run has subagents (`agent/src/specialists.ts`), each a fresh context with its
own craft prompt and a tool subset. Their frames carry `agent` — the `subagent_type`, taken from
`parent_tool_use_id` + `subagent_type` on the SDK message — and the plugin colours and labels the
transcript by it. `forwardSubagentText` is on, so a specialist's prose arrives as
`assistant_message` with `agent` set; deltas from a specialist are dropped rather than streamed, or
they would interleave into the producer's live bubble.

| specialist | writes |
|---|---|
| `harmony-form` | key, tempo, sections, chord progressions, `chords`, `pad` |
| `melody-topline` | `melody`, `counter_melody`, `harmony` (the vocal harmony line), lyrics |
| `rhythm-section` | `drums`, `percussion`, `bass`, `sub` |
| `arrangement-mix` | presence, instruments, energy, humanization — every layer |

`arrangement-mix` runs twice: a roster pass that creates the layers and sets who plays where before
anything is written, and a polish pass after everything is.

The same hook enforces those boundaries: a specialist's write to a layer outside its domain is
denied, naming the owner, and the run continues. Reads are never restricted — nobody can write music
they have not heard. A second hook rewrites `run_in_background` to false on every delegation:
subagents are backgrounded by default, and two agents writing to one session produce two songs.

### `plan`

```jsonc
{
  "summary": "…what this song will be, in a musician's words",
  "key": "F minor", "tempo": 82, "time_signature": "4/4", "style": "lofi",
  "form": [{ "name": "Intro", "bars": 8, "role": "intro" }],
  "layers": ["chords", "bass", "drums", "melody"],
  "steps": [{
    "id": "s1",
    "owner": "producer | harmony-form | melody-topline | rhythm-section | arrangement-mix",
    "title": "Drums and bass together",
    "detail": "one or two sentences: what and why",
    "targets": ["drums@verse", "bass@verse"]   // every layer@section it will write
  }],
  "risks": ["anything being overwritten, or a strong choice the user may not expect"],
  "revision": 1
}
```

`targets` are the point: approving a plan is approving a list of changes, not a paragraph of prose.

## RPC methods (see `crates/flvstx-core/src/ops.rs`)

`checkpoint`, `revert_to_checkpoint`, `get_session`, `get_notes`, `set_key_tempo`, `set_form`, `set_section`, `copy_section`, `set_chords`,
`suggest_chords`, `harmonize`, `set_notes`, `generate`, `generate_all`, `generate_song`, `humanize`,
`analyze`, `set_lyrics`, `lock`, `clear`, `transpose`, `undo`, `redo`, `export`, `add_layer`,
`remove_layer`, `set_arrangement`, `list_layer_kinds`, `set_instrument`, `read_reference`, `vary`,
`list_instruments`, `set_groove`, `list_scales`.

The checklist is derived, not reported: a step counts as done when every `layer@section` in its
`targets` appears in the ledger, so it cannot claim work nobody did. A step that writes nothing
addressable (form, key, a final check) settles when a later step lands, or when the run ends.

`propose_plan` and `verify_step` are not RPCs: they run inside the sidecar. `propose_plan` blocks
until the user answers; `verify_step` reads the run's write ledger — every tool call that actually
succeeded, with the agent that made it — so "did the specialist do it" is a tool result rather than
a self-report.

`checkpoint` and `revert_to_checkpoint` are the host's, not the model's: the plugin marks the
session when a plan is approved, because a run makes far more changes than undo's 64-deep ring.

Every mutating RPC snapshots the session for undo. Errors are returned as `{ok:false, error}` and
surfaced to the model as tool errors so it can correct itself.
