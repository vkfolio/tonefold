// Wire types shared with the Rust side (docs/protocol.md).

export type ClientMessage =
  | { type: "user_message"; text: string; context?: string; session_id?: string | null; model?: string | null }
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
  | { type: "pong" };

/** Something that can run session operations on the plugin (or CLI) side. */
export interface Rpc {
  call(method: string, params: unknown): Promise<unknown>;
}
