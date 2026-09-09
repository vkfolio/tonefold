// Builds agent/reference/INDEX.md — the routing map that ships in every system prompt.
// One line per reference file, so the composer can pick 1-3 to read instead of carrying the
// whole library in context. Run after vendoring or updating the corpus:
//   node scripts/build-reference-index.mjs
import { readdirSync, readFileSync, writeFileSync, statSync } from "node:fs";
import { join, relative, dirname, sep } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "reference");
const corpus = join(root, "music-composition");

/** Every .md under `dir`, depth first. */
function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir).sort()) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (name.endsWith(".md")) out.push(p);
  }
  return out;
}

const groups = new Map();
for (const file of walk(corpus)) {
  const rel = relative(corpus, file).split(sep).join("/");
  const parts = rel.split("/");
  // Paths look like references/<topic>/<file>.md or assets/<file>.md; group by the topic.
  const group = parts.length > 2 ? parts[1] : parts.length === 2 ? parts[0] : "general";
  const stem = parts[parts.length - 1].replace(/\.md$/, "");
  if (!groups.has(group)) groups.set(group, { dir: parts.slice(0, -1).join("/"), files: [] });
  groups.get(group).files.push(stem);
}

const order = ["fundamentals", "harmony", "melody", "counterpoint", "rhythm-groove", "form", "orchestration", "instrument-idiom", "songwriting", "production-aware", "genres", "techniques", "creative-workflows", "research", "assets", "general"];
const seen = new Set();
let body = "";
for (const g of [...order, ...groups.keys()]) {
  if (seen.has(g) || !groups.has(g)) continue;
  seen.add(g);
  const { dir, files } = groups.get(g);
  body += `
**${g}** \`${dir}/\` — ${files.join(", ")}
`;
}

const header = `# Reference library index

Call \`read_reference\` with one of these paths when you need musical depth this prompt does not
give you — an unfamiliar genre, a harmonic device, how an instrument is actually written for.
Read at most three files in a turn, and only when they change what you write; the paths below are
the whole library, so never guess one.

Do not consult it for basics you already know, for FLVSTX's own tools or notation (that is in your
instructions), or before a routine "make another take".

Based on "Music Composition Agent Skill" by SJY051, licensed under CC BY 4.0.
`;

writeFileSync(join(root, "INDEX.md"), header + body, "utf8");
const count = [...groups.values()].reduce((n, v) => n + v.files.length, 0);
console.log(`INDEX.md: ${count} files in ${groups.size} groups`);
