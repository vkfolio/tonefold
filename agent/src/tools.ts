// Composer tools exposed to Claude. Every tool is a thin RPC into the plugin's session (Rust ops).
import { tool, createSdkMcpServer } from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";
import { existsSync, readFileSync, readdirSync, realpathSync, statSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import type { Rpc } from "./protocol.js";
import { AGENT_ROOT } from "./prompt.js";

const TRACK = z.string().describe("layer id (e.g. 'melody', 'arp2'), layer name, or kind name (first layer of that kind)");
const KIND = z.enum(["chords", "pad", "arpeggio", "pluck", "melody", "counter_melody", "harmony", "bass", "sub", "drums", "percussion"]);

function text(v: unknown) {
  if (typeof v === "string") return v;
  if (v && typeof v === "object") {
    const o = v as Record<string, unknown>;
    // Prefer human-readable fields when present.
    const parts: string[] = [];
    for (const k of ["summary", "analysis", "chords", "notation", "hint"]) {
      if (typeof o[k] === "string") parts.push(k === "summary" ? (o[k] as string) : `${k}:\n${o[k]}`);
    }
    if (parts.length) {
      const rest = { ...o };
      for (const k of ["summary", "analysis", "chords", "notation", "hint", "ok"]) delete rest[k];
      if (Object.keys(rest).length) parts.push(JSON.stringify(rest));
      return parts.join("\n");
    }
  }
  return JSON.stringify(v, null, 1);
}

function wrap(rpc: Rpc, method: string, onCall?: (name: string, input: unknown, result: string, ok: boolean) => void) {
  return async (input: Record<string, unknown>) => {
    try {
      const r = await rpc.call(method, input);
      const s = text(r);
      onCall?.(method, input, s, true);
      return { content: [{ type: "text" as const, text: s }] };
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      onCall?.(method, input, msg, false);
      return { content: [{ type: "text" as const, text: `ERROR: ${msg}` }], isError: true };
    }
  };
}

export const TOOL_NAMES = [
  "get_session", "get_notes", "set_key_tempo", "set_form", "set_section", "copy_section", "set_chords",
  "suggest_chords", "harmonize", "set_notes", "generate", "generate_all", "generate_song", "humanize", "analyze", "set_lyrics", "lock",
  "clear", "transpose", "undo", "export", "add_layer", "remove_layer", "set_arrangement", "list_layer_kinds",
  "set_instrument", "read_reference", "vary", "list_instruments", "set_groove", "redo", "list_scales",
];

// --- vendored reference library -------------------------------------------------------------
// The agent has no filesystem tools (see index.ts), so this is the only way it reaches the
// library. Everything is resolved under REF_ROOT and refused outside it.
const REF_ROOT = join(AGENT_ROOT, "reference", "music-composition");
const REF_CACHE = new Map<string, string>();
const MAX_CHARS = 12_000;

function referencePaths(): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    if (!existsSync(dir)) return;
    for (const name of readdirSync(dir).sort()) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (name.endsWith(".md")) out.push(relative(REF_ROOT, p).split(sep).join("/"));
    }
  };
  walk(REF_ROOT);
  return out;
}

/** Files whose path shares the most words with the request — so a wrong guess self-corrects. */
function nearest(query: string, limit = 6): string[] {
  const words = query.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);
  return referencePaths()
    .map((p) => ({ p, score: words.filter((w) => p.toLowerCase().includes(w)).length }))
    .sort((a, b) => b.score - a.score)
    .slice(0, limit)
    .map((x) => x.p);
}

