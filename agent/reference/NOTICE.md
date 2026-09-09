# Vendored reference library

`music-composition/` is a copy of the **Music Composition Agent Skill** — a modular reference
library on harmony, melody, form, rhythm and groove, orchestration, instrument idiom and 24+
genres, written for AI assistants.

> Based on "Music Composition Agent Skill" by SJY051, licensed under CC BY 4.0.
> Source: https://github.com/SJY051/music-composition
> Upstream commit: 07cecf9c8fd15249ea3da311dc9a7c7893ff801f (v1.0, 2026-04-27)

## Licence

The documents are licensed **CC BY 4.0** (`LICENSE-upstream.md`); attribution as above is
required wherever they are redistributed. The upstream `scripts/` directory is MIT and is not
vendored here.

## Changes

- Only `references/` and `assets/` are vendored, under `music-composition/`.
- `references/validation/` (the upstream project's own QA records) was dropped.
- The upstream release notes, roadmap, maintenance, benchmark and README files were dropped.
- No file that is kept has been edited.

## How the composer reads it

The agent cannot open files directly (`agent/src/index.ts` denies the filesystem tools). It calls
the `read_reference` tool instead, choosing a path from the index in its system prompt. Ours is the
engine knowledge — our tools, our notation, our humanize knobs — this is the music knowledge.
