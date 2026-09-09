// The write fence and the plan handshake are the two things that make producer mode safe: nothing
// may change the song before the user approves, and no parked plan may outlive its turn. Both are
// pure logic, so they are tested here rather than by spending a model turn on them.

import assert from "node:assert/strict";
import { test } from "node:test";
import type { PreToolUseHookInput } from "@anthropic-ai/claude-agent-sdk";
import { decisionResult, foregroundAgents, MAX_REVISIONS, ProducerRun, toolFence } from "./producer.js";
import { DOMAIN, kindOf } from "./specialists.js";

function pre(tool: string, tool_input: Record<string, unknown> = {}, agent?: string): PreToolUseHookInput {
  // `agent_id` is what marks a call as a subagent's; `agent_type` alone is not enough.
  return { hook_event_name: "PreToolUse", tool_name: tool, tool_input, agent_id: agent && "a1", agent_type: agent } as PreToolUseHookInput;
}

const denied = (r: Record<string, any>) => r.hookSpecificOutput?.permissionDecision === "deny";

test("a write is denied until the plan is approved", async () => {
  const run = new ProducerRun();
  const fence = toolFence(run);
  run.phase = "planning";
  assert.ok(denied(await fence(pre("mcp__flvstx__set_key_tempo"))), "set_key_tempo before approval");
  assert.ok(denied(await fence(pre("mcp__flvstx__generate"))), "generate before approval");
  // Reading the song is how it plans at all, so reads are never fenced.
  assert.ok(!denied(await fence(pre("mcp__flvstx__get_session"))));
  assert.ok(!denied(await fence(pre("mcp__flvstx__analyze"))));
  assert.ok(!denied(await fence(pre("mcp__flvstx__propose_plan"))));
});

test("approval opens the fence, and only for this run", async () => {
  const run = new ProducerRun();
  const fence = toolFence(run);
  run.phase = "executing";
  assert.ok(!denied(await fence(pre("mcp__flvstx__set_key_tempo"))));
  run.reset();
  assert.ok(denied(await fence(pre("mcp__flvstx__set_key_tempo"))), "reset re-closes the fence");
});

test("the denial names the tool, so the model can act on it", async () => {
  const run = new ProducerRun();
  const out: any = await toolFence(run)(pre("mcp__flvstx__set_notes"));
  assert.match(out.hookSpecificOutput.permissionDecisionReason, /'set_notes'/);
  assert.match(out.hookSpecificOutput.permissionDecisionReason, /propose_plan/);
});

test("a plan parks until it is answered", async () => {
  const run = new ProducerRun();
  const pending = run.await_decision("p1");
  assert.equal(run.awaiting, true);
  assert.equal(run.phase, "awaiting_approval");
  assert.equal(run.settle("p1", { decision: "approve" }), true);
  assert.deepEqual(await pending, { decision: "approve" });
  assert.equal(run.awaiting, false);
});

test("a stale answer is refused, and a double answer cannot resolve twice", async () => {
  const run = new ProducerRun();
  const pending = run.await_decision("p2");
  assert.equal(run.settle("p1", { decision: "approve" }), false, "wrong id");
  assert.equal(run.settle("p2", { decision: "reject", notes: "slower" }), true);
  assert.equal(run.settle("p2", { decision: "approve" }), false, "already answered");
  assert.deepEqual(await pending, { decision: "reject", notes: "slower" });
});

test("nothing can leave a plan parked forever", async () => {
  for (const end of [(r: ProducerRun) => r.settleAll(), (r: ProducerRun) => r.reset()]) {
    const run = new ProducerRun();
    const pending = run.await_decision("p3");
    end(run);
    assert.deepEqual(await pending, { decision: "cancel" });
  }
});

test("the sent-back result carries the note, and stops repeating", () => {
  assert.match(decisionResult({ decision: "approve" }, 1), /APPROVED/);
  const back = decisionResult({ decision: "reject", notes: "100 BPM, drums and bass only" }, 1);
  assert.match(back, /100 BPM, drums and bass only/);
  assert.match(back, /propose_plan again/);
  // A note-less send-back still has to tell the model what to do.
  assert.match(decisionResult({ decision: "reject" }, 1), /ask for one/);
  const exhausted = decisionResult({ decision: "reject", notes: "again" }, MAX_REVISIONS);
  assert.match(exhausted, /Stop planning/);
  assert.match(decisionResult({ decision: "cancel" }, 1), /CANCELLED/);
});

