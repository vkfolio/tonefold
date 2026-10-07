// The bridge in both directions, against fixtures shaped like what the CLI and Ollama actually
// send (captured while building this), and the shim end to end against a stand-in Ollama — a real
// model is neither needed nor available in CI.

import assert from "node:assert/strict";
import http from "node:http";
import { test } from "node:test";
import { estimateTokens, fromOllamaFinal, SseTranslator, toOllamaChat, type ARequest } from "./ollama-bridge.js";
import { startShim } from "./ollama-shim.js";

const TOOL = { name: "mcp__tonefold__set_key_tempo", description: "Set key and tempo.", input_schema: { type: "object", properties: { key: { type: "string" } }, $schema: "http://json-schema.org/draft-07/schema#" } };

test("a CLI request becomes a native chat request with the context window set", () => {
  const req: ARequest = {
    model: "qwen3:8b",
    system: [{ type: "text", text: "billing header" }, { type: "text", text: "You compose.", cache_control: { type: "ephemeral" } }],
    messages: [
      { role: "user", content: [{ type: "text", text: "<system-reminder>x</system-reminder>" }, { type: "text", text: "Set A minor" }] },
      // The trailing system-role message the CLI appends, which Ollama's own endpoint rejects.
      { role: "system", content: [{ type: "text", text: "<total_tokens>1</total_tokens>" }] },
    ],
    tools: [TOOL],
    max_tokens: 32000,
    stream: true,
    thinking: { type: "adaptive" },
  };
  const out = toOllamaChat(req, { numCtx: 16384 });
  assert.equal(out.model, "qwen3:8b");
  assert.equal(out.stream, true);
  assert.equal(out.think, true);
  assert.deepEqual(out.options, { num_ctx: 16384, num_predict: 32000 });
  assert.deepEqual(out.messages, [
    { role: "system", content: "billing header\nYou compose.\n\n<total_tokens>1</total_tokens>" },
    { role: "user", content: "<system-reminder>x</system-reminder>\nSet A minor" },
  ]);
  assert.deepEqual(out.tools, [{ type: "function", function: { name: TOOL.name, description: TOOL.description, parameters: { type: "object", properties: { key: { type: "string" } } } } }]);
});

test("the sidecar's thinking setting overrides the request's", () => {
  const req: ARequest = { model: "m", messages: [{ role: "user", content: "hi" }], thinking: { type: "adaptive" } };
  assert.equal(toOllamaChat(req, { numCtx: 4096, think: false }).think, false);
  assert.equal(toOllamaChat(req, { numCtx: 4096, think: true }).think, true);
  assert.equal(toOllamaChat({ ...req, thinking: undefined }, { numCtx: 4096 }).think, undefined);
});

test("tool calls and their results round-trip through Ollama's fields", () => {
  const req: ARequest = {
    model: "m",
    messages: [
      { role: "user", content: "Set A minor" },
      { role: "assistant", content: [{ type: "thinking", thinking: "easy" }, { type: "tool_use", id: "toolu_1", name: "set_key_tempo", input: { key: "A minor" } }] },
      { role: "user", content: [{ type: "tool_result", tool_use_id: "toolu_1", content: [{ type: "text", text: "key set" }] }, { type: "text", text: "now tempo" }] },
      { role: "assistant", content: [{ type: "tool_use", id: "toolu_2", name: "set_key_tempo", input: { tempo: 92 } }] },
      { role: "user", content: [{ type: "tool_result", tool_use_id: "toolu_2", content: "boom", is_error: true }] },
    ],
    thinking: { type: "disabled" },
  };
  const out = toOllamaChat(req, { numCtx: 4096 });
  assert.equal(out.think, false);
  assert.deepEqual(out.messages, [
    { role: "user", content: "Set A minor" },
    { role: "assistant", content: "", thinking: "easy", tool_calls: [{ function: { name: "set_key_tempo", arguments: { key: "A minor" } } }] },
    { role: "tool", content: "key set", tool_name: "set_key_tempo" },
    { role: "user", content: "now tempo" },
    { role: "assistant", content: "", tool_calls: [{ function: { name: "set_key_tempo", arguments: { tempo: 92 } } }] },
    { role: "tool", content: "ERROR: boom", tool_name: "set_key_tempo" },
  ]);
});

