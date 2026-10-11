// Where a turn's model runs. The SDK drives the Claude Code CLI, and the CLI talks to whatever
// `ANTHROPIC_BASE_URL` names — so an Ollama server (which speaks the Anthropic Messages API, local
// or on another machine) is the same session with a different environment, not a second backend.
//
// Nothing here is Ollama-specific beyond the model listing: any Anthropic-compatible endpoint
// would work through `providerEnv` with a base URL.

import type { Provider } from "./protocol.js";

export const PROVIDERS: Provider[] = ["claude", "ollama"];
export const DEFAULT_OLLAMA_URL = "http://localhost:11434";

/** "claude" unless the value is exactly "ollama" — an unknown provider is never guessed at. */
export function asProvider(v: unknown): Provider {
  return v === "ollama" ? "ollama" : "claude";
}

/**
 * A base URL the CLI can use: scheme added when missing, trailing slashes and a stray `/v1`
 * removed (the CLI appends `/v1/messages` itself). Empty input means the default server.
 */
export function normalizeOllamaUrl(raw: string | null | undefined): string {
  let u = (raw ?? "").trim();
  if (!u) return DEFAULT_OLLAMA_URL;
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(u)) u = `http://${u}`;
  u = u.replace(/\/+$/, "");
  u = u.replace(/\/v1$/i, "");
  return u;
}

/**
 * The subprocess environment for a provider. Ollama's documented recipe for Claude Code: base URL
 * at the server, auth token "ollama", and an empty API key so the CLI does not reach for the
 * user's Anthropic credentials. Claude gets `undefined`, which the SDK reads as "inherit".
 */
export function providerEnv(provider: Provider, baseUrl: string): Record<string, string | undefined> | undefined {
  if (provider !== "ollama") return undefined;
  return {
    ...process.env,
    ANTHROPIC_BASE_URL: normalizeOllamaUrl(baseUrl),
    ANTHROPIC_AUTH_TOKEN: "ollama",
    ANTHROPIC_API_KEY: "",
    // No Anthropic account is in play, so nothing about it needs fetching either.
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
  };
}

/** `GET {url}` as JSON; `null` when the route is absent (404), a thrown error on other failures. */
async function fetchJson(url: string, fetchImpl: typeof fetch): Promise<unknown | null> {
  let res: Response;
  try {
    res = await fetchImpl(url, { signal: AbortSignal.timeout(5000) });
  } catch (e) {
    const why = e instanceof Error ? (e.name === "TimeoutError" ? "timed out" : e.message) : String(e);
    throw new Error(`could not reach ${new URL(url).origin} (${why}) — is the model server (Ollama, llama.cpp, …) running there?`);
  }
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`${url} answered ${res.status}`);
  return res.json();
}

/** Names from an Ollama `/api/tags` body, deduplicated and sorted. */
function tagsToNames(body: unknown): string[] {
  const models = (body as { models?: { name?: string; model?: string }[] }).models ?? [];
  return tidy(models.map((m) => m.name ?? m.model ?? ""));
}

/** Ids from an OpenAI-compatible `/v1/models` body (llama.cpp et al.), deduplicated and sorted. */
function openAiToNames(body: unknown): string[] {
  const data = (body as { data?: { id?: string; name?: string }[] }).data ?? [];
  return tidy(data.map((m) => m.id ?? m.name ?? ""));
}

function tidy(names: string[]): string[] {
  return [...new Set(names.filter(Boolean))].sort();
}

/**
 * The models a server reports. Native Ollama first (`/api/tags`); when that route is missing
 * (llama.cpp, vLLM, LM Studio…), fall back to the OpenAI-compatible `/v1/models`.
 */
export async function listOllamaModels(baseUrl: string, fetchImpl: typeof fetch = fetch): Promise<string[]> {
  const base = normalizeOllamaUrl(baseUrl);
  const tags = await fetchJson(`${base}/api/tags`, fetchImpl);
  if (tags) return tagsToNames(tags);
  const models = await fetchJson(`${base}/v1/models`, fetchImpl);
  if (models) return openAiToNames(models);
  throw new Error(`${base} lists no models: /api/tags and /v1/models both answered 404`);
}

/** Which chat API a server speaks; see `cliBaseUrl` in index.ts. */
export type Backend = "ollama" | "anthropic";

/**
 * Ollama's native route means the translating shim; otherwise, a server answering the Anthropic
 * Messages API (llama.cpp's `/v1/messages`, for one) can be handed to the CLI unchanged. A route
 * that exists replies with its own error status, so anything but 404 counts as supported.
 */
export async function detectBackend(baseUrl: string, fetchImpl: typeof fetch = fetch): Promise<Backend> {
  const base = normalizeOllamaUrl(baseUrl);
  try {
    if ((await fetchImpl(`${base}/api/tags`, { signal: AbortSignal.timeout(4000) })).ok) return "ollama";
  } catch {}
  try {
    const res = await fetchImpl(`${base}/v1/messages`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "{}",
      signal: AbortSignal.timeout(4000),
    });
    if (res.status !== 404) return "anthropic";
  } catch {}
  return "ollama";
}
