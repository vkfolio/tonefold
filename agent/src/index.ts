// FLVSTX agent sidecar: WebSocket server that runs a Claude Agent SDK session with composer tools.
// The plugin (or flvstx-cli chat) connects, sends user messages, and answers RPCs (docs/protocol.md).
import { WebSocketServer, WebSocket } from "ws";
import { query, type SDKUserMessage, type Query } from "@anthropic-ai/claude-agent-sdk";
import { makeServer, TOOL_NAMES } from "./tools.js";
import { systemPrompt, AGENT_ROOT } from "./prompt.js";
import type { AgentMessage, ClientMessage, Rpc } from "./protocol.js";

const VERSION = "0.1.0";
const args = process.argv.slice(2);
const flag = (n: string) => {
  const i = args.indexOf(n);
  return i >= 0 ? args[i + 1] : undefined;
};
const PORT = Number(flag("--port") ?? process.env.FLVSTX_PORT ?? 7878);
const MODEL = flag("--model") ?? process.env.FLVSTX_MODEL; // undefined = CLI default
const MAX_TURNS = Number(flag("--max-turns") ?? 40);

const log = (...a: unknown[]) => console.error(`[flvstx-agent ${new Date().toISOString().slice(11, 19)}]`, ...a);

/** Async queue used as the SDK's streaming prompt input. */
class Inbox implements AsyncIterable<SDKUserMessage> {
  private items: SDKUserMessage[] = [];
  private waiters: ((v: IteratorResult<SDKUserMessage>) => void)[] = [];
  private closed = false;
  push(text: string) {
    const m: SDKUserMessage = { type: "user", message: { role: "user", content: text }, parent_tool_use_id: null, session_id: "" };
    const w = this.waiters.shift();
    if (w) w({ value: m, done: false });
    else this.items.push(m);
  }
  close() {
    this.closed = true;
    for (const w of this.waiters.splice(0)) w({ value: undefined as never, done: true });
  }
  [Symbol.asyncIterator](): AsyncIterator<SDKUserMessage> {
    return {
      next: () => {
        const it = this.items.shift();
        if (it) return Promise.resolve({ value: it, done: false });
        if (this.closed) return Promise.resolve({ value: undefined as never, done: true });
        return new Promise((res) => this.waiters.push(res));
      },
    };
  }
}

class Connection implements Rpc {
  private nextId = 1;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  private inbox = new Inbox();
  private q: Query | null = null;
  private running = false;
  private turnActive = false;
  private sessionId = "";
  private lastContext = "";
  private model: string | undefined = MODEL;

  constructor(private ws: WebSocket) {
    ws.on("message", (data) => this.onMessage(data.toString()));
    ws.on("close", () => this.dispose());
    ws.on("error", (e) => log("ws error", e.message));
    this.send({ type: "ready", backend: "sdk", version: VERSION });
  }

  send(m: AgentMessage) {
    if (this.ws.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(m));
  }

