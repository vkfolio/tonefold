// The write fence and the plan handshake are the two things that make producer mode safe: nothing
// may change the song before the user approves, and no parked plan may outlive its turn. Both are
// pure logic, so they are tested here rather than by spending a model turn on them.

import assert from "node:assert/strict";
import { test } from "node:test";
import type { PreToolUseHookInput } from "@anthropic-ai/claude-agent-sdk";
import { decisionResult, MAX_REVISIONS, ProducerRun, writeFence } from "./producer.js";

function pre(tool: string): PreToolUseHookInput {
  return { hook_event_name: "PreToolUse", tool_name: tool, tool_input: {} } as PreToolUseHookInput;
}

const denied = (r: Record<string, any>) => r.hookSpecificOutput?.permissionDecision === "deny";

test("a write is denied until the plan is approved", async () => {
  const run = new ProducerRun();
  const fence = writeFence(run);
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
  const fence = writeFence(run);
  run.phase = "executing";
  assert.ok(!denied(await fence(pre("mcp__flvstx__set_key_tempo"))));
  run.reset();
  assert.ok(denied(await fence(pre("mcp__flvstx__set_key_tempo"))), "reset re-closes the fence");
});

test("the denial names the tool, so the model can act on it", async () => {
  const run = new ProducerRun();
  const out: any = await writeFence(run)(pre("mcp__flvstx__set_notes"));
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
