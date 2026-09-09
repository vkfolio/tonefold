// Wire types shared with the Rust side (docs/protocol.md).
//
// Compatibility rule, in both directions, forever: every field added to an existing message is
// optional here and `#[serde(default)]` in `crates/flvstx-ipc/src/lib.rs`. The plugin and the
// sidecar ship separately, and an old plugin must be able to read a new sidecar's frames.

export type Mode = "composer" | "producer";

/** Who does a plan step. "producer" means the producer does it itself. */
export type StepOwner = "producer" | "harmony-form" | "melody-topline" | "rhythm-section" | "arrangement-mix";

export interface PlanStep {
  /** Stable within a plan ("s1"); every later frame refers back to it. */
  id: string;
  owner: StepOwner;
  title: string;
  /** One or two sentences a musician would recognise, not a tool call. */
  detail: string;
  /** What it will write, as "layer@section" ("bass@verse", "drums@*"). */
  targets: string[];
}

export interface PlanSection {
  name: string;
  bars: number;
  role?: string;
}

export interface Plan {
  /** One paragraph: what the song will be. */
  summary: string;
  key?: string;
  tempo?: number;
  time_signature?: string;
  style?: string;
  form?: PlanSection[];
  layers?: string[];
  steps: PlanStep[];
  /** 1 on the first proposal, incremented each time the user sends it back. */
  revision: number;
  /** Anything the user should know before approving ("this replaces your chorus melody"). */
  risks?: string[];
}

export type PlanDecision = "approve" | "reject" | "cancel";

/** Where a producer turn is: nothing may be written before `executing`. */
export type Phase = "idle" | "planning" | "awaiting_approval" | "executing";

export type ClientMessage =
  | { type: "user_message"; text: string; context?: string; session_id?: string | null; model?: string | null; mode?: Mode }
  | { type: "plan_decision"; plan_id: string; decision: PlanDecision; notes?: string }
  | { type: "rpc_result"; id: number; ok: boolean; result?: unknown; error?: string }
  | { type: "cancel" }
  | { type: "ping" };

export type AgentMessage =
  | { type: "ready"; backend: "sdk" | "cli"; version: string; modes?: string[] }
  | { type: "assistant_delta"; text: string; agent?: string }
  | { type: "assistant_message"; text: string; agent?: string }
  | { type: "tool_call"; name: string; input: unknown; tool_use_id?: string; agent?: string }
  | { type: "tool_result"; name: string; summary: string; tool_use_id?: string; agent?: string; ok?: boolean }
  | { type: "rpc"; id: number; method: string; params: unknown }
  | { type: "done"; session_id: string; cost_usd?: number; turns: number; mode?: string }
  | { type: "error"; message: string; code?: string; agent?: string }
  | { type: "pong" }
  | { type: "plan_proposed"; plan_id: string; plan: Plan }
  | { type: "plan_resolved"; plan_id: string; decision: PlanDecision | "stale"; notes?: string }
  | { type: "phase"; phase: Phase };

/** Something that can run session operations on the plugin (or CLI) side. */
export interface Rpc {
  call(method: string, params: unknown): Promise<unknown>;
}
