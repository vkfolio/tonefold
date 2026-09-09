// FLVSTX agent sidecar: WebSocket server that runs a Claude Agent SDK session with composer tools.
// The plugin (or flvstx-cli chat) connects, sends user messages, and answers RPCs (docs/protocol.md).
import { WebSocketServer, WebSocket } from "ws";
import { query, type SDKUserMessage, type Query } from "@anthropic-ai/claude-agent-sdk";
import { makeServer, TOOL_NAMES } from "./tools.js";
import { systemPrompt, AGENT_ROOT } from "./prompt.js";
import type { AgentMessage, ClientMessage, Mode, Phase, Plan, Rpc } from "./protocol.js";
import { checklist, decisionResult, foregroundAgents, ProducerRun, READ_TOOLS, toolFence } from "./producer.js";
import { SPECIALISTS } from "./specialists.js";

const VERSION = "1.2.0";
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

/** Which agent produced a message: a subagent's type, or undefined for the main thread. */
function authorOf(msg: { parent_tool_use_id?: string | null; subagent_type?: string }): string | undefined {
  return msg.parent_tool_use_id ? msg.subagent_type ?? "specialist" : undefined;
}

class Connection implements Rpc {
  private nextId = 1;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  private inbox = new Inbox();
  private q: Query | null = null;
  private running = false;
  private turnActive = false;
  private lastContext = "";
  private model: string | undefined = MODEL;
  private mode: Mode = "composer";
  /** One conversation per mode: their tools and personas differ, so they cannot share a session. */
  private sessionIds: Record<Mode, string> = { composer: "", producer: "" };
  private run = new ProducerRun();
  /** tool_use id -> the subagent that made the call, so its result can be attributed too. */
  private agentOf = new Map<string, string | undefined>();

