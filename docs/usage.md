# FLVSTX — using it in FL Studio

## Build and install

```
cargo xtask bundle flvstx-plugin --release
cd agent && npm install && npm run build && cd ..
powershell -ExecutionPolicy Bypass -File scripts/install.ps1
```

`install.ps1` copies `FLVSTX.clap` to `%LOCALAPPDATA%\Programs\Common\CLAP`, `FLVSTX.vst3` to
`%LOCALAPPDATA%\Programs\Common\VST3`, and the piano-roll script to
`Documents\Image-Line\FL Studio\Settings\Piano roll scripts`. In FL Studio run
**Options > Manage plugins > Find more plugins** (make sure both per-user folders are in the search
paths; add them with the folder icon if not) and enable FLVSTX. Prefer the **CLAP** build in FL Studio.

The composer sidecar needs Node 22 and your Claude Code login (`claude` must be logged in on this
PC). The plugin starts `node agent/dist/index.js --port 7878` on the first chat message; set
`FLVSTX_AGENT_DIR` if the repo is not at `D:\FLVSTX`, `FLVSTX_PORT` to change the port, and
`FLVSTX_MODEL` to force a model.

## Layers and sections

A song is a list of **sections** (each with a role: intro, verse, pre-chorus, chorus, bridge, break, build,
drop, outro; and an energy 0-100%) and a list of **layers**. A new session has chords, melody, bass and
drums; add more from the Layers panel (`+ Layer`): pad, arpeggio, pluck, counter-melody, harmony, sub,
percussion. Each layer has its own MIDI channel (shown under its name). The **Arrangement** checkbox shows
the layers × sections grid: click a cell to silence or enable a layer in that section (intros and breaks
start sparse by default).

Generation is layered and order-independent: whatever exists constrains the next suggestion.
`Suggest <layer>` gives three takes for the selected layer (chords fitted to an existing melody, melody over
existing chords, arps/pads/plucks from the chords, counter-melody and harmony from the lead, bass/sub from
chords, drums/percussion by energy). `Keep` locks the layer. `Generate section` fills every unlocked layer of
the section; `Generate song` fills the whole song with continuity (later sections reuse the first melody's
motif, the final chorus is the biggest, builds roll into the next section, intros/breaks are sparse).

The chat header has a model picker (default / sonnet / opus) that applies to the next message.

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
   instance per instrument and pick that layer's channel on each. Instances do not share a session yet: generate in one, **Export song**, and import the .mid
   files into the others' target channels if you want separate instruments.
4. Edit notes in the piano roll: double-click adds, drag moves, drag the right edge resizes,
   right-click or Delete removes, ↑/↓ transposes (Shift = octave), Ctrl+A selects all, Ctrl+D
   duplicates, drag in the velocity lane sets velocity, Ctrl+wheel zooms. The composer sees your
   edits on its next turn.
5. Commit to FL: **Export section** / **Export song** writes `%LOCALAPPDATA%\FLVSTX\export\latest.json`,
   `latest.mid` and one `.mid` per track. Then either
   - open the target channel's piano roll and run **Tools > Scripts > FLVSTX Import** (choose the
     track; it merges into the existing notes at the timeline selection), or
   - drag a `.mid` file from the export folder (**Open folder** button) onto a Channel Rack slot.

The session, chat history and composer session id are saved inside the FL project.

## Command line

```
cargo run -p flvstx-cli -- demo --style lofi --key "F major" --bars 8 --out demo.mid
cargo run -p flvstx-cli -- chat session.json          # terminal chat through the sidecar
cargo run -p flvstx-cli -- op session.json generate_all '{"section":"verse"}'
cargo run -p flvstx-plugin --release --features standalone   # GUI without a DAW
```
