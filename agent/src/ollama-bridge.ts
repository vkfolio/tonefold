// Anthropic Messages API <-> Ollama native chat API, as pure functions.
//
// Why not Ollama's own `/v1/messages`? Two things it cannot do, both fatal here. The context
// window cannot be set per request, so it silently truncates our ~10k-token prompt (system prompt
// plus 32 tool schemas) to the server's default of 2048 tokens and the model, having never seen a
// tool, invents one. And it rejects the CLI's trailing system-role message outright. The native
// `/api/chat` takes `options.num_ctx`, tools, thinking and tool results directly, so the shim
// (ollama-shim.ts) translates in both directions with what is below.

// --- Anthropic side ----------------------------------------------------------------------------

export type ABlock = {
  type: string;
  text?: string;
  thinking?: string;
  id?: string;
  name?: string;
  input?: unknown;
  tool_use_id?: string;
  content?: string | ABlock[];
  is_error?: boolean;
  [k: string]: unknown;
};
export type AMessage = { role: string; content: string | ABlock[] };
export type ATool = { name: string; description?: string; input_schema?: Record<string, unknown>; type?: string };
export interface ARequest {
  model: string;
  system?: string | ABlock[];
  messages: AMessage[];
  tools?: ATool[];
  max_tokens?: number;
  temperature?: number;
  top_p?: number;
  stop_sequences?: string[];
  stream?: boolean;
  thinking?: { type?: string };
  [k: string]: unknown;
}
export interface AMessageOut {
  id: string;
  type: "message";
  role: "assistant";
  model: string;
  content: ABlock[];
  stop_reason: "end_turn" | "tool_use" | "max_tokens";
  stop_sequence: null;
  usage: { input_tokens: number; output_tokens: number };
}

// --- Ollama side -------------------------------------------------------------------------------

export type OToolCall = { function: { name: string; arguments: unknown; index?: number } };
export type OMessage = { role: string; content: string; thinking?: string; tool_calls?: OToolCall[]; tool_name?: string };
export interface OChatRequest {
  model: string;
  messages: OMessage[];
  tools?: { type: "function"; function: { name: string; description?: string; parameters?: Record<string, unknown> } }[];
  stream: boolean;
  think?: boolean;
  options: Record<string, unknown>;
}
/** One NDJSON line of a streamed `/api/chat` reply, or the whole reply when not streaming. */
export interface OChunk {
  message?: Partial<OMessage>;
  done?: boolean;
  done_reason?: string;
  prompt_eval_count?: number;
  eval_count?: number;
  error?: string;
}

export interface BridgeOptions {
  /** Context window to run the model with. Ollama's default (2048 or 4096) loses our prompt. */
  numCtx: number;
  /**
   * Whether the model may think before answering. Undefined follows the request; the sidecar
   * passes false by default, because a thinking local model spends minutes on a paragraph of
   * deliberation before its first tool call, and the composer's tools do the hard part anyway.
   */
  think?: boolean;
}

function textOf(content: string | ABlock[] | undefined): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .map((b) => (b.type === "text" ? b.text ?? "" : b.type === "image" ? "[image omitted]" : ""))
    .filter(Boolean)
    .join("\n");
}

/**
 * The native request for an Anthropic one. System text — the `system` field and any system-role
 * messages the CLI slipped into `messages` — becomes one leading system message; tool results
 * become tool-role messages named after the call they answer; assistant tool calls and thinking
 * take Ollama's fields.
 */
export function toOllamaChat(req: ARequest, opts: BridgeOptions): OChatRequest {
  const system: string[] = [];
  const s = textOf(req.system);
  if (s) system.push(s);
  const messages: OMessage[] = [];
  const toolNames = new Map<string, string>();
  for (const m of req.messages ?? []) {
    if (m.role === "system") {
      const t = textOf(m.content);
      if (t) system.push(t);
      continue;
    }
    if (m.role === "assistant") {
      const out: OMessage = { role: "assistant", content: "" };
      if (typeof m.content === "string") out.content = m.content;
      else {
        const text: string[] = [];
        const think: string[] = [];
        for (const b of m.content) {
          if (b.type === "text" && b.text) text.push(b.text);
          else if (b.type === "thinking" && b.thinking) think.push(b.thinking);
          else if (b.type === "tool_use") {
            if (b.id && b.name) toolNames.set(b.id, b.name);
            (out.tool_calls ??= []).push({ function: { name: b.name ?? "", arguments: b.input ?? {} } });
          }
        }
        out.content = text.join("\n");
        if (think.length) out.thinking = think.join("\n");
      }
      messages.push(out);
      continue;
    }
    // user
    if (typeof m.content === "string") {
      messages.push({ role: "user", content: m.content });
      continue;
    }
    const text: string[] = [];
    for (const b of m.content) {
      if (b.type === "tool_result") {
        const body = textOf(b.content) || (b.is_error ? "error" : "ok");
        messages.push({ role: "tool", content: b.is_error ? `ERROR: ${body}` : body, tool_name: toolNames.get(b.tool_use_id ?? "") ?? undefined });
      } else if (b.type === "text" && b.text) text.push(b.text);
      else if (b.type === "image") text.push("[image omitted]");
    }
    if (text.length) messages.push({ role: "user", content: text.join("\n") });
  }
  if (system.length) messages.unshift({ role: "system", content: system.join("\n\n") });

  const options: Record<string, unknown> = { num_ctx: opts.numCtx };
  if (typeof req.max_tokens === "number") options.num_predict = req.max_tokens;
  if (typeof req.temperature === "number") options.temperature = req.temperature;
  if (typeof req.top_p === "number") options.top_p = req.top_p;
  if (req.stop_sequences?.length) options.stop = req.stop_sequences;

  const out: OChatRequest = { model: req.model, messages, stream: req.stream !== false, options };
  const tools = (req.tools ?? []).filter((t) => t.input_schema);
  if (tools.length) {
    out.tools = tools.map((t) => {
      const { $schema: _drop, ...parameters } = t.input_schema as Record<string, unknown>;
      return { type: "function", function: { name: t.name, description: t.description, parameters } };
    });
  }
  const think = req.thinking?.type;
  if (opts.think !== undefined) out.think = opts.think;
  else if (think === "enabled" || think === "adaptive") out.think = true;
  else if (think === "disabled") out.think = false;
  return out;
}

