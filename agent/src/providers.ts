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

/** `GET {base}/api/tags`, as model names. Throws with a message a user can act on. */
export async function listOllamaModels(baseUrl: string, fetchImpl: typeof fetch = fetch): Promise<string[]> {
  const base = normalizeOllamaUrl(baseUrl);
  let res: Response;
  try {
    res = await fetchImpl(`${base}/api/tags`, { signal: AbortSignal.timeout(5000) });
  } catch (e) {
    const why = e instanceof Error ? (e.name === "TimeoutError" ? "timed out" : e.message) : String(e);
    throw new Error(`could not reach ${base} (${why}) — is Ollama running there?`);
  }
  if (!res.ok) throw new Error(`${base}/api/tags answered ${res.status}`);
  const body = (await res.json()) as { models?: { name?: string; model?: string }[] };
  const names = (body.models ?? []).map((m) => m.name ?? m.model ?? "").filter(Boolean);
  return [...new Set(names)].sort();
}
