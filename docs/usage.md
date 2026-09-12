# FLVSTX — using it in FL Studio

## Install

From a release package — unzip it and run:

```
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

From a checkout, build first, then run the same command:

```
cargo xtask bundle flvstx-plugin --release
cargo build --release -p flvstx-plugin --features standalone --bin flvstx-standalone
cargo build --release -p flvstx-cli
cd agent && npm ci && npm run build && cd ..
```

`scripts\package.ps1` does all of that and stages a release zip under `dist\`.

`install.ps1` copies `FLVSTX.clap` to `C:\Program Files\Common Files\CLAP`, `FLVSTX.vst3` to
`C:\Program Files\Common Files\VST3`, and the piano-roll script to
`Documents\Image-Line\FL Studio\Settings\Piano roll scripts`. In FL Studio run
**Options > Manage plugins > Find installed plugins** (the machine-wide Common Files folders are scanned
by default; do not add custom paths, FL treats added paths as VST2) and favourite FLVSTX. Prefer the **CLAP** build in FL Studio.

The composer sidecar needs Node 22 and your Claude Code login (`claude` must be logged in on this
PC) — or an Ollama server, see below. The plugin starts `node agent/dist/index.js --port 7878` on
the first chat message; set `FLVSTX_AGENT_DIR` if the repo is not at `D:\FLVSTX`, `FLVSTX_PORT` to
change the port, and `FLVSTX_MODEL` to force a model. `FLVSTX_PROVIDER=ollama` and
`FLVSTX_OLLAMA_URL` make Ollama the sidecar's default; `FLVSTX_OLLAMA_NUM_CTX` (default 16384) is
the context window it asks a local model for, and `FLVSTX_OLLAMA_THINK=1` makes **Think** the default.

### Running on Ollama instead of Claude

The chat header has a **Claude / Ollama** switch. On Ollama, type the server's URL (the default
`http://localhost:11434` is the Ollama on this PC; another machine works too if Ollama there was
started with `OLLAMA_HOST=0.0.0.0`), pick a model from the list the server reports, and chat as
usual. **↻** asks the server again after you `ollama pull` something. **Think** lets a thinking
model reason before it answers: better on hard requests, much slower on a small GPU, and the
reasoning shows in the transcript as it goes (collapsed once the answer arrives). The same box is
there for Claude, on by default (adaptive: Claude decides how much); off is faster and cheaper on
simple requests. Each provider remembers its own setting, and the choices are saved with the
project. The terminal chat has the same: `/ollama [URL]`, `/models`, `/model NAME`,
`/think on|off`, `/claude`.

What to expect:

- **Pick a model with tool calling** — `ollama show NAME` lists `tools` under Capabilities. The
  composer works entirely through tools; a model without them can only talk.
- **Size matters, both ways.** Every turn carries the composer's whole brief and 32 tool schemas
  (about 10k tokens), so very small models lose the thread. But a model that does not fit your
  GPU runs on the CPU at a couple of tokens a second: on an 8 GB card a 27B model is a
  ten-minute turn, an 8B one (`ollama pull qwen3:8b`) is well under a minute. `ollama ps` shows
  how much of the model is in VRAM. The first turn after a model loads is the slowest.
- **Producer mode** runs the same specialists on the local model. It works, but it is many long
  turns, and a model that mislabels a layer or skips a step will be caught by the checklist rather
  than by its own care.
- **No cost is shown**: nothing leaves your machine (or your network, for a remote server).
- Claude and Ollama keep separate conversations. Switching back to Claude resumes where Claude
  left off.

## Editor layout and composer

Use **Composer** in the top toolbar to show or hide chat and make room for the piano roll.
Drag panel dividers to resize them; **Settings > Display size** changes text and control sizes. MIDI output routing also lives in Settings.
Section tabs scroll horizontally, and the arrangement grid scrolls when the song grows.

The composer keeps its message input below the transcript. **Quick ideas** and the starter
buttons fill an editable draft; press **Send request** or Enter when ready. Shift+Enter adds
a new line. Consecutive tool calls appear as one expandable **composition steps** entry. **Chat options > Clear
transcript** clears visible messages while retaining the song and composer session.

