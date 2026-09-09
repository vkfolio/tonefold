// The specialists the producer delegates to. Each one is a fresh context with its own craft prompt
// and a tool set scoped to what it is allowed to touch — the point is depth, not headcount: the
// context that chooses a progression should not also be full of hi-hat programming.
//
// Boundaries are enforced by the fence in producer.ts, not asked for in the prompts, because a
// specialist that quietly rewrites the melody breaks the plan the user approved.

import type { AgentDefinition } from "@anthropic-ai/claude-agent-sdk";
import { specialistPrompt } from "./prompt.js";

/** Layer kinds, longest first so `counter_melody` is never read as `melody`. */
const LAYER_KINDS = [
  "counter_melody",
  "percussion",
  "arpeggio",
  "harmony",
  "melody",
  "chords",
  "drums",
  "pluck",
  "bass",
  "pad",
  "sub",
];

/**
 * The layer kind a `track` argument names. Layer ids are the kind, optionally numbered
 * (`model.rs: add_track`), so a prefix match is exact for ids and for kind names alike. Returns
 * undefined for anything we cannot place — an unknown name is left to the session to reject.
 */
export function kindOf(track: string): string | undefined {
  const t = track.trim().toLowerCase();
  return LAYER_KINDS.find((k) => t === k || t.startsWith(k));
}

/**
 * What each specialist may write. Note `harmony` is the *vocal harmony line* and belongs to the
 * topline writer; the chord layer is `chords`. Reads are never restricted.
 */
export const DOMAIN: Record<string, string[]> = {
  "harmony-form": ["chords", "pad"],
  "melody-topline": ["melody", "counter_melody", "harmony"],
  "rhythm-section": ["drums", "percussion", "bass", "sub"],
  "arrangement-mix": LAYER_KINDS,
};

const READ = ["get_session", "get_notes", "analyze", "read_reference", "list_layer_kinds", "list_scales", "list_instruments"];

const mcp = (names: string[]) => names.map((n) => `mcp__flvstx__${n}`);

export const SPECIALISTS: Record<string, AgentDefinition> = {
  "harmony-form": {
    description:
      "Key, tempo, sections and chord progressions, plus the chords and pad layers. Use for the harmonic and structural skeleton of a song, before any part that has to follow it.",
    // `model` is deliberately absent: the panel's picker governs every agent in the run.
    prompt: specialistPrompt("harmony-form"),
    // `add_layer`/`set_instrument` are here so a missing pad does not block the whole step; the
    // fence still holds them to chords and pad.
    tools: mcp([...READ, "suggest_chords", "set_key_tempo", "set_form", "set_section", "copy_section", "set_chords", "harmonize", "generate", "set_notes", "add_layer", "set_instrument"]),
    maxTurns: 30,
  },
};

export const SPECIALIST_NAMES = Object.keys(SPECIALISTS);