test("a specialist may not write outside its own layers", async () => {
  const run = new ProducerRun();
  const fence = toolFence(run);
  run.phase = "executing";

  assert.ok(!denied(await fence(pre("mcp__flvstx__generate", { track: "chords" }, "harmony-form"))));
  assert.ok(!denied(await fence(pre("mcp__flvstx__generate", { track: "pad2" }, "harmony-form"))), "numbered ids belong to their kind");
  assert.ok(denied(await fence(pre("mcp__flvstx__generate", { track: "melody" }, "harmony-form"))), "the topline is not its to write");
  assert.ok(denied(await fence(pre("mcp__flvstx__set_notes", { track: "drums" }, "harmony-form"))));

  // The vocal harmony line is the topline writer's, however much it sounds like harmony.
  assert.ok(denied(await fence(pre("mcp__flvstx__generate", { track: "harmony" }, "harmony-form"))));
  assert.ok(!denied(await fence(pre("mcp__flvstx__generate", { track: "harmony" }, "melody-topline"))));

  // add_layer names a `kind`, not a `track`, and is fenced the same way.
  assert.ok(!denied(await fence(pre("mcp__flvstx__add_layer", { kind: "pad" }, "harmony-form"))));
  assert.ok(denied(await fence(pre("mcp__flvstx__add_layer", { kind: "sub" }, "harmony-form"))));

  // Section-level work carries no track, and the producer itself is never boundary-checked.
  assert.ok(!denied(await fence(pre("mcp__flvstx__set_form", {}, "harmony-form"))));
  assert.ok(!denied(await fence(pre("mcp__flvstx__generate", { track: "drums" }))));
});

test("the denial says who owns the layer, so the specialist can hand it back", async () => {
  const run = new ProducerRun();
  run.phase = "executing";
  const out: any = await toolFence(run)(pre("mcp__flvstx__generate", { track: "bass" }, "harmony-form"));
  const reason = out.hookSpecificOutput.permissionDecisionReason;
  assert.match(reason, /'bass' is not yours/);
  assert.match(reason, /chords, pad/);
});

test("every layer kind belongs to exactly one writer, apart from the arranger", () => {
  const owners = ["harmony-form", "melody-topline", "rhythm-section"];
  const counts = new Map<string, number>();
  for (const o of owners) for (const k of DOMAIN[o]) counts.set(k, (counts.get(k) ?? 0) + 1);
  for (const [kind, n] of counts) assert.equal(n, 1, `${kind} has ${n} owners`);
  // The arranger sees everything, and every other domain is inside it.
  for (const o of owners) for (const k of DOMAIN[o]) assert.ok(DOMAIN["arrangement-mix"].includes(k), k);
  assert.equal(kindOf("counter_melody2"), "counter_melody", "the longer name wins");
  assert.equal(kindOf("nonesuch"), undefined);
});

test("the ledger records what landed, and a new run starts empty", () => {
  const run = new ProducerRun();
  run.record({ agent: "harmony-form", tool: "generate", track: "chords", section: "verse" });
  run.record({ agent: "harmony-form", tool: "set_form" });
  run.record({ agent: "producer", tool: "generate", track: "chords", section: "verse" });
  assert.deepEqual(run.targetsTouched(), ["chords@verse"]);
  assert.equal(run.writes.length, 3);
  run.reset();
  assert.deepEqual(run.writes, []);
});

test("delegation never runs in the background", async () => {
  const rewrite: any = await foregroundAgents()(pre("Agent", { description: "chords", prompt: "…" }));
  assert.equal(rewrite.hookSpecificOutput.updatedInput.run_in_background, false);
  assert.equal(rewrite.hookSpecificOutput.updatedInput.prompt, "…", "the rest of the call is untouched");
  // Already in the foreground: nothing to say.
  const noop = await foregroundAgents()(pre("Agent", { run_in_background: false }));
  assert.deepEqual(noop, {});
});
