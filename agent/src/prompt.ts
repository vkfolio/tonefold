import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const AGENT_ROOT = existsSync(join(here, "..", "package.json")) ? join(here, "..") : join(here, "..", "..");

export function systemPrompt(): string {
  const base = readFileSync(join(AGENT_ROOT, "prompts", "composer.md"), "utf8");
  const skillsDir = join(AGENT_ROOT, "skills");
  let skills = "";
  if (existsSync(skillsDir)) {
    for (const f of readdirSync(skillsDir).filter((f) => f.endsWith(".md")).sort()) {
      skills += `\n\n---\n# Skill: ${f.replace(/\.md$/, "")}\n\n` + readFileSync(join(skillsDir, f), "utf8");
    }
  }
  // The routing map for the vendored reference library (see agent/reference/NOTICE.md). Small on
  // purpose: it lists every path so the model never has to guess one, and the files themselves are
  // read on demand through the read_reference tool.
  const index = join(AGENT_ROOT, "reference", "INDEX.md");
  const routing = existsSync(index) ? `

---
# Reference library

${readFileSync(index, "utf8")}` : "";
  return base + skills + routing;
}
