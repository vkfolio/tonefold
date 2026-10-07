# Tonefold tutorial: a complete "Twinkle Twinkle Little Star" song, finished in FL Studio

> Tonefold is a standalone app first. This tutorial also covers the optional plugin, because it
> finishes the song in FL Studio. To stay in the app, skip Part 2 and use *Export…* instead of
> Part 9. Install the plugins with `scripts\install.ps1 -Plugins`.

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

You need: FL Studio 2024 or newer, and Node.js 20+ with Claude Code logged in on this PC (only the
chat composer needs those two — everything else works without them).

1. Unzip the release, open a terminal in the unzipped folder, and run:
   ```
   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Plugins
   ```
   Accept the administrator prompt — that is the plugins going into the folders FL Studio scans
   (without `-Plugins` you get the app alone, with no prompt).
   It prints what it installed: the CLAP and VST3 files, the standalone app, the composer and the
   piano-roll script. First run also downloads the built-in sounds (31 MB).

   Building from source instead? `cargo xtask bundle tonefold-plugin --release`, then
   `cargo build --release -p tonefold-plugin --features standalone --bin tonefold-standalone`, then
   `cd agent && npm ci && npm run build && cd ..`, then the same install command.
2. Check Claude is logged in: run `claude` in a terminal once; if it asks you to log in, do it, then exit.

You should now have:
- `C:\Program Files\Common Files\CLAP\Tonefold.clap`
- `C:\Program Files\Common Files\VST3\Tonefold.vst3`
- `Documents\Image-Line\FL Studio\Settings\Piano roll scripts\Tonefold Import.pyscript`

## Part 2 · Make FL Studio see the plugin (once)

1. Open FL Studio. Menu **Options > Manage plugins**.
2. The installer put the files in the standard folders FL already scans (`C:\Program Files\Common Files\CLAP`
   and `...\VST3`), so do not add search paths. If you added `AppData\Local\Programs\Common\...` paths
   earlier, remove them (select the path, press the minus icon): FL registers added paths as VST2 and ignores
   CLAP/VST3 files in them.
3. Tick **Rescan previously verified plugins**, then click **Find installed plugins** (top left). Wait for the scan.
4. Type **Tonefold** in the **Find** box at the bottom right. You should see Tonefold with format **CLAP** (and
   one with **VST3**). Click the star on the **CLAP** row to favourite it so it appears in the Add menu.
   Use CLAP in FL Studio; VST3 MIDI output is unreliable in FL.
5. Close Manage plugins.

## Part 3 · Set up a project (every new song)

Tonefold has its own built-in sounds (a General MIDI bank with pianos, strings, synths, drum kits), so you
can compose and listen without routing anything.

1. **File > New**. Set the project tempo to **100**.
2. **Add > Tonefold** (the CLAP version). Its window opens: chat on the left, piano roll in the middle,
   Layers on the right. Press Play later and you hear every layer through this channel.
3. Each layer has an instrument: select a layer, then use the instrument dropdown in the piano roll's
   toolbar (next to the layer name). Sensible defaults are chosen per layer and style (music box for a
   kids' melody, electric piano for chords, finger bass, drum kit for drums).
4. **Built-in sound** (bottom bar, next to Panic) is on by default, with a volume slider. Leave it on.
5. No Patcher, ports or MIDI channels are needed. (Advanced: the **Send** dropdown and the wrapper MIDI
   ports still exist if you want to drive a VST instrument live; untick Built-in sound then.)
6. Text size: the **A** menu at the top right. Resize the window by dragging its edges or the corner.

You can also run Tonefold without FL Studio: Start Menu > **Tonefold** opens the standalone app with the same
built-in sounds; export or drag layers into FL later.

## Part 4 · Set the key and style

In the Tonefold top bar:
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

**Way 1, drag and drop (recommended).** Add your real instruments in the Channel Rack (FLEX for melody,
a piano for chords, a bass, FPC for drums…). In the Tonefold Layers panel every layer has a small **⇗**
handle. Drag the handle and drop it:
- onto an instrument's button in the **Channel Rack**: FL creates the notes for that channel, or
- onto that instrument's open **piano roll**: the notes land there (this replaces what the pattern
  already had, so use an empty pattern).
The handle turns into a **✓** once written. A drag writes the whole song for that layer; hold **Shift**
while dragging to write only the selected section.

**Way 2, export + script.** Press **Export… > MIDI · whole song** (bottom bar) and pick a folder. It writes
`latest.mid` (all layers) and one `.mid` per layer there, and refreshes the same files in
`%LOCALAPPDATA%\Tonefold\export\` so the import script always sees the latest take. In the target
channel's piano roll choose **Tools > Scripts > Tonefold Import**, pick the layer, press Accept. Or drag
a `.mid` from the folder you picked (**Open folder** reopens it) onto a Channel Rack slot.

Once imported, the notes are ordinary FL notes on your instruments; untick **Built-in sound** in Tonefold
(or mute its channel) so you don't hear both.

## Part 10 · Save and come back later

- **File > Save** in FL Studio. Tonefold stores the song, the chat history and the composer's session inside
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
| Get notes into FL | *Export… > MIDI* (pick a folder) then piano roll *Tools > Scripts > Tonefold Import* |
| Send someone the song | *Export… > Audio* — a `.wav` mix plus one `.wav` per layer |
| Hear the song inside Tonefold | *Play* (loops the section, or the song with *Loop section* off) |
| Follow FL's transport | tick *Sync to host* |

## If something is wrong

- **Composer shows "offline"**: press *Connect*. If it stays offline, open a terminal in `D:\Tonefold\agent`
  and run `npm run build`, then check `%LOCALAPPDATA%\Tonefold\agent.log`. Claude Code must be logged in.
- **No sound on Play**: check the wrapper MIDI *Output port* on Tonefold equals the *Input port* on the
  instrument, and that the *MIDI output* parameter is *All* or the right channel.
- **Script not in Tools > Scripts**: rerun `scripts\install.ps1`, then restart FL Studio.
- **Notes land in the wrong place**: the import places notes at the piano roll's timeline selection start;
  clear the selection (click in the empty ruler) before importing.