  constructor(private ws: WebSocket) {
    ws.on("message", (data) => this.onMessage(data.toString()));
    ws.on("close", () => this.dispose());
    ws.on("error", (e) => log("ws error", e.message));
    this.send({ type: "ready", backend: "sdk", version: VERSION, modes: ["composer", "producer"] });
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
        // Settle a parked plan first: interrupting while the tool handler is awaiting would tear
        // down the call and leave the promise alive forever.
        this.run.settleAll("cancel");
        if (this.q && this.turnActive) {
          this.q.interrupt().catch((e) => log("interrupt failed", e));
        }
        return;
      case "plan_decision": {
        // Never a user_message: the producer is parked inside a tool call, so the one-turn gate
        // would reject it, and queueing it as the next turn would deadlock.
        const settled = this.run.settle(m.plan_id, { decision: m.decision, notes: m.notes });
        this.send({ type: "plan_resolved", plan_id: m.plan_id, decision: settled ? m.decision : "stale", notes: m.notes });
        return;
      }
      case "user_message": {
        if (this.turnActive) return this.send({ type: "error", message: "a turn is already running; cancel it first" });
        if (m.session_id && !this.sessionIds[this.mode]) this.sessionIds[this.mode] = m.session_id;
        this.lastContext = m.context ?? "";
        const wantedMode: Mode = m.mode === "producer" ? "producer" : "composer";
        const wanted = m.model && m.model !== "default" ? m.model : MODEL;
        // The two modes differ in tools, hooks and persona — all frozen when the query is created —
        // so a mode change ends the current session and starts the other one, which resumes its own
        // conversation by id.
        if (wantedMode !== this.mode || wanted !== this.model) {
          this.mode = wantedMode;
          // Model change: end the current SDK session; the next one resumes the conversation with the new model.
          this.model = wanted;
          if (this.q) {
            log(`restarting session: mode=${this.mode} model=${wanted ?? "default"}`);
            this.inbox.close();
            this.q.interrupt().catch(() => {});
            this.q = null;
            this.running = false;
            this.inbox = new Inbox();
          }
        }
        this.turnActive = true;
        if (this.mode === "producer") this.setPhase("planning");
        const text = m.context ? `<session_state>\n${m.context}\n</session_state>\n\n${m.text}` : m.text;
        this.ensureRunning();
        this.inbox.push(text);
        return;
      }
    }
  }

  /** Progress, derived from the ledger — never from a claim that a step is done. */
  private sendChecklist(finished = false) {
    if (!this.run.plan) return;
    this.send({ type: "todos", items: checklist(this.run.plan, this.run.writes, finished) });
  }

  private setPhase(phase: Phase) {
    this.run.phase = phase;
    this.send({ type: "phase", phase });
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
    const producer = this.mode === "producer";
    const reviewPlan = producer
      ? async (plan: Plan) => {
          const planId = `plan-${Date.now().toString(36)}`;
          this.run.revision += 1;
          const revised: Plan = { ...plan, revision: this.run.revision };
          // Register the wait before announcing the plan: an answer that arrives in the same tick
          // would otherwise find nothing pending and be reported as stale.
          const answered = this.run.await_decision(planId);
          this.send({ type: "plan_proposed", plan_id: planId, plan: revised });
          this.send({ type: "phase", phase: "awaiting_approval" });
          const outcome = await answered;
          if (outcome.decision === "approve") {
            this.run.plan = revised;
            this.sendChecklist();
          }
          this.setPhase(outcome.decision === "approve" ? "executing" : "planning");
          return decisionResult(outcome, this.run.revision);
        }
      : undefined;
    const server = makeServer(
      this,
      (name, input, result, ok, toolUseId) => {
      // The hook knows the caller for certain; the message pump only knows it when the SDK gave the
      // block an id we saw. Prefer the hook.
      const agent = this.run.callerOf(name) ?? (toolUseId ? this.agentOf.get(toolUseId) : undefined);
      // The ledger records what landed, not what was attempted — hence here and not in the hook.
      // Reads are not writes: a specialist that only looked at a layer has not written it, and
      // verify_step would otherwise call the step done.
      if (ok && producer && !READ_TOOLS.has(name) && name !== "propose_plan") {
        // `add_layer` names a `kind`; everything else names a `track`.
        const p = (input ?? {}) as { track?: unknown; kind?: unknown; section?: unknown };
        this.run.record({
          agent: agent ?? "producer",
          tool: name,
          track: typeof p.track === "string" ? p.track : typeof p.kind === "string" ? p.kind : undefined,
          section: typeof p.section === "string" ? p.section : undefined,
        });
        this.sendChecklist();
      }
      this.send({
        type: "tool_result",
        name,
        summary: ok ? result.slice(0, 400) : `ERROR: ${result.slice(0, 400)}`,
        tool_use_id: toolUseId,
        agent,
        ok,
      });
      },
      reviewPlan ? { reviewPlan, ledger: () => this.run.writes } : undefined,
    );
    const resume = this.sessionIds[this.mode] || undefined;
    log(`starting SDK session${resume ? ` (resume ${resume})` : ""} model=${this.model ?? "default"}`);
    this.q = query({
      prompt: this.inbox,
      options: {
        systemPrompt: systemPrompt(this.mode),
        model: this.model,
        maxTurns: MAX_TURNS,
        cwd: AGENT_ROOT,
        mcpServers: { flvstx: server },
        allowedTools: [
          ...[...TOOL_NAMES, ...(producer ? ["propose_plan", "verify_step"] : [])].map((t) => `mcp__flvstx__${t}`),
          // Both spellings: the delegation tool has been called each at different SDK versions.
          ...(producer ? ["Agent", "Task"] : []),
        ],
        // `allowedTools` only auto-approves — `tools` is what decides which built-ins exist at all
        // (sdk.d.ts). Without it the producer also gets SendMessage, TaskOutput and the rest, and it
        // will reach for them: in testing it tried to message a specialist that had hit its turn
        // limit instead of re-delegating the work that was left.
        tools: producer ? ["Agent", "Task"] : [],
        disallowedTools: [
          "Bash", "Read", "Write", "Edit", "MultiEdit", "Glob", "Grep", "WebSearch", "WebFetch", "NotebookEdit",
          "TodoWrite",
          ...(producer ? [] : ["Task", "Agent"]),
        ],
        // The specialists. `model` is absent from every definition, so the panel's picker governs
        // the whole run.
        agents: producer ? SPECIALISTS : undefined,
        // Both rules the design depends on are enforced here rather than asked for in a prompt:
        // nothing is written before the user approves, and no specialist writes outside its layers.
        hooks: producer
          ? {
              PreToolUse: [
                { matcher: "mcp__flvstx__.*", hooks: [toolFence(this.run, log)], timeout: 10 },
                { matcher: "Agent|Task", hooks: [foregroundAgents()], timeout: 10 },
              ],
            }
          : undefined,
        forwardSubagentText: true,
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
          if (any.subtype === "task_started" && any.subagent_type) {
            this.send({ type: "assistant_message", text: `_${any.description ?? "working"}_`, agent: any.subagent_type });
          }
          if (any.subtype === "init") {
            log(`init model=${any.model} apiKeySource=${any.apiKeySource} session=${any.session_id}`);
            const all: string[] = any.tools ?? [];
            const ours = all.filter((t) => t.includes("flvstx"));
            // The built-ins matter as much as ours: anything unexpected here is a tool the
            // model can reach for and we did not mean it to have.
            log(`mcp_servers=${JSON.stringify(any.mcp_servers)} flvstx=${ours.length} builtin=[${all.filter((t) => !t.includes("flvstx")).join(",")}]`);
          }
          break;
        case "stream_event": {
          const ev = any.event;
          if (ev?.type === "content_block_delta" && ev.delta?.type === "text_delta") {
            // A forwarded subagent's prose must not interleave into the main bubble character by
            // character, so deltas carry the author and the UI decides where to put them.
            streamedText += ev.delta.text;
            this.send({ type: "assistant_delta", text: ev.delta.text, agent: authorOf(any) });
          }
          break;
        }
        case "assistant": {
          const agent = authorOf(any);
          for (const b of any.message?.content ?? []) {
            if (b.type === "tool_use") {
              const name = String(b.name).replace(/^mcp__flvstx__/, "");
              if (name === "ToolSearch") continue;
              // Remember who owns this call so its result can be attributed too.
              if (b.id) this.agentOf.set(b.id, agent);
              this.send({ type: "tool_call", name, input: b.input, tool_use_id: b.id, agent });
            } else if (b.type === "text" && b.text) {
              this.send({ type: "assistant_message", text: b.text, agent });
              streamedText = "";
            }
          }
          break;
        }
        case "result": {
          turns = any.num_turns ?? turns;
          this.sessionIds[this.mode] = any.session_id ?? this.sessionIds[this.mode];
          this.turnActive = false;
          if (any.subtype === "success") {
            this.send({ type: "done", session_id: this.sessionIds[this.mode], cost_usd: any.total_cost_usd, turns, mode: this.mode });
          } else {
            const err = any.errors?.join("; ") || any.subtype || "unknown error";
            log("turn ended with", any.subtype, err);
            this.send({ type: "error", message: `turn ended: ${err}` });
            this.send({ type: "done", session_id: this.sessionIds[this.mode], cost_usd: any.total_cost_usd, turns, mode: this.mode });
          }
          break;
        }
        default:
          // Everything else — task lifecycle, tool progress, hooks, compaction. Producer mode turns
          // several of these into real events; until then, log the shapes so the next phase is
          // written against what actually arrives rather than against the type union.
          log(`sdk message: ${msg.type}${any.subtype ? `/${any.subtype}` : ""}${any.parent_tool_use_id ? " (subagent)" : ""}`);
          break;
      }
    }
    log("SDK session ended");
    this.running = false;
    this.turnActive = false;
    if (this.mode === "producer") {
      this.sendChecklist(true);
      this.send({ type: "phase", phase: "idle" });
    }
    this.run.reset();
    this.q = null;
    // A fresh inbox for the next turn (the old one was consumed).
    this.inbox = new Inbox();
  }

  private dispose() {
    // A parked plan would otherwise keep its tool call — and the turn — alive forever.
    this.run.settleAll("cancel");
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
