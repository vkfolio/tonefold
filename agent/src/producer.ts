// Producer mode: the composer plans first, the user approves the plan, and only then may anything
// be written. The approval is enforced here rather than asked for in a prompt — a PreToolUse hook
// denies every mutating tool until the run reaches the `executing` phase.

import type { HookInput, PreToolUseHookInput } from "@anthropic-ai/claude-agent-sdk";
import type { Phase, Plan, PlanDecision } from "./protocol.js";
import { DOMAIN, kindOf } from "./specialists.js";

/** Tools that only read. Everything else in the flvstx server writes and needs an approved plan. */
export const READ_TOOLS = new Set([
  "get_session",
  "get_notes",
  "analyze",
  "read_reference",
  "list_layer_kinds",
  "list_instruments",
  "list_scales",
  "suggest_chords",
]);

export const MAX_REVISIONS = 3;

export interface PlanOutcome {
  decision: PlanDecision;
  notes?: string;
}

/** One change that actually landed. The producer verifies against this instead of a self-report. */
export interface WriteEntry {
  agent: string;
  tool: string;
  track?: string;
  section?: string;
}

/**
 * One producer turn's state. The phase is what the write fence reads, so it is the single place
 * that decides whether the session can change.
 */
export class ProducerRun {
  phase: Phase = "idle";
  revision = 0;
  planId: string | null = null;
  /** What has been written since the plan was approved, in order, attributed. */
  readonly writes: WriteEntry[] = [];
  private pending: { resolve: (o: PlanOutcome) => void } | null = null;

  /** Called when a tool has actually succeeded — an attempted write is not a write. */
  record(entry: WriteEntry) {
    this.writes.push(entry);
  }

  /** Everything written to a `layer@section` target, as the plan's steps name them. */
  targetsTouched(): string[] {
    const seen = new Set<string>();
    for (const w of this.writes) {
      if (w.track) seen.add(`${w.track}@${w.section ?? "*"}`);
    }
    return [...seen];
  }

  /** Parks until the user answers. Resolves (never rejects) so the tool always returns something. */
  await_decision(planId: string): Promise<PlanOutcome> {
    this.planId = planId;
    this.phase = "awaiting_approval";
    return new Promise((resolve) => {
      this.pending = { resolve };
    });
  }

  /** Answers a pending plan. Returns false when the id is stale (double click, old plan, reconnect). */
  settle(planId: string, outcome: PlanOutcome): boolean {
    if (!this.pending || this.planId !== planId) return false;
    const { resolve } = this.pending;
    this.pending = null;
    resolve(outcome);
    return true;
  }

  /** Ends any wait — used on cancel, disconnect and turn end so a parked tool can never hang. */
  settleAll(decision: PlanDecision = "cancel") {
    if (this.pending) {
      const { resolve } = this.pending;
      this.pending = null;
      resolve({ decision });
    }
  }

  get awaiting(): boolean {
    return this.pending !== null;
  }

  reset() {
    this.settleAll();
    this.phase = "idle";
    this.revision = 0;
    this.planId = null;
    this.writes.length = 0;
  }
}

/** What the model is told after the user answers. The wording is the producer's next instruction. */
export function decisionResult(outcome: PlanOutcome, revision: number): string {
  switch (outcome.decision) {
    case "approve":
      return (
        "APPROVED. This plan is now the contract. Work through the steps in order, and do not add " +
        "steps that are not in it — if you find you need one, do the planned work first and say so " +
        "in your final report."
      );
    case "reject":
      if (revision >= MAX_REVISIONS) {
        return (
          `The user has sent the plan back ${revision} times. Stop planning. Summarise in one short ` +
          "paragraph where you and the user disagree, and end your turn without writing anything."
        );
      }
      return (
        `PLAN SENT BACK (revision ${revision} of ${MAX_REVISIONS}). The user wants: ` +
        `${outcome.notes?.trim() || "(no note given — ask for one in a single short line)"}\n\n` +
        "Revise the plan and call propose_plan again. Do not write anything yet."
      );
    case "cancel":
    default:
      return "CANCELLED by the user. Stop now, write nothing, and end your turn with one short line.";
  }
}

const deny = (reason: string) => ({
  hookSpecificOutput: { hookEventName: "PreToolUse" as const, permissionDecision: "deny" as const, permissionDecisionReason: reason },
});

/** A refusal the user never sees is worth a line in the log; it is usually a brief that overreached. */
const denyLogged = (reason: string, log: (...a: unknown[]) => void) => {
  log(`fence: denied — ${reason}`);
  return deny(reason);
};

/**
 * The fence. Registered for `mcp__flvstx__*` only, so anything reaching it is one of our tools.
 * Reads are always allowed — nobody can write music they have not heard — and two rules apply to
 * everything else: nothing may change the session until the user has approved a plan, and no
 * specialist may write outside its own layers.
 */
export function toolFence(run: ProducerRun, log: (...a: unknown[]) => void = () => {}) {
  return async (input: HookInput) => {
    if (input.hook_event_name !== "PreToolUse") return {};
    const h = input as PreToolUseHookInput;
    log(`fence: ${h.tool_name} by ${h.agent_id ? h.agent_type : "producer"} phase=${run.phase}`);
    const tool = String(h.tool_name).replace(/^mcp__flvstx__/, "");
    if (READ_TOOLS.has(tool) || tool === "propose_plan") return {};
    if (run.phase !== "executing") {
      return denyLogged(
        `'${tool}' would change the song, and the plan has not been approved yet. ` +
          "Call propose_plan first and wait for the user's answer.",
        log,
      );
    }
    // `agent_id` is what distinguishes a subagent's call from the producer's own (sdk.d.ts:177).
    const agent = h.agent_id ? h.agent_type : undefined;
    const domain = agent ? DOMAIN[agent] : undefined;
    // `add_layer` and `remove_layer` name a `kind`; everything else names a `track` (an id or a
    // kind). Both resolve to a layer kind, which is what a domain is a list of.
    const args = (h.tool_input ?? {}) as { track?: unknown; kind?: unknown };
    const track = typeof args.track === "string" ? args.track : typeof args.kind === "string" ? args.kind : undefined;
    if (domain && track !== undefined) {
      const kind = kindOf(track);
      if (kind && !domain.includes(kind)) {
        return denyLogged(
          `'${track}' is not yours to write: ${agent} owns ${domain.join(", ")}. ` +
            "Do your part of the brief and tell the producer what the other layer needs — it will " +
            "hand that to whoever owns it.",
          log,
        );
      }
    }
    return {};
  };
}

/**
 * Subagents run in the background by default, so a step whose result the producer is about to use
 * would return immediately and the next step would write over a session still being changed. Every
 * delegation is made to block; parallelism, when we want it, will be an explicit decision.
 */
export function foregroundAgents() {
  return async (input: HookInput) => {
    if (input.hook_event_name !== "PreToolUse") return {};
    const h = input as PreToolUseHookInput;
    const inp = (h.tool_input ?? {}) as Record<string, unknown>;
    if (inp.run_in_background === false) return {};
    return {
      hookSpecificOutput: {
        hookEventName: "PreToolUse" as const,
        updatedInput: { ...inp, run_in_background: false },
      },
    };
  };
}
