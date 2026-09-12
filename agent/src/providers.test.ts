// The provider switch is an environment, not a second backend: these pin the URL cleanup the CLI
// depends on, the exact variables Ollama documents, and that Claude's environment is untouched.

import assert from "node:assert/strict";
import { test } from "node:test";
import { asProvider, DEFAULT_OLLAMA_URL, listOllamaModels, normalizeOllamaUrl, providerEnv } from "./providers.js";

test("base URLs are normalised to what the CLI expects", () => {
  assert.equal(normalizeOllamaUrl(""), DEFAULT_OLLAMA_URL);
  assert.equal(normalizeOllamaUrl(undefined), DEFAULT_OLLAMA_URL);
  assert.equal(normalizeOllamaUrl("  http://localhost:11434/  "), "http://localhost:11434");
  // The CLI appends /v1/messages itself, so a pasted OpenAI-style base loses its /v1.
  assert.equal(normalizeOllamaUrl("http://studio-pc:11434/v1"), "http://studio-pc:11434");
  assert.equal(normalizeOllamaUrl("studio-pc:11434"), "http://studio-pc:11434");
  assert.equal(normalizeOllamaUrl("https://ollama.example.com"), "https://ollama.example.com");
});

test("an unknown provider is claude, never a guess", () => {
  assert.equal(asProvider("ollama"), "ollama");
  assert.equal(asProvider("claude"), "claude");
  assert.equal(asProvider("openai"), "claude");
  assert.equal(asProvider(undefined), "claude");
});

test("ollama gets the documented environment; claude inherits", () => {
  assert.equal(providerEnv("claude", "http://localhost:11434"), undefined);
  const env = providerEnv("ollama", "remote-box:11434/")!;
  assert.equal(env.ANTHROPIC_BASE_URL, "http://remote-box:11434");
  assert.equal(env.ANTHROPIC_AUTH_TOKEN, "ollama");
  // Empty, not absent: an inherited real key would otherwise be sent to the Ollama server.
  assert.equal(env.ANTHROPIC_API_KEY, "");
  // PATH and friends still come through, or the CLI cannot even start.
  assert.equal(env.PATH, process.env.PATH);
});

test("the model list is names, deduplicated and sorted", async () => {
  const fetchImpl = (async (url: string | URL | Request) => {
    assert.equal(String(url), "http://localhost:11434/api/tags");
    return new Response(JSON.stringify({ models: [{ name: "qwen3:8b" }, { name: "gpt-oss:20b" }, { model: "qwen3:8b" }] }));
  }) as typeof fetch;
  assert.deepEqual(await listOllamaModels("localhost:11434", fetchImpl), ["gpt-oss:20b", "qwen3:8b"]);
});

test("an unreachable server is an error a user can act on", async () => {
  const fetchImpl = (async () => {
    throw new TypeError("fetch failed");
  }) as unknown as typeof fetch;
  await assert.rejects(listOllamaModels("http://nowhere:11434", fetchImpl), /could not reach http:\/\/nowhere:11434 .*Ollama running/);
  const bad = (async () => new Response("nope", { status: 500 })) as typeof fetch;
  await assert.rejects(listOllamaModels("http://localhost:11434", bad), /answered 500/);
});