  call(method: string, params: unknown): Promise<unknown> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.send({ type: "rpc", id, method, params });
      setTimeout(() => {
        if (this.pending.delete(id)) reject(new Error(`rpc ${method} timed out`));
      }, 30_000);
    });
  }

  private onMessage(raw: string) {
    let m: ClientMessage;
    try {
      m = JSON.parse(raw);
    } catch {
      return this.send({ type: "error", message: "bad json" });
    }
    switch (m.type) {
      case "ping":
        return this.send({ type: "pong" });
      case "rpc_result": {
        const p = this.pending.get(m.id);
        if (!p) return;
        this.pending.delete(m.id);
        if (m.ok) p.resolve(m.result);
        else p.reject(new Error(m.error ?? "rpc failed"));
        return;
      }
      case "cancel":
        if (this.q && this.turnActive) {
          this.q.interrupt().catch((e) => log("interrupt failed", e));
        }
        return;
      case "user_message": {
        if (this.turnActive) return this.send({ type: "error", message: "a turn is already running; cancel it first" });
        if (m.session_id && !this.sessionId) this.sessionId = m.session_id;
        this.lastContext = m.context ?? "";
        const wanted = m.model && m.model !== "default" ? m.model : MODEL;
        if (wanted !== this.model) {
          // Model change: end the current SDK session; the next one resumes the conversation with the new model.
          this.model = wanted;
          if (this.q) {
            log(`switching model to ${wanted ?? "default"}`);
            this.inbox.close();
            this.q.interrupt().catch(() => {});
            this.q = null;
            this.running = false;
            this.inbox = new Inbox();
          }
        }
        this.turnActive = true;
        const text = m.context ? `<session_state>\n${m.context}\n</session_state>\n\n${m.text}` : m.text;
        this.ensureRunning();
        this.inbox.push(text);
        return;
      }
    }
  }

  private ensureRunning() {
    if (this.running) return;
    this.running = true;
    this.runLoop().catch((e) => {
      log("agent loop failed", e);
      this.send({ type: "error", message: e instanceof Error ? e.message : String(e) });
      this.running = false;
      this.turnActive = false;
    });
  }

  private async runLoop() {
    const server = makeServer(this, (name, input, result, ok) => {
      this.send({ type: "tool_result", name, summary: ok ? result.slice(0, 400) : `ERROR: ${result.slice(0, 400)}` });
    });
    const resume = this.sessionId || undefined;
    log(`starting SDK session${resume ? ` (resume ${resume})` : ""} model=${this.model ?? "default"}`);
    this.q = query({
      prompt: this.inbox,
      options: {
        systemPrompt: systemPrompt(),
        model: this.model,
        maxTurns: MAX_TURNS,
        cwd: AGENT_ROOT,
        mcpServers: { flvstx: server },
        allowedTools: TOOL_NAMES.map((t) => `mcp__flvstx__${t}`),
        disallowedTools: ["Bash", "Read", "Write", "Edit", "MultiEdit", "Glob", "Grep", "WebSearch", "WebFetch", "Task", "NotebookEdit", "TodoWrite", "Agent"],
        permissionMode: "bypassPermissions",
        allowDangerouslySkipPermissions: true,
        includePartialMessages: true,
        resume,
        settingSources: [],
      },
    });
    let turns = 0;
    let streamedText = "";
    for await (const msg of this.q) {
      const any = msg as any;
      switch (msg.type) {
        case "system":
          if (any.subtype === "init") {
            log(`init model=${any.model} apiKeySource=${any.apiKeySource} session=${any.session_id}`);
            log(`mcp_servers=${JSON.stringify(any.mcp_servers)} tools=${(any.tools ?? []).filter((t: string) => t.includes("flvstx")).join(",") || "(no flvstx tools)"} all=${(any.tools ?? []).length}`);
          }
          break;
        case "stream_event": {
          const ev = any.event;
          if (ev?.type === "content_block_delta" && ev.delta?.type === "text_delta") {
            streamedText += ev.delta.text;
            this.send({ type: "assistant_delta", text: ev.delta.text });
          }
          break;
        }
        case "assistant": {
          for (const b of any.message?.content ?? []) {
            if (b.type === "tool_use") {
              const name = String(b.name).replace(/^mcp__flvstx__/, "");
              if (name === "ToolSearch") continue;
              this.send({ type: "tool_call", name, input: b.input });
            } else if (b.type === "text" && b.text) {
              this.send({ type: "assistant_message", text: b.text });
              streamedText = "";
            }
          }
          break;
        }
        case "result": {
          turns = any.num_turns ?? turns;
          this.sessionId = any.session_id ?? this.sessionId;
          this.turnActive = false;
          if (any.subtype === "success") {
            this.send({ type: "done", session_id: this.sessionId, cost_usd: any.total_cost_usd, turns });
          } else {
            const err = any.errors?.join("; ") || any.subtype || "unknown error";
            log("turn ended with", any.subtype, err);
            this.send({ type: "error", message: `turn ended: ${err}` });
            this.send({ type: "done", session_id: this.sessionId, cost_usd: any.total_cost_usd, turns });
          }
          break;
        }
        default:
          break;
      }
    }
    log("SDK session ended");
    this.running = false;
    this.turnActive = false;
    this.q = null;
    // A fresh inbox for the next turn (the old one was consumed).
    this.inbox = new Inbox();
  }

  private dispose() {
    this.inbox.close();
    if (this.q) this.q.interrupt().catch(() => {});
    for (const p of this.pending.values()) p.reject(new Error("connection closed"));
    this.pending.clear();
    log("client disconnected");
  }
}

const wss = new WebSocketServer({ host: "127.0.0.1", port: PORT });
wss.on("listening", () => log(`listening on ws://127.0.0.1:${PORT}`));
wss.on("connection", (ws) => {
  log("client connected");
  new Connection(ws);
});
wss.on("error", (e) => {
  log("server error", e.message);
  process.exit(1);
});
process.on("SIGINT", () => process.exit(0));
process.on("SIGTERM", () => process.exit(0));
