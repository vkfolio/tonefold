// The write fence and the plan handshake are the two things that make producer mode safe: nothing
// may change the song before the user approves, and no parked plan may outlive its turn. Both are
// pure logic, so they are tested here rather than by spending a model turn on them.

import assert from "node:assert/strict";
import { test } from "node:test";
import type { PreToolUseHookInput } from "@anthropic-ai/claude-agent-sdk";
import { checklist, decisionResult, foregroundAgents, MAX_REVISIONS, ProducerRun, READ_TOOLS, toolFence } from "./producer.js";
import { matches, type WriteEntry } from "./producer.js";
import type { Plan } from "./protocol.js";
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
  assert.ok(denied(await fence(pre("mcp__tonefold__set_key_tempo"))), "set_key_tempo before approval");
  assert.ok(denied(await fence(pre("mcp__tonefold__generate"))), "generate before approval");
  // Reading the song is how it plans at all, so reads are never fenced.
  assert.ok(!denied(await fence(pre("mcp__tonefold__get_session"))));
  assert.ok(!denied(await fence(pre("mcp__tonefold__analyze"))));
  assert.ok(!denied(await fence(pre("mcp__tonefold__propose_plan"))));
});

test("approval opens the fence, and only for this run", async () => {
  const run = new ProducerRun();
  const fence = toolFence(run);
  run.phase = "executing";
  assert.ok(!denied(await fence(pre("mcp__tonefold__set_key_tempo"))));
  run.reset();
  assert.ok(denied(await fence(pre("mcp__tonefold__set_key_tempo"))), "reset re-closes the fence");
});

test("the denial names the tool, so the model can act on it", async () => {
  const run = new ProducerRun();
  const out: any = await toolFence(run)(pre("mcp__tonefold__set_notes"));
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

  assert.ok(!denied(await fence(pre("mcp__tonefold__generate", { track: "chords" }, "harmony-form"))));
  assert.ok(!denied(await fence(pre("mcp__tonefold__generate", { track: "pad2" }, "harmony-form"))), "numbered ids belong to their kind");
  assert.ok(denied(await fence(pre("mcp__tonefold__generate", { track: "melody" }, "harmony-form"))), "the topline is not its to write");
  assert.ok(denied(await fence(pre("mcp__tonefold__set_notes", { track: "drums" }, "harmony-form"))));

  // The vocal harmony line is the topline writer's, however much it sounds like harmony.
  assert.ok(denied(await fence(pre("mcp__tonefold__generate", { track: "harmony" }, "harmony-form"))));
  assert.ok(!denied(await fence(pre("mcp__tonefold__generate", { track: "harmony" }, "melody-topline"))));

  // add_layer names a `kind`, not a `track`, and is fenced the same way.
  assert.ok(!denied(await fence(pre("mcp__tonefold__add_layer", { kind: "pad" }, "harmony-form"))));
  assert.ok(denied(await fence(pre("mcp__tonefold__add_layer", { kind: "sub" }, "harmony-form"))));

  // Section-level work carries no track, and the producer itself is never boundary-checked.
  assert.ok(!denied(await fence(pre("mcp__tonefold__set_form", {}, "harmony-form"))));
  assert.ok(!denied(await fence(pre("mcp__tonefold__generate", { track: "drums" }))));
});

test("the denial says who owns the layer, so the specialist can hand it back", async () => {
  const run = new ProducerRun();
  run.phase = "executing";
  const out: any = await toolFence(run)(pre("mcp__tonefold__generate", { track: "bass" }, "harmony-form"));
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

test("verify_step's target grammar matches the plan's own words", () => {
  const w = (track: string, section?: string): WriteEntry => ({ agent: "rhythm-section", tool: "generate", track, section });
  assert.ok(matches(w("bass", "verse"), "bass@verse"));
  assert.ok(matches(w("bass", "Verse"), "bass@verse"), "section names are not case-sensitive");
  assert.ok(matches(w("bass2", "verse"), "bass@verse"), "a numbered layer is still that kind");
  assert.ok(matches(w("drums", "chorus"), "drums@*"), "a wildcard section takes any");
  assert.ok(matches(w("drums", "chorus"), "drums"), "so does no section at all");
  assert.ok(!matches(w("bass", "chorus"), "bass@verse"));
  assert.ok(!matches(w("melody", "verse"), "bass@verse"));
  // Session-wide work carries no section and no track.
  assert.ok(matches(w("chords"), "chords@verse"), "a write with no section counts for every section");
  assert.ok(!matches({ agent: "producer", tool: "set_form" }, "chords@verse"));
});

test("the checklist is read from what landed, not from what was claimed", () => {
  const plan: Plan = {
    steps: [
      { id: "s1", owner: "arrangement-mix", title: "Roster", detail: "", targets: [] },
      { id: "s2", owner: "harmony-form", title: "Chords", detail: "", targets: ["chords@verse", "chords@chorus"] },
      { id: "s3", owner: "rhythm-section", title: "Groove", detail: "", targets: ["drums@*", "bass@*"] },
      { id: "s4", owner: "arrangement-mix", title: "Polish", detail: "", targets: [] },
    ],
  } as Plan;
  const w = (track: string, section?: string): WriteEntry => ({ agent: "x", tool: "generate", track, section });
  const status = (writes: WriteEntry[], finished = false) => checklist(plan, writes, finished).map((t) => t.status);

  // Nothing yet: the first step is the one in hand.
  assert.deepEqual(status([]), ["in_progress", "pending", "pending", "pending"]);
  // Half a step is not a step.
  assert.deepEqual(status([w("chords", "verse")]), ["in_progress", "pending", "pending", "pending"]);
  // Both targets landed — and the roster step before it must have happened for that to be possible.
  assert.deepEqual(status([w("chords", "verse"), w("chords", "chorus")]), ["completed", "completed", "in_progress", "pending"]);
  assert.deepEqual(
    status([w("chords", "verse"), w("chords", "chorus"), w("drums", "verse"), w("bass", "verse")]),
    ["completed", "completed", "completed", "in_progress"],
  );
  // The run ending settles whatever cannot be proved either way.
  assert.deepEqual(status([w("chords", "verse")], true), ["completed", "pending", "pending", "completed"]);
  assert.deepEqual(checklist(plan, [])[1].content, "Chords");
});

test("reads are not writes", () => {
  // The ledger is the answer to "was this written?", so anything in the read set must stay out of
  // it — `get_notes` on a layer is not a part.
  for (const read of ["get_notes", "get_session", "analyze", "suggest_chords", "verify_step"]) {
    assert.ok(READ_TOOLS.has(read), `${read} should be a read`);
  }
  for (const write of ["generate", "set_notes", "set_chords", "humanize", "add_layer", "set_arrangement"]) {
    assert.ok(!READ_TOOLS.has(write), `${write} changes the song`);
  }
});