The piano roll's **Help** menu lists editing gestures and shortcuts. **Fit notes** frames the selected
section and its pitch range; hide Composer for a wider editing area. Layer cards show a miniature
note preview and separate Mute, Lock, and MIDI drag controls. Expand **Section details** to edit
a section. Composition actions sit above the piano roll; playback and export sit below it.

### Navigating the piano roll

- Enable **Pan** and drag to move the view in both directions. Turn Pan off to edit notes.
- Middle-mouse drag pans without switching modes.
- Scroll moves through pitches; Shift+scroll or scrolling over the ruler moves through time.
- Use the right pitch scrollbar and bottom time scrollbar for direct navigation.
- **Time -/+** zooms horizontally; **Pitch -/+** changes row height.
- Ctrl+scroll zooms time; Ctrl+Shift+scroll (or Ctrl+scroll over the keys) zooms pitch.
- **Fit notes** resets both axes. When the whole section fits, zoom in to pan horizontally.

## Layers and sections

A song is a list of **sections** (each with a role: intro, verse, pre-chorus, chorus, bridge, break, build,
drop, outro; and an energy 0-100%) and a list of **layers**. A new session has chords, melody, bass and
drums; add more from the Layers panel (`+ Layer`): pad, arpeggio, pluck, counter-melody, harmony, sub,
percussion. Each layer has its own MIDI channel (shown under its name). The **Arrangement** checkbox shows
the layers × sections grid: click a cell to silence or enable a layer in that section (intros and breaks
start sparse by default).

Generation is layered and order-independent: whatever exists constrains the next suggestion.
`Suggest <layer>` gives three takes for the selected layer from the rule engine (chords fitted to an existing
melody, melody over existing chords, arps/pads/plucks from the chords, counter-melody and harmony from the
lead, bass/sub from chords, drums/percussion by energy) — instant, free, and a different roll of the same
dice each time. `Ask AI` beside it hands that one layer to the composer instead: it reads the section, picks
the approach from the style and what the other layers are doing — including writing the notes itself when it
has a specific idea — checks its work and says what it did in the chat. Slower and it costs a turn, so reach
for it when the engine's takes are all the same shape and you want judgement rather than another seed.
`Keep` locks the layer. `Generate section` fills every unlocked layer of
the section; `Generate song` fills the whole song with continuity (later sections reuse the first melody's
motif, the final chorus is the biggest, builds roll into the next section, intros/breaks are sparse).

The chat header has a model picker (default / sonnet / opus) that applies to the next message, and
the Claude / Ollama switch described above.

## Built-in sounds, drag-out, standalone

FLVSTX renders its own audio with a General MIDI soundfont (GeneralUser GS, downloaded by the installer to
`%LOCALAPPDATA%\FLVSTX\soundfont`). Each layer has an instrument (piano roll toolbar dropdown; the composer
can set it too); the **Built-in sound** toggle and volume are in the bottom bar. Only one plugin instance
per project renders audio. Layers are dragged into FL Studio with the **⇗** handle in the Layers panel
(drop on a Channel Rack instrument or an open piano roll; Shift = selected section only). The installer also
puts a standalone app in the Start Menu (**FLVSTX**), which uses WASAPI audio.

## Workflow

1. Add FLVSTX as an instrument in the Channel Rack. Open it and type a brief in the chat
   ("lo-fi loop in F, 8 bars", "kids song about brushing teeth", "epic cinematic intro in D minor").
2. The composer sets key/tempo/form, chords, then generates the parts and explains what it did.
   Use the quick-action buttons for common iterations, or keep chatting ("make the chorus busier",
   "different melody, keep the chords", "swing it more").
3. Hear it: **Play** loops the selected section through the plugin's MIDI output; **Sync to host**
   follows FL's transport instead. Route the output to an instrument: in the FLVSTX wrapper window
   set **MIDI > Output port** to e.g. 10, and on the target instrument (FLEX, FPC, …) set
   **MIDI > Input port** to 10. Every layer goes out on its own MIDI channel (shown in the Layers
   panel); the plugin's **MIDI output** parameter selects one channel per instance, so add one FLVSTX
   instance per instrument and pick that layer's channel on each. All instances in the project share one session, so each can send a different layer.