let counter = 0;
const newId = (prefix: string) => `${prefix}_${Date.now().toString(36)}${(counter++).toString(36)}`;

function stopReason(c: OChunk, sawTool: boolean): AMessageOut["stop_reason"] {
  if (sawTool) return "tool_use";
  return c.done_reason === "length" ? "max_tokens" : "end_turn";
}

/** The Anthropic message for a non-streamed reply. */
export function fromOllamaFinal(c: OChunk, model: string): AMessageOut {
  const content: ABlock[] = [];
  const m = c.message ?? {};
  if (m.thinking) content.push({ type: "thinking", thinking: m.thinking, signature: "" });
  if (m.content) content.push({ type: "text", text: m.content });
  for (const t of m.tool_calls ?? []) {
    content.push({ type: "tool_use", id: newId("toolu"), name: t.function.name, input: t.function.arguments ?? {} });
  }
  return {
    id: newId("msg"),
    type: "message",
    role: "assistant",
    model,
    content,
    stop_reason: stopReason(c, (m.tool_calls ?? []).length > 0),
    stop_sequence: null,
    usage: { input_tokens: c.prompt_eval_count ?? 0, output_tokens: c.eval_count ?? 0 },
  };
}

const sse = (event: string, data: unknown) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;

/**
 * Turns streamed Ollama chunks into Anthropic server-sent events. Thinking and text arrive as
 * deltas and become thinking/text blocks; a tool call arrives whole and becomes a tool_use block
 * with one input_json_delta. Feed every chunk in order; `start()` first, and the chunk with
 * `done: true` ends the message.
 */
export class SseTranslator {
  private index = -1;
  private open: "thinking" | "text" | null = null;
  private sawTool = false;
  constructor(private model: string) {}

  start(): string {
    return sse("message_start", {
      type: "message_start",
      message: { id: newId("msg"), type: "message", role: "assistant", model: this.model, content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 0, output_tokens: 0 } },
    });
  }

  private close(): string {
    if (this.open === null) return "";
    const out = sse("content_block_stop", { type: "content_block_stop", index: this.index });
    this.open = null;
    return out;
  }

  private ensure(kind: "thinking" | "text"): string {
    if (this.open === kind) return "";
    let out = this.close();
    this.index += 1;
    this.open = kind;
    const block = kind === "thinking" ? { type: "thinking", thinking: "" } : { type: "text", text: "" };
    out += sse("content_block_start", { type: "content_block_start", index: this.index, content_block: block });
    return out;
  }

  feed(c: OChunk): string {
    let out = "";
    if (c.error) {
      out += this.close();
      out += sse("error", { type: "error", error: { type: "api_error", message: c.error } });
      return out;
    }
    const m = c.message ?? {};
    if (m.thinking) {
      out += this.ensure("thinking");
      out += sse("content_block_delta", { type: "content_block_delta", index: this.index, delta: { type: "thinking_delta", thinking: m.thinking } });
    }
    if (m.content) {
      out += this.ensure("text");
      out += sse("content_block_delta", { type: "content_block_delta", index: this.index, delta: { type: "text_delta", text: m.content } });
    }
    for (const t of m.tool_calls ?? []) {
      this.sawTool = true;
      out += this.close();
      this.index += 1;
      out += sse("content_block_start", { type: "content_block_start", index: this.index, content_block: { type: "tool_use", id: newId("toolu"), name: t.function.name, input: {} } });
      out += sse("content_block_delta", { type: "content_block_delta", index: this.index, delta: { type: "input_json_delta", partial_json: JSON.stringify(t.function.arguments ?? {}) } });
      out += sse("content_block_stop", { type: "content_block_stop", index: this.index });
    }
    if (c.done) {
      out += this.close();
      out += sse("message_delta", {
        type: "message_delta",
        delta: { stop_reason: stopReason(c, this.sawTool), stop_sequence: null },
        usage: { input_tokens: c.prompt_eval_count ?? 0, output_tokens: c.eval_count ?? 0 },
      });
      out += sse("message_stop", { type: "message_stop" });
    }
    return out;
  }
}

/** A rough token count for `/v1/messages/count_tokens`; the CLI only uses it for budgeting. */
export function estimateTokens(req: ARequest): number {
  const chars = textOf(req.system).length + (req.messages ?? []).reduce((n, m) => n + JSON.stringify(m.content).length, 0) + JSON.stringify(req.tools ?? []).length;
  return Math.ceil(chars / 4);
}
