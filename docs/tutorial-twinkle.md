# FLVSTX tutorial: a complete "Twinkle Twinkle Little Star" song in FL Studio

This walks through everything from zero: installing, opening the plugin in FL Studio, hearing it,
building the song layer by layer, arranging it into a ~2 minute piece, and getting every part into FL
Studio channels with your own instruments. Every step says what you should see.

Terms used below:
- **Layer** = one musical part (melody, chords, bass, drums, arp, pad…). Each layer is one instrument in FL.
- **Section** = a part of the song (intro, verse, chorus…). The song is a list of sections.
- **Take** = one of three alternatives the plugin offers when you press *Suggest*.
- **Composer** = the chat on the left, powered by Claude.

---

## Part 1 · Install (once)

You need: FL Studio 2024 or newer, Node.js 22, Rust (for building), and Claude Code logged in on this PC.

1. Open a terminal in `D:\FLVSTX` and build everything:
   ```
   cargo xtask bundle flvstx-plugin --release
   cd agent
   npm install
   npm run build
   cd ..
   powershell -ExecutionPolicy Bypass -File scripts\install.ps1
   ```
   The last command prints three lines: the CLAP file, the VST3 file, and the piano-roll script it copied.
2. Check Claude is logged in: run `claude` in a terminal once; if it asks you to log in, do it, then exit.

You should now have:
- `%LOCALAPPDATA%\Programs\Common\CLAP\FLVSTX.clap`
- `%LOCALAPPDATA%\Programs\Common\VST3\FLVSTX.vst3`
- `Documents\Image-Line\FL Studio\Settings\Piano roll scripts\FLVSTX Import.pyscript`

## Part 2 · Make FL Studio see the plugin (once)

1. Open FL Studio. Menu **Options > Manage plugins**.
2. On the left, under *Plugin search paths*, make sure these folders are listed (add them with the folder
   icon if not):
   - `C:\Users\<you>\AppData\Local\Programs\Common\CLAP`
   - `C:\Users\<you>\AppData\Local\Programs\Common\VST3`
3. Click **Find more plugins** (top left). Wait for the scan.
4. In the list, find **FLVSTX**. You will see two entries (CLAP and VST3). Tick the star / **Favorite**
   on the **CLAP** one so it appears in the Add menu. Use CLAP in FL Studio; VST3 MIDI output is unreliable in FL.
5. Close Manage plugins.

## Part 3 · Set up a project for the song (every new song)

1. **File > New**. Set the project tempo to **100** (click the BPM display and type 100).
2. Add the plugin: **Add > FLVSTX** (or press F8 for the Plugin picker and drag FLVSTX in). It appears in the
   Channel Rack. Its window opens: chat on the left, piano roll in the middle, Layers on the right.
3. Add an instrument to hear the melody: **Add > FLEX** (or any synth/piano). Pick a bright sound, for
   example a music box, xylophone, or piano preset.
4. Route FLVSTX into that instrument:
   - In the FLVSTX window, click the small **wrapper settings** icon (the gear/plug icon at the top-left
     of the plugin window) and open the **MIDI** tab. Set **Output port** to **1**.
   - In the FLEX window, same gear icon, **MIDI** tab: set **Input port** to **1**.
   Both must show the same number.
