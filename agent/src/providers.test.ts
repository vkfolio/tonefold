// The provider switch is an environment, not a second backend: these pin the URL cleanup the CLI
// depends on, the exact variables Ollama documents, and that Claude's environment is untouched.

import assert from "node:assert/strict";
import { test } from "node:test";
import { asProvider, DEFAULT_OLLAMA_URL, detectBackend, listOllamaModels, normalizeOllamaUrl, providerEnv } from "./providers.js";

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
  await assert.rejects(listOllamaModels("http://nowhere:11434", fetchImpl), /could not reach http:\/\/nowhere:11434 .*model server .* running there/);
  const bad = (async () => new Response("nope", { status: 500 })) as typeof fetch;
  await assert.rejects(listOllamaModels("http://localhost:11434", bad), /answered 500/);
});

test("a server without /api/tags lists models through /v1/models instead", async () => {
  const calls: string[] = [];
  const fetchImpl = (async (url: string | URL | Request) => {
    const u = String(url);
    calls.push(u);
    if (u.endsWith("/api/tags")) return new Response("not found", { status: 404 });
    assert.equal(u, "http://localhost:8080/v1/models");
    return new Response(JSON.stringify({ data: [{ id: "local-model" }, { id: "gpt-oss:20b" }, { id: "local-model" }] }));
  }) as typeof fetch;
  assert.deepEqual(await listOllamaModels("localhost:8080", fetchImpl), ["gpt-oss:20b", "local-model"]);
  assert.deepEqual(calls, ["http://localhost:8080/api/tags", "http://localhost:8080/v1/models"]);
});

test("detectBackend: native Ollama wins, Anthropic-native is used directly", async () => {
  assert.equal(await detectBackend("localhost:11434", (async () => new Response(JSON.stringify({ models: [] }))) as typeof fetch), "ollama");
  // /api/tags missing, /v1/messages answers with its own error (a 404 here would be "no such route").
  const misses = (async (u: string | URL | Request) => (u.toString().endsWith("/api/tags") ? new Response("nope", { status: 404 }) : new Response("{}", { status: 500 }))) as typeof fetch;
  assert.equal(await detectBackend("localhost:8080", misses), "anthropic");
  // Both missing: fall back to the translating shim, which will surface the failure clearly.
  const gone = (async () => new Response("nope", { status: 404 })) as typeof fetch;
  assert.equal(await detectBackend("localhost:8080", gone), "ollama");
});
