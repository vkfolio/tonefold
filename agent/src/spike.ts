// Phase 0 spike: does the Agent SDK run unattended with an in-process tool using the local Claude login?
import { query, tool, createSdkMcpServer } from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";

const setChords = tool(
  "set_chords",
  "Set the chord progression for a section. notation: bars separated by '|', e.g. '| C | Am | F | G |'",
  { section: z.string(), notation: z.string() },
  async ({ section, notation }) => {
    console.log(`[tool] set_chords(${section}, ${notation})`);
    return { content: [{ type: "text", text: `ok: 4 bars parsed, all diatonic to C major` }] };
  },
);
const server = createSdkMcpServer({ name: "flvstx", version: "0.0.1", tools: [setChords] });

const t0 = Date.now();
let firstText = 0;
for await (const msg of query({
  prompt: "Propose a warm, simple 4-bar chord progression for a kids' rhyme in C major and store it with set_chords for section 'Verse'. Then reply in one sentence.",
  options: {
    systemPrompt: "You are a music composer assistant. Use tools to write results.",
    mcpServers: { flvstx: server },
    allowedTools: ["mcp__flvstx__set_chords"],
    permissionMode: "bypassPermissions",
    allowDangerouslySkipPermissions: true,
    maxTurns: 4,
  },
})) {
  if (msg.type === "system" && (msg as any).subtype === "init") {
    const m = msg as any;
    console.log(`[init] model=${m.model} apiKeySource=${m.apiKeySource} tools=${(m.tools||[]).filter((t:string)=>t.startsWith("mcp__")).join(",")}`);
  } else if (msg.type === "assistant") {
    for (const b of (msg as any).message.content) {
      if (b.type === "text") { if (!firstText) firstText = Date.now() - t0; console.log(`[assistant] ${b.text}`); }
      if (b.type === "tool_use") console.log(`[tool_use] ${b.name} ${JSON.stringify(b.input)}`);
    }
  } else if (msg.type === "result") {
    const r = msg as any;
    console.log(`[result] subtype=${r.subtype} turns=${r.num_turns} cost=$${r.total_cost_usd} session=${r.session_id} total=${Date.now() - t0}ms firstText=${firstText}ms`);
    if (r.subtype !== "success") console.log(JSON.stringify(r).slice(0, 800));
  }
}
