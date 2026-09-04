// Wire types shared with the Rust side (docs/protocol.md).

export type ClientMessage =
  | { type: "user_message"; text: string; context?: string; session_id?: string | null }
  | { type: "rpc_result"; id: number; ok: boolean; result?: unknown; error?: string }
  | { type: "cancel" }
  | { type: "ping" };

export type AgentMessage =
  | { type: "ready"; backend: "sdk" | "cli"; version: string }
  | { type: "assistant_delta"; text: string }
  | { type: "assistant_message"; text: string }
  | { type: "tool_call"; name: string; input: unknown }
  | { type: "tool_result"; name: string; summary: string }
  | { type: "rpc"; id: number; method: string; params: unknown }
  | { type: "done"; session_id: string; cost_usd?: number; turns: number }
  | { type: "error"; message: string }
  | { type: "pong" };

/** Something that can run session operations on the plugin (or CLI) side. */
export interface Rpc {
  call(method: string, params: unknown): Promise<unknown>;
}
