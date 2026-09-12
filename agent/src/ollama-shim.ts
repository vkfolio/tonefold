// A loopback server the Claude Code CLI is pointed at when the provider is Ollama. It answers the
// Anthropic Messages API by translating each request into Ollama's native chat API and the reply
// back — the mapping lives in ollama-bridge.ts, with the reasons Ollama's own Anthropic endpoint
// is not used. Anything that is not a messages request is forwarded to the server verbatim.

import http, { type IncomingMessage, type ServerResponse } from "node:http";
import https from "node:https";
import type { AddressInfo } from "node:net";
import { estimateTokens, fromOllamaFinal, SseTranslator, toOllamaChat, type ARequest, type OChunk } from "./ollama-bridge.js";
import { normalizeOllamaUrl } from "./providers.js";

/**
 * Context window to ask for. Enough for the prompt, the tool schemas and a conversation; larger
 * costs VRAM that an 8 GB laptop GPU does not have, and the model spills to the CPU and crawls.
 */
export const DEFAULT_NUM_CTX = 16384;

export interface Shim {
  /** `http://127.0.0.1:<port>` — what the CLI is pointed at. */
  url: string;
  target: string;
  close(): void;
}

type Log = (...a: unknown[]) => void;

function anthropicError(res: ServerResponse, status: number, message: string) {
  if (!res.headersSent) res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify({ type: "error", error: { type: status === 429 ? "rate_limit_error" : "api_error", message } }));
}

/**
 * The context window for a model: the configured size, capped at what the model was trained for
 * (`/api/show` reports it). Cached per model; a server that cannot answer gets the configured size.
 */
async function contextFor(base: URL, model: string, wanted: number, cache: Map<string, number>, log: Log): Promise<number> {
  const hit = cache.get(model);
  if (hit) return hit;
  let ctx = wanted;
  try {
    const res = await fetch(new URL("/api/show", base), { method: "POST", body: JSON.stringify({ model }), signal: AbortSignal.timeout(5000) });
    const info = ((await res.json()) as { model_info?: Record<string, unknown> }).model_info ?? {};
    const key = Object.keys(info).find((k) => k.endsWith(".context_length"));
    const max = key ? Number(info[key]) : NaN;
    if (Number.isFinite(max) && max > 0) ctx = Math.min(wanted, max);
  } catch (e) {
    log(`shim: /api/show ${model} failed (${e instanceof Error ? e.message : e}); using num_ctx=${ctx}`);
  }
  cache.set(model, ctx);
  return ctx;
}

/** Starts a shim forwarding to `target` (an Ollama base URL). Resolves once it is listening. */
export function startShim(target: string, log: Log = () => {}, numCtx = DEFAULT_NUM_CTX, think: () => boolean | undefined = () => false): Promise<Shim> {
  const base = new URL(normalizeOllamaUrl(target));
  const client = base.protocol === "https:" ? https : http;
  const ctxCache = new Map<string, number>();

  const upstream = (path: string, method: string, headers: Record<string, string | string[]>, body: Buffer, onRes: (r: IncomingMessage) => void, onErr: (e: Error) => void) => {
    const req = client.request({ protocol: base.protocol, hostname: base.hostname, port: base.port || undefined, path, method, headers: { ...headers, host: base.host, "content-length": String(body.length) } }, onRes);
    req.on("error", onErr);
    req.end(body);
    return req;
  };

  /** A messages request: translate, run, translate back — streamed or not, as asked. */
  const messages = async (req: IncomingMessage, res: ServerResponse, body: Buffer) => {
    let areq: ARequest;
    try {
      areq = JSON.parse(body.toString("utf8"));
    } catch {
      return anthropicError(res, 400, "request body is not JSON");
    }
    const ctx = await contextFor(base, areq.model, numCtx, ctxCache, log);
    const oreq = toOllamaChat(areq, { numCtx: ctx, think: think() });
    const payload = Buffer.from(JSON.stringify(oreq));
    const up = upstream("/api/chat", "POST", { "content-type": "application/json" }, payload, (upRes) => {
      const status = upRes.statusCode ?? 502;
      if (status !== 200) {
        let text = "";
        upRes.on("data", (c) => (text += c));
        upRes.on("end", () => {
          let msg = text;
          try {
            msg = (JSON.parse(text) as { error?: string }).error ?? text;
          } catch {}
          log(`shim: ollama ${status}: ${msg.slice(0, 200)}`);
          anthropicError(res, status, msg);
        });
        return;
      }
      if (!oreq.stream) {
        let text = "";
        upRes.on("data", (c) => (text += c));
        upRes.on("end", () => {
          try {
            const out = fromOllamaFinal(JSON.parse(text) as OChunk, areq.model);
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify(out));
          } catch (e) {
            anthropicError(res, 502, `unreadable reply from Ollama: ${e instanceof Error ? e.message : e}`);
          }
        });
        return;
      }
      res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache", connection: "keep-alive" });
      const tr = new SseTranslator(areq.model);
      res.write(tr.start());
      let buf = "";
      upRes.on("data", (c: Buffer) => {
        buf += c.toString("utf8");
        let nl: number;
        while ((nl = buf.indexOf("\n")) >= 0) {
          const line = buf.slice(0, nl).trim();
          buf = buf.slice(nl + 1);
          if (!line) continue;
          try {
            res.write(tr.feed(JSON.parse(line) as OChunk));
          } catch (e) {
            log(`shim: bad chunk from Ollama: ${line.slice(0, 120)} (${e instanceof Error ? e.message : e})`);
          }
        }
      });
      upRes.on("end", () => res.end());
      upRes.on("error", () => res.end());
    }, (e) => {
      log(`shim: ${base.origin} unreachable: ${e.message}`);
      anthropicError(res, 502, `Ollama at ${base.origin} is unreachable: ${e.message}`);
    });
    // A client that gives up mid-stream (the turn was cancelled) must not leave the model running.
    res.on("close", () => up.destroy());
  };

  const server = http.createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on("data", (c: Buffer) => chunks.push(c));
    req.on("end", () => {
      const body = Buffer.concat(chunks);
      const path = req.url ?? "/";
      if (req.method === "POST" && /^\/v1\/messages\/count_tokens(\?|$)/.test(path)) {
        try {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({ input_tokens: estimateTokens(JSON.parse(body.toString("utf8"))) }));
        } catch {
          anthropicError(res, 400, "request body is not JSON");
        }
        return;
      }
      if (req.method === "POST" && /^\/v1\/messages(\?|$)/.test(path)) {
        messages(req, res, body).catch((e) => anthropicError(res, 500, e instanceof Error ? e.message : String(e)));
        return;
      }
      // Everything else, verbatim.
      const headers: Record<string, string | string[]> = {};
      for (const [k, v] of Object.entries(req.headers)) {
        if (v !== undefined && k !== "host" && k !== "content-length" && k !== "connection") headers[k] = v;
      }
      const up = upstream(path, req.method ?? "GET", headers, body, (upRes) => {
        res.writeHead(upRes.statusCode ?? 502, upRes.headers);
        upRes.pipe(res);
      }, (e) => anthropicError(res, 502, `Ollama at ${base.origin} is unreachable: ${e.message}`));
      res.on("close", () => up.destroy());
    });
  });

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as AddressInfo;
      const url = `http://127.0.0.1:${port}`;
      log(`ollama shim ${url} -> ${base.origin} (num_ctx up to ${numCtx})`);
      resolve({ url, target: base.origin, close: () => server.close() });
    });
  });
}