test("a non-streamed reply becomes an Anthropic message", () => {
  const out = fromOllamaFinal(
    { message: { role: "assistant", content: "Done.", thinking: "ok", tool_calls: [{ function: { name: "set_key_tempo", arguments: { key: "A minor" } } }] }, done: true, done_reason: "stop", prompt_eval_count: 300, eval_count: 20 },
    "m",
  );
  assert.equal(out.stop_reason, "tool_use");
  assert.deepEqual(out.usage, { input_tokens: 300, output_tokens: 20 });
  assert.equal(out.content[0].type, "thinking");
  assert.equal(out.content[1].type, "text");
  assert.equal(out.content[2].type, "tool_use");
  assert.equal(out.content[2].name, "set_key_tempo");
  assert.deepEqual(out.content[2].input, { key: "A minor" });
  assert.match(String(out.content[2].id), /^toolu_/);
  assert.equal(fromOllamaFinal({ message: { role: "assistant", content: "x" }, done: true, done_reason: "length" }, "m").stop_reason, "max_tokens");
});

/** Parses SSE text back into (event, data) pairs. */
function events(sse: string): { event: string; data: any }[] {
  return sse
    .split("\n\n")
    .filter(Boolean)
    .map((chunk) => {
      const [e, d] = chunk.split("\n");
      return { event: e.replace("event: ", ""), data: JSON.parse(d.replace("data: ", "")) };
    });
}

test("streamed chunks become the Anthropic event sequence", () => {
  const tr = new SseTranslator("m");
  let sse = tr.start();
  sse += tr.feed({ message: { thinking: "The user" } });
  sse += tr.feed({ message: { thinking: " wants" } });
  sse += tr.feed({ message: { content: "Sure" } });
  sse += tr.feed({ message: { content: "." } });
  sse += tr.feed({ message: { content: "", tool_calls: [{ function: { name: "set_key_tempo", arguments: { key: "A minor" } } }] } });
  sse += tr.feed({ message: { content: "" }, done: true, done_reason: "stop", prompt_eval_count: 100, eval_count: 9 });
  const ev = events(sse);
  assert.deepEqual(
    ev.map((e) => e.event),
    ["message_start", "content_block_start", "content_block_delta", "content_block_delta", "content_block_stop", "content_block_start", "content_block_delta", "content_block_delta", "content_block_stop", "content_block_start", "content_block_delta", "content_block_stop", "message_delta", "message_stop"],
  );
  assert.equal(ev[1].data.content_block.type, "thinking");
  assert.equal(ev[2].data.delta.thinking, "The user");
  assert.equal(ev[5].data.content_block.type, "text");
  assert.equal(ev[5].data.index, 1);
  assert.equal(ev[7].data.delta.text, ".");
  assert.equal(ev[9].data.content_block.type, "tool_use");
  assert.equal(ev[9].data.index, 2);
  assert.equal(ev[10].data.delta.partial_json, JSON.stringify({ key: "A minor" }));
  assert.equal(ev[12].data.delta.stop_reason, "tool_use");
  assert.deepEqual(ev[12].data.usage, { input_tokens: 100, output_tokens: 9 });
});

test("a mid-stream error is an error event", () => {
  const tr = new SseTranslator("m");
  const ev = events(tr.start() + tr.feed({ message: { content: "hi" } }) + tr.feed({ error: "model crashed" }));
  assert.equal(ev.at(-1)?.event, "error");
  assert.equal(ev.at(-1)?.data.error.message, "model crashed");
  assert.equal(ev.at(-2)?.event, "content_block_stop");
});

test("token counting is a rough size, never zero for a real request", () => {
  assert.ok(estimateTokens({ model: "m", system: "x".repeat(400), messages: [{ role: "user", content: "hi" }] }) >= 100);
});