5. Tell FLVSTX which layer this instrument should play: in the FLVSTX wrapper, the plugin has one
   parameter called **MIDI output**. Set it to **Ch 2** (melody is on channel 2; the Layers panel shows
   each layer's channel under its name). Leave it on **All** if you just want to hear everything at once
   through one instrument for now.
6. Text too small or too big? Top right of the FLVSTX window, the **A** menu picks the size. Drag the
   bottom-right corner of the piano roll area to make the window bigger.

## Part 4 · Set the key and style

In the FLVSTX top bar:
1. Key: **C** and **major**.
2. BPM: **100** (this is the plugin's own tempo for its Play button; FL's tempo is used when *Sync to host* is on).
3. Style: type **kids** and press Tab. This picks nursery-rhyme grooves, plain triads and gentle humanization.

## Part 5 · Write the melody first and get it right

Twinkle Twinkle is a known tune, so we tell the composer exactly what we want.

1. Click **+ Section** (top bar). A section called *Verse* (8 bars) appears in the section strip.
2. In the chat box type and press Enter:
   > Write the Twinkle Twinkle Little Star melody in C major in the Verse section, 8 bars, one note per syllable, with the lyrics attached. Keep it simple and singable.
   The status shows *thinking…*, then tool chips appear (set_notes, analyze…) and the notes appear in the
   piano roll with the syllables written on them. The composer replies with a short summary.
3. Listen: press **▶ Play** in the bottom bar. It loops the Verse. (Make sure *Loop section* is ticked.)
4. Iterate until you like it. Examples:
   - "Make bar 4 end on a longer note."
   - "Move the whole melody up an octave."
   - "Add a small pickup note before bar 5."
   - Or edit by hand in the piano roll: drag notes, double-click to add, right-click to delete, ↑/↓ to transpose.
   - Or press **Suggest Melody** (bottom bar) to get three fresh takes; click *1*, *2*, *3* to compare.
5. When it is right, press **Keep** (or click **L** next to *Melody* in the Layers panel). The melody is now
   locked: nothing later will change it.

## Part 6 · Chords that fit the melody

1. In the Layers panel click **Chords** to select the layer.
2. Press **Suggest Chords** (bottom bar). Because a melody exists, the plugin *harmonizes* it: three
   progressions that fit the tune, from simple (I, IV, V) to richer (7ths, a borrowed chord).
   The chord names show in the section row (for example `| C (I) | F (IV) | C (I) | G (V) |`).
3. Click takes *1*, *2*, *3* while playing to compare. **More** gives three new ones.
   Or ask the composer: "Give me the simplest chords for this melody, one chord per bar" or
   "Make bar 6 a minor iv chord".
4. Press **Keep**.

Tip: if you prefer chords first for another song, do Part 6 before Part 5; then *Suggest Melody* writes
a melody over your chords instead.

## Part 7 · Bass, drums, and extra layers

1. Layers panel > **Bass** > **Suggest Bass**. Takes: style default, root-fifth, push. For a kids' song
   *root5* is the classic "oom-pah". Keep one.
2. **Drums** > **Suggest Drums**. Takes: as is / lighter / busier. Keep one.
3. Add colour: in the Layers panel choose a kind in the dropdown and press **+ Layer**:
   - **Arp** (arpeggio): plays the chords as broken notes; try *Suggest Arp* takes (up 1/16, up-down 1/8, chord).
   - **Perc** (percussion): shaker/conga layers.
   - **Harmony**: a second voice a third below the melody. **Counter**: an answering line in the melody's rests.
   - **Pad**: long held chords (nice under the intro and outro).
   Each new layer gets its own MIDI channel (shown under its name).

## Part 8 · Arrange it into a 2-minute song

At 100 BPM one 8-bar section is about 19 seconds, so six or seven sections make ~2 minutes.

Option A, ask the composer (recommended):
> Turn this into a full 2-minute song: Intro 4 bars, Verse, Chorus, Verse 2, Chorus, Bridge 4 bars, final Chorus, Outro 4 bars. Keep my melody and chords in the verses; make the chorus a restatement of the melody a little higher with fuller chords; intro just pad and arp; outro slows down with long notes.

The composer builds the sections, copies the locked layers, and generates the rest with continuity.

Option B, by hand:
1. Select the Verse in the section strip and press **duplicate** for each copy you need; rename them
   (Section name box) and set the **role** dropdown (intro, verse, chorus, bridge, outro). The role changes the
   defaults: intros hold chords and drop drums, choruses get fuller voicings, and so on.
2. Tick **Arrangement** in the top bar. A grid appears: layers down the side, sections across. Click a cell to
   silence a layer in that section (grey) or bring it back. Typical: intro = pad + arp only; verse 1 = no
   percussion; chorus = everything; outro = pad + melody.
3. Press **Generate song**. Every unlocked layer in every section is filled; locked layers are untouched.
4. Untick *Loop section* and press **Play** to hear the whole song. The playhead runs along the section strip.
5. Fix individual sections: select a section, select a layer, *Suggest*, compare takes, *Keep*.

## Part 9 · Get every layer into FL Studio

Two ways; use both if you like.

**Way 1, the script (exact notes into FL's piano roll):**
1. In FLVSTX press **Export song** (bottom bar). It writes `%LOCALAPPDATA%\FLVSTX\export\`: `latest.json`,
   `latest.mid`, and one `.mid` per layer (`melody.mid`, `chords.mid`, `bass.mid`, `drums.mid`, `arpeggio.mid`…).
   The chat panel confirms.
2. In FL Studio add one instrument per layer (FLEX for melody, a piano for chords, a bass synth, FPC for
   drums, another FLEX for the arp…). In the Channel Rack, click each one to open it.
3. For each instrument: open its **piano roll** (click the channel's name, then press F7 or use the
   Channel Rack piano-roll button). In the piano roll menu choose **Tools > Scripts > FLVSTX Import**.
   A small dialog appears: pick the layer in the **Track** dropdown (it lists the layer ids), leave
   *Replace existing notes* off, press **Accept**. The notes appear.
4. Repeat for every layer. The song lands as one long pattern per channel; that is fine for a first pass.
   If you prefer one pattern per section, run **Export section** for each section instead and import into a
   new pattern each time.

**Way 2, drag the files:** press **Open folder** and drag `melody.mid` from the Explorer window onto an
instrument in the Channel Rack. FL creates a channel with the notes. Dropping onto the piano roll works but
replaces what is there.

Once imported, the notes are ordinary FL notes: edit, quantize, or humanize them further in FL.

**Live monitoring without exporting:** keep FLVSTX routed to instruments through MIDI ports (Part 3).
To drive several instruments at once, add more FLVSTX instances, one per instrument, each with its
*MIDI output* parameter set to that layer's channel; each instance keeps its own session, so use export for
the final song.

## Part 10 · Save and come back later

- **File > Save** in FL Studio. FLVSTX stores the song, the chat history and the composer's session inside
  the project, so when you reopen the project the conversation continues where it left off.
- The **default / sonnet / opus** picker in the chat header changes which Claude model answers next.

## Quick reference

| Want to… | Do |
|---|---|
| Three alternatives for the selected layer | *Suggest <layer>*, then click takes 1/2/3, *More*, *Keep* |
| Stop a layer from changing | *L* next to the layer, or *Keep* |
| Silence a layer in a section | tick *Arrangement*, click the cell |
| Change a section's character | role dropdown + energy slider in the section row |
| Fill everything | *Generate section* or *Generate song* |
| Undo anything | *Undo* in the bottom bar |
| Get notes into FL | *Export section/song* then piano roll *Tools > Scripts > FLVSTX Import* |
| Hear the song inside FLVSTX | *Play* (loops the section, or the song with *Loop section* off) |
| Follow FL's transport | tick *Sync to host* |

## If something is wrong

- **Composer shows "offline"**: press *Connect*. If it stays offline, open a terminal in `D:\FLVSTX\agent`
  and run `npm run build`, then check `%LOCALAPPDATA%\FLVSTX\agent.log`. Claude Code must be logged in.
- **No sound on Play**: check the wrapper MIDI *Output port* on FLVSTX equals the *Input port* on the
  instrument, and that the *MIDI output* parameter is *All* or the right channel.
- **Script not in Tools > Scripts**: rerun `scripts\install.ps1`, then restart FL Studio.
- **Notes land in the wrong place**: the import places notes at the piano roll's timeline selection start;
  clear the selection (click in the empty ruler) before importing.
