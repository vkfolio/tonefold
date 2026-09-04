// Composer tools exposed to Claude. Every tool is a thin RPC into the plugin's session (Rust ops).
import { tool, createSdkMcpServer } from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";
import type { Rpc } from "./protocol.js";

const TRACK = z.enum(["chords", "melody", "bass", "drums"]);

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
  "suggest_chords", "set_notes", "generate", "generate_all", "humanize", "analyze", "set_lyrics", "lock",
  "clear", "transpose", "undo", "export",
];

export function makeServer(rpc: Rpc, onCall?: (name: string, input: unknown, result: string, ok: boolean) => void) {
  const genParams = z
    .object({
      style: z.string().optional().describe("style hint, e.g. pop, lofi, trap, house, cinematic, kids"),
      energy: z.number().min(0).max(1).optional().describe("0 sparse/quiet .. 1 dense/loud; default = section energy"),
      seed: z.number().int().optional().describe("change to get a different take"),
      motif: z.string().optional().describe("melody only: 1-2 bar motif in melodic notation to develop, e.g. 'E4:8 G4:8 A4:4 G4:4 E4:4'"),
      contour: z.enum(["arch", "rise", "fall", "wave"]).optional(),
      lyrics: z.string().optional().describe("melody only: lyrics (one line per phrase) -> one syllable per note (kids/nursery mode)"),
      pattern: z.enum(["root", "root5", "octave", "walking", "pedal", "push", "808", "pulse"]).optional().describe("bass only"),
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
        sections: z.array(z.object({ name: z.string(), bars: z.number().int().min(1).max(128), energy: z.number().min(0).max(1).optional(), chords: z.string().optional().describe("chord notation, e.g. '| C | Am | F | G |' or '| I | vi | IV | V |'"), lyrics: z.string().optional() })),
        keep_existing: z.boolean().optional(),
      },
      wrap(rpc, "set_form", onCall),
    ),
    tool("set_section", "Rename or resize a section, or change its energy.", { section: z.string(), name: z.string().optional(), bars: z.number().int().optional(), energy: z.number().optional() }, wrap(rpc, "set_section", onCall)),
    tool("copy_section", "Copy chords and all track notes from one section to another (e.g. verse -> verse2 before varying it).", { from: z.string(), to: z.string() }, wrap(rpc, "copy_section", onCall)),
    tool(
      "set_chords",
      "Set the chord progression of a section. One bar per '| … |' cell; several chords in a bar split it evenly ('| C Am |'); '.' continues the previous chord ('| C . Am . |' = C for half a bar then Am). Symbols (C, Am7, Fmaj7, G7sus4, Dm7/F) or roman numerals (I, vi, IV, V7, bVII, ii°, V/V) relative to the key. If fewer bars are written than the section has, the pattern repeats.",
      { section: z.string(), notation: z.string() },
      wrap(rpc, "set_chords", onCall),
    ),
    tool("suggest_chords", "Get a few idiomatic 4-bar progressions for the current key and a style, as chord symbols with roman numerals. Use as raw material, then adapt.", { style: z.string().optional(), count: z.number().int().min(1).max(8).optional(), seed: z.number().int().optional() }, wrap(rpc, "suggest_chords", onCall)),
    tool(
      "set_notes",
      "Write explicit notes for a track in a section (replaces the clip). Melody/bass/chords notation: tokens 'PITCH:DUR' where DUR is 1,2,4,8,16,32 (add '.' for dotted, 't' for triplet), 'r:DUR' rests, '~' ties to the next same pitch, '@v90' velocity, '/syl' lyric; bar lines '|' optional. Example: 'E4:8 E4:8 F4:8 G4:4. r:8 | G4:4 F4:4 E4:2'. Drums notation: one lane per line, 16th steps: 'K: x---x---x---x---' 'S: ----x-------x-g-' 'H: x-x-x-x-x-x-x-x-' (X accent, g ghost, f flam; lanes K S CL H OH RS T1 T2 T3 RD CR P). Notes past the section end are dropped. Humanization is applied unless humanize=false.",
      { track: TRACK, section: z.string(), notation: z.string(), humanize: z.boolean().optional() },
      wrap(rpc, "set_notes", onCall),
    ),
    tool("generate", "Generate one track for a section with the rule engine (voice-led chords, motif-developed melody, bass pattern, groove drums), then humanize. Returns the notation and an analysis. Re-run with a different seed for another take, or pass a motif/lyrics/pattern to steer it.", { track: TRACK, section: z.string(), params: genParams }, wrap(rpc, "generate", onCall)),
    tool("generate_all", "Generate chords, melody, bass and drums for a section in one go (locked tracks are skipped).", { section: z.string(), params: genParams }, wrap(rpc, "generate_all", onCall)),
    tool(
      "humanize",
      "Re-apply humanization to a track (all sections or one). Params override the style preset: timing_ms (looseness at 16th level, 3-15), pocket_ms (negative = laid back, positive = pushed), swing (0.5 straight .. 0.66 heavy), swing_16ths, vel_jitter (0-0.15), accent (0-1), phrase_arc (0-1), gate_var (0-0.2), seed.",
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
    tool("export", "Write the current song (or one section) as latest.json + .mid files for FL Studio (the FLVSTX Import piano-roll script reads them). Only when the user asks to export/commit.", { section: z.string().optional() }, wrap(rpc, "export", onCall)),
  ];
  return createSdkMcpServer({ name: "flvstx", version: "0.1.0", tools });
}