/** A stand-in Ollama: records what it got, streams a canned reply. */
function fakeOllama(reply: object[]) {
  const seen: { url: string; body: any }[] = [];
  const server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      seen.push({ url: req.url ?? "", body: body ? JSON.parse(body) : null });
      if (req.url === "/api/show") return res.end(JSON.stringify({ model_info: { "qwen3.context_length": 8192 } }));
      if (req.url === "/api/tags") return res.end(JSON.stringify({ models: [{ name: "qwen3:8b" }] }));
      if (req.url !== "/api/chat") {
        res.writeHead(404);
        return res.end();
      }
      res.writeHead(200, { "content-type": "application/x-ndjson" });
      let i = 0;
      const tick = () => {
        if (i < reply.length) {
          res.write(JSON.stringify(reply[i++]) + "\n");
          setTimeout(tick, 5);
        } else res.end();
      };
      tick();
    });
  });
  return { server, seen, listen: () => new Promise<number>((r) => server.listen(0, "127.0.0.1", () => r((server.address() as { port: number }).port))) };
}

test("the shim serves a streamed messages request through a native chat call", async () => {
  const fake = fakeOllama([
    { message: { role: "assistant", content: "", thinking: "hm" }, done: false },
    { message: { role: "assistant", content: "", tool_calls: [{ function: { name: "set_key_tempo", arguments: { key: "A minor" } } }] }, done: false },
    { message: { role: "assistant", content: "" }, done: true, done_reason: "stop", prompt_eval_count: 50, eval_count: 5 },
  ]);
  const port = await fake.listen();
  const shim = await startShim(`127.0.0.1:${port}/`, () => {}, 32768);
  try {
    const res = await fetch(`${shim.url}/v1/messages?beta=true`, {
      method: "POST",
      headers: { "content-type": "application/json", "x-api-key": "ollama" },
      body: JSON.stringify({ model: "qwen3:8b", stream: true, max_tokens: 100, system: "S", messages: [{ role: "user", content: "hi" }, { role: "system", content: "budget" }], tools: [TOOL] }),
    });
    assert.equal(res.status, 200);
    assert.equal(res.headers.get("content-type"), "text/event-stream");
    const ev = events(await res.text());
    assert.equal(ev[0].event, "message_start");
    assert.equal(ev.at(-1)?.event, "message_stop");
    assert.ok(ev.some((e) => e.event === "content_block_start" && e.data.content_block.type === "tool_use" && e.data.content_block.name === "set_key_tempo"));
    // Not a messages request: forwarded as is.
    const tags = await fetch(`${shim.url}/api/tags`);
    assert.deepEqual(await tags.json(), { models: [{ name: "qwen3:8b" }] });
    const count = await fetch(`${shim.url}/v1/messages/count_tokens`, { method: "POST", body: JSON.stringify({ model: "m", messages: [{ role: "user", content: "hello there" }] }) });
    assert.ok(((await count.json()) as { input_tokens: number }).input_tokens > 0);
  } finally {
    shim.close();
    fake.server.close();
  }
  const chat = fake.seen.find((s) => s.url === "/api/chat")!;
  // The model's own limit (8192, from /api/show) caps the configured 32768.
  assert.equal(chat.body.options.num_ctx, 8192);
  assert.equal(chat.body.messages[0].role, "system");
  assert.equal(chat.body.messages[0].content, "S\n\nbudget");
  assert.equal(chat.body.messages.at(-1).role, "user");
  assert.equal(chat.body.tools[0].function.name, TOOL.name);
});

test("an unreachable server answers as an API error rather than hanging", async () => {
  const shim = await startShim("http://127.0.0.1:1");
  try {
    const res = await fetch(`${shim.url}/v1/messages`, { method: "POST", body: JSON.stringify({ model: "m", messages: [] }) });
    assert.equal(res.status, 502);
    const j = (await res.json()) as { error: { message: string } };
    assert.match(j.error.message, /unreachable/);
  } finally {
    shim.close();
  }
});