/** Returns just the requested section of a document, heading included. */
function slice(body: string, heading: string): string | null {
  const lines = body.split(/\r?\n/);
  const want = heading.toLowerCase().replace(/^#+\s*/, "").trim();
  let start = -1;
  let depth = 0;
  for (let i = 0; i < lines.length; i++) {
    const m = /^(#+)\s+(.*)$/.exec(lines[i]);
    if (!m) continue;
    if (start === -1 && m[2].toLowerCase().trim().includes(want)) {
      start = i;
      depth = m[1].length;
    } else if (start !== -1 && m[1].length <= depth) {
      return lines.slice(start, i).join("\n");
    }
  }
  return start === -1 ? null : lines.slice(start).join("\n");
}

export async function readReference(input: Record<string, unknown>) {
  const rel = String(input.path ?? "");
  const err = (text: string) => ({ content: [{ type: "text" as const, text }], isError: true });
  if (!rel.endsWith(".md")) {
    return err(`path must be a .md file under the reference library. Closest: ${nearest(rel).join(", ")}`);
  }
  const target = resolve(REF_ROOT, rel);
  const root = realpathSync(REF_ROOT);
  if (!target.startsWith(root + sep) || !existsSync(target)) {
    return err(`no such reference '${rel}'. Closest: ${nearest(rel).join(", ")}`);
  }
  // Resolve symlinks too, so a link cannot point out of the library.
  if (!realpathSync(target).startsWith(root + sep)) {
    return err(`'${rel}' is outside the reference library`);
  }
  let body = REF_CACHE.get(target) ?? readFileSync(target, "utf8");
  REF_CACHE.set(target, body);
  if (typeof input.section === "string" && input.section.trim()) {
    const part = slice(body, input.section);
    if (!part) {
      const headings = body.split(/\r?\n/).filter((l) => /^#+\s/.test(l)).map((l) => l.replace(/^#+\s*/, "")).slice(0, 20);
      return err(`no section '${input.section}' in ${rel}. Headings: ${headings.join(" | ")}`);
    }
    body = part;
  }
  const cap = Math.min(Number(input.max_chars ?? MAX_CHARS) || MAX_CHARS, 20_000);
  if (body.length > cap) {
    const cut = body.lastIndexOf("\n#", cap);
    body = body.slice(0, cut > cap / 2 ? cut : cap) + `\n\n[truncated — call again with section="..." for the rest]`;
  }
  return { content: [{ type: "text" as const, text: body }] };
}

export function makeServer(rpc: Rpc, onCall?: (name: string, input: unknown, result: string, ok: boolean) => void) {
  const genParams = z
    .object({
      style: z.string().optional().describe("style hint, e.g. pop, lofi, trap, house, cinematic, kids"),
      energy: z.number().min(0).max(1).optional().describe("0 sparse/quiet .. 1 dense/loud; default = section energy"),
      seed: z.number().int().optional().describe("change to get a different take"),
      motif: z.string().optional().describe("melody only: 1-2 bar motif in melodic notation to develop, e.g. 'E4:8 G4:8 A4:4 G4:4 E4:4'"),
      from_section: z.string().optional().describe("melody only: reuse the motif of this section (default: first section with a melody)"),
      rate: z.enum(["4", "8", "16", "32", "8t", "16t"]).optional().describe("arpeggio rate"),
      octaves: z.number().int().min(1).max(3).optional().describe("arpeggio octaves"),
      contour: z.enum(["arch", "rise", "fall", "wave"]).optional(),
      lyrics: z.string().optional().describe("melody only: lyrics (one line per phrase) -> one syllable per note (kids/nursery mode)"),
      pattern: z.string().optional().describe("bass: root|root5|octave|walking|pedal|push|808|pulse; arpeggio: up|down|updown|random|chord; percussion: shaker|conga|tambourine|cowbell|mixed; harmony: third|sixth|above"),
      fills: z.boolean().optional().describe("drums only: fill on the last bar of each 4-bar phrase (default true)"),
      density: z.number().min(0).max(1).optional(),
      humanize: z.boolean().optional().describe("apply humanization (default true)"),
    })
    .optional();

  const tools = [
    tool("get_session", "Read the current session: key, tempo, style, sections with chords and which tracks have notes. Call this first in a conversation and after the user says they edited something.", { detail: z.enum(["summary", "full"]).optional() }, wrap(rpc, "get_session", onCall)),
    tool("get_notes", "Read one track's notes in one section as compact notation plus an analysis.", { track: TRACK, section: z.string() }, wrap(rpc, "get_notes", onCall)),
    tool("set_key_tempo", "Set key (e.g. 'C major', 'A minor', 'F# dorian'), tempo (BPM), style and/or time signature ('4/4', '3/4', '6/8').", { key: z.string().optional(), tempo: z.number().optional(), style: z.string().optional(), time_signature: z.string().optional() }, wrap(rpc, "set_key_tempo", onCall)),
    tool(
      "set_form",
      "Define the song sections in order (replaces the form; existing chords/notes for sections with the same name are kept unless overridden). Section ids are derived from names (lowercase alphanumerics).",
      {
        sections: z.array(z.object({ name: z.string(), bars: z.number().int().min(1).max(128), role: z.enum(["intro", "verse", "pre_chorus", "chorus", "bridge", "break", "build", "drop", "outro", "other"]).optional().describe("defaults from the name"), energy: z.number().min(0).max(1).optional(), chords: z.string().optional().describe("chord notation, e.g. '| C | Am | F | G |' or '| I | vi | IV | V |'"), lyrics: z.string().optional(), silent_layers: z.array(z.string()).optional().describe("layer ids that do not play in this section") })),
        keep_existing: z.boolean().optional(),
      },
      wrap(rpc, "set_form", onCall),
    ),
    tool("set_section", "Rename or resize a section, or change its energy or role.", { section: z.string(), name: z.string().optional(), bars: z.number().int().optional(), energy: z.number().optional(), role: z.enum(["intro", "verse", "pre_chorus", "chorus", "bridge", "break", "build", "drop", "outro", "other"]).optional() }, wrap(rpc, "set_section", onCall)),
    tool("add_layer", "Add a layer (track) of a kind: chords, pad, arpeggio, pluck, melody, counter_melody, harmony, bass, sub, drums, percussion. Returns its id and MIDI channel. Kinds derive from what exists (arpeggio/pad/pluck from chords, counter_melody/harmony from the melody).", { kind: KIND, name: z.string().optional() }, wrap(rpc, "add_layer", onCall)),
    tool("remove_layer", "Remove a layer.", { track: TRACK }, wrap(rpc, "remove_layer", onCall)),
    tool("set_arrangement", "Make a layer play or stay silent in a section (or all sections when section is omitted). Use it for intros/breaks/outros and to bring layers in gradually.", { track: TRACK, section: z.string().optional(), active: z.boolean() }, wrap(rpc, "set_arrangement", onCall)),
    tool("set_instrument", "Set the built-in General MIDI instrument a layer is auditioned with (name like 'Music Box', 'String Ensemble 1', 'Synth Bass 1', or program number 0-127; drums use the drum kit). Pick sounds that fit the style.", { track: TRACK, instrument: z.string() }, wrap(rpc, "set_instrument", onCall)),
    tool("list_layer_kinds", "List the available layer kinds with descriptions.", {}, wrap(rpc, "list_layer_kinds", onCall)),
    tool("generate_song", "Generate every active, unlocked layer in every section in dependency order, with melodic continuity across sections (chorus restates the verse motif higher) and section-role contrast (intro/break sparse, chorus/drop full, build rises into the next section).", { params: genParams }, wrap(rpc, "generate_song", onCall)),
    tool("copy_section", "Copy chords and all track notes from one section to another (e.g. verse -> verse2 before varying it).", { from: z.string(), to: z.string() }, wrap(rpc, "copy_section", onCall)),
    tool(
      "set_chords",
      "Set the chord progression of a section. One bar per '| … |' cell; several chords in a bar split it evenly ('| C Am |'); '.' continues the previous chord ('| C . Am . |' = C for half a bar then Am). Symbols (C, Am7, Fmaj7, G7sus4, Dm7/F) or roman numerals (I, vi, IV, V7, bVII, ii°, V/V) relative to the key. If fewer bars are written than the section has, the pattern repeats.",
      { section: z.string(), notation: z.string() },
      wrap(rpc, "set_chords", onCall),
    ),
    tool(
      "harmonize",
      "Fit chord progressions to the melody already in a section (melody-first workflow). Scores candidate chords against the melody's notes with functional movement and cadences. Returns up to `count` distinct progressions (simple / diatonic / rich, 1 or 2 chords per bar); pass apply=<index> to set one on the section. Then generate chords/bass/drums.",
      { section: z.string(), complexity: z.enum(["simple", "diatonic", "rich"]).optional(), count: z.number().int().min(1).max(6).optional(), apply: z.number().int().min(0).optional() },
      wrap(rpc, "harmonize", onCall),
    ),
    tool("suggest_chords", "Get a few idiomatic 4-bar progressions for the current key and a style, as chord symbols with roman numerals. Use as raw material, then adapt.", { style: z.string().optional(), count: z.number().int().min(1).max(8).optional(), seed: z.number().int().optional() }, wrap(rpc, "suggest_chords", onCall)),
    tool(
      "set_notes",
      "Write explicit notes for a track in a section (replaces the clip). Melody/bass/chords notation: tokens 'PITCH:DUR' where DUR is 1,2,4,8,16,32 (add '.' for dotted, 't' for triplet), 'r:DUR' rests, '~' ties to the next same pitch, '@v90' velocity, '/syl' lyric; bar lines '|' optional. Example: 'E4:8 E4:8 F4:8 G4:4. r:8 | G4:4 F4:4 E4:2'. Drums notation: one lane per line, 16th steps: 'K: x---x---x---x---' 'S: ----x-------x-g-' 'H: x-x-x-x-x-x-x-x-' (X accent, g ghost, f flam; lanes K S CL H OH RS T1 T2 T3 RD CR P). '@t+12' places a note 12 ms late against the grid ('@t-8' early) when you want a deliberate push or drag. Notes past the section end are dropped. Humanization is applied unless humanize=false. Pass from_bar (and to_bar) to rewrite only those bars — fixing bar 3 does not mean resending the whole clip.",
      { track: TRACK, section: z.string(), notation: z.string(), humanize: z.boolean().optional(), from_bar: z.number().int().min(1).optional(), to_bar: z.number().int().min(1).optional() },
      wrap(rpc, "set_notes", onCall),
    ),
    tool("generate", "Generate one track for a section with the rule engine (voice-led chords, motif-developed melody, bass pattern, groove drums), then humanize. Returns the notation and an analysis. Re-run with a different seed for another take, or pass a motif/lyrics/pattern to steer it.", { track: TRACK, section: z.string(), params: genParams }, wrap(rpc, "generate", onCall)),
    tool("generate_all", "Generate every active, unlocked layer of a section in dependency order (chords, melody, then pads/arps/plucks, bass/sub, counter/harmony, drums/percussion).", { section: z.string(), params: genParams }, wrap(rpc, "generate_all", onCall)),
    tool(
      "humanize",
      "Re-apply humanization to a track (all sections or one). Params override the style preset: timing_ms (the real millisecond spread, 3 tight .. 15 loose), pocket_ms (negative = laid back, positive = pushed), snare_pocket_ms (backbeat placement: -5 pop and rock, +12 hip-hop and R&B), pocket_spread (how far a kit spreads around the pocket), swing (0.5 straight .. 0.66 heavy), swing_16ths, vel_jitter (0-0.15), accent (0-1), phrase_arc (0-1), gate_var (0-0.2), seed.",
      {
        track: TRACK,
        section: z.string().optional(),
        params: z
          .object({
            timing_ms: z.number().min(0).max(40).optional(),
            pocket_ms: z.number().min(-40).max(40).optional(),
            swing: z.number().min(0.5).max(0.75).optional(),
            swing_16ths: z.boolean().optional(),
            vel_jitter: z.number().min(0).max(0.3).optional(),
            accent: z.number().min(0).max(1).optional(),
            phrase_arc: z.number().min(0).max(1).optional(),
            gate_var: z.number().min(0).max(0.3).optional(),
            snare_pocket_ms: z.number().min(-30).max(30).optional(),
            pocket_spread: z.number().min(0).max(3).optional(),
            seed: z.number().int().optional(),
          })
          .optional(),
      },
      wrap(rpc, "humanize", onCall),
    ),
    tool("analyze", "Analyze tracks: range, leaps, out-of-key notes, strong-beat non-chord tones, density per bar, rests, velocity spread, warnings. Call at the end of a turn to check your work.", { track: TRACK.optional(), section: z.string().optional() }, wrap(rpc, "analyze", onCall)),
    tool("set_lyrics", "Attach lyrics to a section (one line per phrase). Used by the kids/nursery melody generator: one syllable per note.", { section: z.string(), lyrics: z.string() }, wrap(rpc, "set_lyrics", onCall)),
    tool("lock", "Lock or unlock a track so generators leave it alone.", { track: TRACK, locked: z.boolean() }, wrap(rpc, "lock", onCall)),
    tool("clear", "Clear notes of a track (all sections or one), or everything.", { track: TRACK.optional(), section: z.string().optional() }, wrap(rpc, "clear", onCall)),
    tool("transpose", "Transpose notes by semitones (pitched tracks only unless a track is named).", { semitones: z.number().int(), track: TRACK.optional(), section: z.string().optional() }, wrap(rpc, "transpose", onCall)),
    tool("undo", "Undo the last change.", {}, wrap(rpc, "undo", onCall)),
    tool("redo", "Redo the change you just undid.", {}, wrap(rpc, "redo", onCall)),
    tool(
      "vary",
      "Regenerate a section as a variation of itself: same harmony, same motif, a different performance. Use after copy_section (which copies verbatim) so a repeated chorus develops instead of repeating, or when the user asks for 'the same but different'.",
      { section: z.string(), track: TRACK.optional().describe("one layer, or all unlocked layers of the section"), amount: z.number().min(0).max(1).optional().describe("0 subtle .. 1 far from the original"), seed: z.number().int().optional() },
      wrap(rpc, "vary", onCall),
    ),
    tool(
      "set_groove",
      "Set the groove template for the song or one layer: straight_pop, swing_16 (MPC 16ths), boom_bap (dragged backbeat), house, jazz_swing, tresillo (3+3+2), or none for dead straight. Defaults come from the style.",
      { groove: z.string(), track: TRACK.optional() },
      wrap(rpc, "set_groove", onCall),
    ),
    tool("list_instruments", "List the General MIDI instruments set_instrument accepts, with their program numbers.", {}, wrap(rpc, "list_instruments", onCall)),
    tool("list_scales", "List the scales set_key_tempo accepts.", {}, wrap(rpc, "list_scales", onCall)),
    tool(
      "read_reference",
      "Read one file from the vendored music-composition reference library (harmony, melody, groove, form, orchestration, instrument idiom, 24+ genres). Pick a path from the index in your instructions; never guess. Use it for musical depth you do not already have — an unfamiliar genre, a harmonic device, how an instrument is really written for — at most three files per turn.",
      {
        path: z.string().describe("path from the index, e.g. 'references/genres/afrobeats-and-amapiano.md'"),
        section: z.string().optional().describe("return only this heading's section"),
        max_chars: z.number().int().min(500).max(20000).optional(),
      },
      async (input: Record<string, unknown>) => {
        const r = await readReference(input);
        onCall?.("read_reference", input, r.content[0].text.slice(0, 200), !("isError" in r));
        return r;
      },
    ),
    tool("export", "Write the current song (or one section) as latest.json + .mid files for FL Studio (the FLVSTX Import piano-roll script reads them). Only when the user asks to export/commit.", { section: z.string().optional() }, wrap(rpc, "export", onCall)),
  ];
  return createSdkMcpServer({ name: "flvstx", version: "0.1.0", tools });
}