4. Move the playhead by dragging the bar ruler at the top of the piano roll; **Play** starts from
   there. Click the piano keys down the left edge to hear a note (drag for a glissando); keys the layer
   already uses are marked with a dot.
5. Edit notes in the piano roll: double-click adds, drag moves, drag the right edge resizes,
   right-click or Delete removes, ↑/↓ transposes (Shift = octave), Ctrl+A selects all, Ctrl+D
   duplicates, drag in the velocity lane sets velocity, Ctrl+wheel zooms. The composer sees your
   edits on its next turn.
6. Commit to FL: **Export… > MIDI** (whole song or this section) asks for a folder, then writes `latest.json`,
   `latest.mid` (all tracks) and one `.mid` per track into it. The same files are refreshed in
   `%LOCALAPPDATA%\FLVSTX\export\` too, which is where the import script reads them. Then either
   - open the target channel's piano roll and run **Tools > Scripts > FLVSTX Import** (choose the
     track; it merges into the existing notes at the timeline selection), or
   - drag a `.mid` file from the export folder (**Open folder** reopens the last one) onto a Channel Rack slot,
   - or drag a layer's **⇗** handle in the Layers panel straight onto an FL channel or its piano roll.

**Export… > Audio** renders the built-in sounds offline into the folder you pick: `song.wav` (the mix)
plus `song-<layer>.wav` for every layer that has notes, as stereo 16-bit 44.1 kHz. Section renders are
named after the section instead (`Verse.wav`, `Verse-melody.wav`, …). Muted layers are left out
entirely, every stem is rendered at the mix's gain so the stems add back up to the mix, and the mix is
turned down rather than clipped if it goes over. Use it to send a take to someone, to check an idea
without a DAW, or to drop the stems into a DAW as audio.

### How it is made to sound played

Generation is seeded from the section and its occurrence, so a repeated chorus is a new performance
rather than a copy of the first. Each layer is then placed by a **groove template** (the systematic
way a style leans on the grid — `straight_pop`, `swing_16`, `boom_bap`, `house`, `jazz_swing`,
`tresillo`, or `none`) and a per-layer random walk whose `timing_ms` is a real millisecond spread.
Drums sit in a pocket: the kick a little behind, hats ahead, and the backbeat placed by style.
Sustained parts also carry controller lanes — CC11 swells, sustain pedal at chord changes,
pitch-bend glides on 808 basses — which play through the built-in sounds, ride along in exported
MIDI, and follow a layer dragged into FL Studio.

Ask the composer for "the same but different" (it calls `vary`), a feel (`set_groove`), or a
deliberate push on one note (`@t-15` in its notation).

### The composer's reference library

`agent/reference/` holds a vendored composition library — harmony, motif development, phrase
structure, groove, form, orchestration, instrument idiom and 24+ genres. The composer reads from it
on demand through its `read_reference` tool, guided by an index in its prompt, so an unfamiliar
genre comes from a reference rather than from memory. Based on "Music Composition Agent Skill" by
SJY051, licensed under CC BY 4.0 — see `agent/reference/NOTICE.md`.

**Song…** (top bar) keeps songs as files: **New**, **Open…**, **Save**, **Save as…**. A `.flvstx` file
holds the whole session — sections, layers, notes, chat and the composer session — so you can come back
to a song later or keep several going.

Inside FL Studio the session is also saved in the FL project, so a project reload restores it. The
standalone app instead autosaves to `%LOCALAPPDATA%\FLVSTXutosave.flvstx` as you work and reopens
that song on the next launch, so nothing is lost if you close it.

## Command line

```
cargo run -p flvstx-cli -- demo --style lofi --key "F major" --bars 8 --out demo.mid
cargo run -p flvstx-cli -- chat session.json          # terminal chat through the sidecar
cargo run -p flvstx-cli -- op session.json generate_all '{"section":"verse"}'
cargo run -p flvstx-plugin --release --features standalone   # GUI without a DAW
```
