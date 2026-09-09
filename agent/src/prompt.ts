import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const AGENT_ROOT = existsSync(join(here, "..", "package.json")) ? join(here, "..") : join(here, "..", "..");

const prompts = (...parts: string[]) => join(AGENT_ROOT, "prompts", ...parts);

/** How to write notation and address layers — the same for every persona, so it lives in one file. */
const notation = () => "\n\n" + readFileSync(prompts("_notation.md"), "utf8");

/** The persona differs per mode; the craft knowledge below it does not. */
export function systemPrompt(mode: "composer" | "producer" = "composer"): string {
  const base = readFileSync(prompts(`${mode}.md`), "utf8") + notation();
  return base + skillsText() + referenceIndex();
}

/**
 * A specialist's prompt: its craft brief, then the same platform facts the producer has. It gets the
 * skills and the reference index too — it is the one doing the writing.
 */
export function specialistPrompt(name: string): string {
  return readFileSync(prompts("specialists", `${name}.md`), "utf8") + notation() + skillsText() + referenceIndex();
}

function skillsText(): string {
  const skillsDir = join(AGENT_ROOT, "skills");
  let skills = "";
  if (existsSync(skillsDir)) {
    for (const f of readdirSync(skillsDir).filter((f) => f.endsWith(".md")).sort()) {
      skills += `\n\n---\n# Skill: ${f.replace(/\.md$/, "")}\n\n` + readFileSync(join(skillsDir, f), "utf8");
    }
  }
  return skills;
}

function referenceIndex(): string {
  // The routing map for the vendored reference library (see agent/reference/NOTICE.md). Small on
  // purpose: it lists every path so the model never has to guess one, and the files themselves are
  // read on demand through the read_reference tool.
  const index = join(AGENT_ROOT, "reference", "INDEX.md");
  return existsSync(index) ? `

---
# Reference library

${readFileSync(index, "utf8")}` : "";
}
