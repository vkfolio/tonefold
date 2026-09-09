//! Session operations: a JSON-in / JSON-out dispatcher used by the plugin (serving the agent's tool
//! calls over IPC) and by the CLI chat harness. Keeping it here means the agent's tools behave the
//! same everywhere and can be tested without a DAW.
//!
//! Layers are addressed by id (e.g. "melody", "arp2"), name, or kind name (first layer of that kind).
//! Sections by id or name.

use crate::analyze::analyze_clip;
use crate::generate::{chords::suggest_progressions, generate_section, generate_song, generate_track, GenParams};
use crate::humanize::{humanize, HumanizeParams};
use crate::model::{Clip, ClipSource, SectionRole, Session, TrackRole, PPQ};
use crate::notation::{format_chords, format_drums, format_melody, parse_chords, parse_drums, parse_melody};
use crate::theory::{Key, ScaleKind};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A named snapshot taken before a run, so a whole batch of changes can be undone at once.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub id: String,
    pub label: String,
    pub session: Session,
}

/// Undo-able session store with snapshots.
#[derive(Debug, Default)]
pub struct Store {
    pub session: Session,
    pub history: Vec<Session>,
    pub redo: Vec<Session>,
    /// Bumped on every mutation so UIs know to refresh.
    pub revision: u64,
    /// Named snapshots. Undo's ring holds 64 steps and a producer run makes far more than that, so
    /// "put it back the way it was" needs a mark of its own rather than a walk backwards.
    pub checkpoints: Vec<Checkpoint>,
}

impl Store {
    pub fn new(session: Session) -> Self {
        Store { session, history: Vec::new(), redo: Vec::new(), revision: 0, checkpoints: Vec::new() }
    }

    /// Marks the current session so it can be restored later. Keeps the last eight.
    pub fn checkpoint(&mut self, label: &str) -> String {
        let id = format!("cp{}", self.revision);
        self.checkpoints.retain(|c| c.id != id);
        self.checkpoints.push(Checkpoint { id: id.clone(), label: label.to_string(), session: self.session.clone() });
        if self.checkpoints.len() > 8 {
            self.checkpoints.remove(0);
        }
        id
    }

    /// Puts the session back to a checkpoint. Undoable in one step, like any other change.
    pub fn revert_to(&mut self, id: &str) -> bool {
        let Some(c) = self.checkpoints.iter().find(|c| c.id == id).cloned() else { return false };
        let backup = std::mem::replace(&mut self.session, c.session);
        self.history.push(backup);
        if self.history.len() > 64 {
            self.history.remove(0);
        }
        self.redo.clear();
        self.revision += 1;
        true
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.history.pop() {
            let cur = std::mem::replace(&mut self.session, prev);
            self.redo.push(cur);
            self.revision += 1;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            let cur = std::mem::replace(&mut self.session, next);
            self.history.push(cur);
            self.revision += 1;
            true
        } else {
            false
        }
    }

    /// Runs a mutating operation with an automatic snapshot (rolled back on error).
    pub fn mutate<T>(&mut self, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        let backup = self.session.clone();
        match f(&mut self.session) {
            Ok(v) => {
                self.history.push(backup);
                if self.history.len() > 64 {
                    self.history.remove(0);
                }
                self.redo.clear();
                self.revision += 1;
                Ok(v)
            }
            Err(e) => {
                self.session = backup;
                Err(e)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionSpec {
    pub name: String,
    pub bars: u32,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub energy: Option<f32>,
    #[serde(default)]
    pub chords: Option<String>,
    #[serde(default)]
    pub lyrics: Option<String>,
    /// Layer ids/kinds that are silent in this section.
    #[serde(default)]
    pub silent_layers: Option<Vec<String>>,
}

fn arg<'a, T: serde::de::DeserializeOwned>(params: &'a Value, name: &str) -> Result<T> {
    let v = params.get(name).ok_or_else(|| Error::Parse(format!("missing parameter '{name}'")))?;
    serde_json::from_value(v.clone()).map_err(|e| Error::Parse(format!("parameter '{name}': {e}")))
}

fn opt<T: serde::de::DeserializeOwned>(params: &Value, name: &str) -> Result<Option<T>> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|e| Error::Parse(format!("parameter '{name}': {e}"))),
    }
}

/// Resolves the `track` parameter to a layer id.
fn track_id(session: &Session, params: &Value) -> Result<String> {
    let s: String = arg(params, "track")?;
    session.track_by(&s).map(|t| t.id.clone()).ok_or_else(|| Error::UnknownTrack(format!("{s} (layers: {})", session.tracks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", "))))
}

fn section_ids(session: &Session, section: &Option<String>) -> Result<Vec<String>> {
    match section {
        Some(id) => Ok(vec![session.section(id).map(|s| s.id.clone()).ok_or_else(|| Error::UnknownSection(id.clone()))?]),
        None => Ok(session.sections.iter().map(|s| s.id.clone()).collect()),
    }
}

/// Compact, LLM-friendly description of the session.
pub fn describe(session: &Session) -> String {
    let mut s = format!(
        "Key: {} | Tempo: {:.0} BPM | Time: {}/{} | Style: {} | Sections: {} | Layers: {}\n",
        session.key.name(),
        session.tempo,
        session.time_sig.num,
        session.time_sig.den,
        session.style,
        session.sections.len(),
        session.tracks.iter().map(|t| format!("{}{}{} [{}]", t.id, if t.kind.name() != t.id { format!("({})", t.kind.name()) } else { String::new() }, if t.locked { "[locked]" } else { "" }, crate::gm::program_name(t.program(&session.style)))).collect::<Vec<_>>().join(", ")
    );
    let bar = session.bar_ticks();
    for sec in &session.sections {
        s.push_str(&format!("\n## {} (id={}, role={}, {} bars, energy {:.2})\n", sec.name, sec.id, sec.role.name(), sec.bars, sec.energy));
        if sec.chords.is_empty() {
            s.push_str("chords: (none)\n");
        } else {
            s.push_str(&format!("chords: {}\n", format_chords(&sec.chords, &session.key, sec.bars, bar)));
        }
        if let Some(l) = &sec.lyrics {
            s.push_str(&format!("lyrics: {}\n", l.replace('\n', " / ")));
        }
        for t in &session.tracks {
            if !t.active_in(&sec.id) {
                s.push_str(&format!("{}: silent in this section\n", t.id));
                continue;
            }
            match t.clips.get(&sec.id) {
                Some(c) if !c.notes.is_empty() => {
                    let src = match &c.source {
                        ClipSource::Generated { .. } => "generated",
                        ClipSource::Agent => "agent",
                        ClipSource::Edited => "edited by user",
                        ClipSource::Imported => "imported",
                    };
                    s.push_str(&format!("{}: {} notes ({}{})\n", t.id, c.notes.len(), src, if t.muted { ", muted" } else { "" }));
                }
                _ => s.push_str(&format!("{}: (empty)\n", t.id)),
            }
        }
    }
    s
}

fn clip_report(session: &Session, track: &str, section: &str, notes: &[crate::model::Note]) -> (String, String) {
    let sec = session.section(section).cloned().unwrap();
    let t = session.track_by(track).unwrap();
    let notation = if t.kind.is_pitched() { format_melody(notes) } else { format_drums(notes, sec.bars, session.bar_ticks()) };
    let mut a = analyze_clip(session, t.kind, &sec, notes);
    a.track = t.id.clone();
    (a.summary(), notation)
}

/// Dispatches one operation. Mutating operations snapshot for undo.
pub fn dispatch(store: &mut Store, method: &str, params: &Value) -> Result<Value> {
    match method {
        "get_session" => {
            let detail: Option<String> = opt(params, "detail")?;
            let mut out = json!({ "summary": describe(&store.session), "revision": store.revision });
            if detail.as_deref() == Some("full") {
                out["session"] = serde_json::to_value(&store.session).unwrap_or_default();
            }
            Ok(out)
        }
        "get_notes" => {
            let tid = track_id(&store.session, params)?;
            let section: String = arg(params, "section")?;
            let sec = store.session.section(&section).cloned().ok_or_else(|| Error::UnknownSection(section.clone()))?;
            let notes = store.session.clip(&tid, &sec.id).map(|c| c.notes.clone()).unwrap_or_default();
            let (analysis, notation) = clip_report(&store.session, &tid, &sec.id, &notes);
            Ok(json!({ "track": tid, "section": sec.id, "notation": notation, "note_count": notes.len(), "analysis": analysis }))
        }
        "set_key_tempo" => {
            let key: Option<String> = opt(params, "key")?;
            let tempo: Option<f32> = opt(params, "tempo")?;
            let style: Option<String> = opt(params, "style")?;
            let ts: Option<String> = opt(params, "time_signature")?;
            store.mutate(|s| {
                if let Some(k) = key {
                    s.key = Key::parse(&k).ok_or_else(|| Error::Parse(format!("cannot parse key '{k}' (try 'C major', 'A minor', 'F# dorian')")))?;
                }
                if let Some(t) = tempo {
                    if !(30.0..=300.0).contains(&t) {
                        return Err(Error::Parse("tempo must be 30..300".into()));
                    }
                    s.tempo = t;
                }
                if let Some(st) = style {
                    s.style = st;
                }
                if let Some(ts) = ts {
                    let (n, d) = ts.split_once('/').ok_or_else(|| Error::Parse("time_signature like '4/4'".into()))?;
                    s.time_sig = crate::model::TimeSig { num: n.trim().parse().map_err(|_| Error::Parse("bad numerator".into()))?, den: d.trim().parse().map_err(|_| Error::Parse("bad denominator".into()))? };
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true, "summary": describe(&store.session) }))
        }
        "set_form" => {
            let specs: Vec<SectionSpec> = arg(params, "sections")?;
            let keep: Option<bool> = opt(params, "keep_existing")?;
            store.mutate(|s| {
                let old = std::mem::take(&mut s.sections);
                let old_clips: Vec<(String, std::collections::BTreeMap<String, Clip>)> = s.tracks.iter_mut().map(|t| (t.id.clone(), std::mem::take(&mut t.clips))).collect();
                for t in s.tracks.iter_mut() {
                    t.inactive.clear();
                }
                let mut ids = Vec::new();
                for spec in specs {
                    if spec.bars == 0 || spec.bars > 128 {
                        return Err(Error::Parse(format!("section '{}': bars must be 1..128", spec.name)));
                    }
                    let role = match &spec.role {
                        Some(r) => SectionRole::parse(r).ok_or_else(|| Error::Parse(format!("unknown section role '{r}' (intro, verse, pre_chorus, chorus, bridge, break, build, drop, outro)")))?,
                        None => SectionRole::from_name(&spec.name),
                    };
                    let energy = spec.energy.unwrap_or_else(|| role.default_energy());
                    let id = s.add_section(&spec.name, spec.bars, energy);
                    s.section_mut(&id).unwrap().role = role;
                    for t in s.tracks.iter_mut() {
                        if role.default_layers_active(t.kind) {
                            t.inactive.remove(&id);
                        } else {
                            t.inactive.insert(id.clone());
                        }
                    }
                    let bar = s.bar_ticks();
                    let key = s.key;
                    if keep.unwrap_or(true) {
                        if let Some(o) = old.iter().find(|o| o.id == id) {
                            if spec.chords.is_none() {
                                s.section_mut(&id).unwrap().chords = o.chords.iter().filter(|c| c.start < spec.bars * bar).cloned().collect();
                            }
                            for (tid, clips) in &old_clips {
                                if let Some(c) = clips.get(&id) {
                                    if let Some(t) = s.track_by_mut(tid) {
                                        t.clips.insert(id.clone(), c.clone());
                                    }
                                }
                            }
                        }
                    }
                    if let Some(ch) = &spec.chords {
                        s.section_mut(&id).unwrap().chords = parse_chords(ch, &key, spec.bars, bar)?;
                    }
                    if let Some(l) = spec.lyrics {
                        s.section_mut(&id).unwrap().lyrics = Some(l);
                    }
                    if let Some(silent) = &spec.silent_layers {
                        for t in s.tracks.iter_mut() {
                            t.inactive.remove(&id);
                        }
                        for l in silent {
                            let tid = s.track_by(l).map(|t| t.id.clone());
                            if let Some(tid) = tid {
                                s.track_by_mut(&tid).unwrap().inactive.insert(id.clone());
                            }
                        }
                    }
                    ids.push(id);
                }
                Ok(ids)
            })
            .map(|ids| json!({ "ok": true, "section_ids": ids, "summary": describe(&store.session) }))
        }
        "add_layer" => {
            let kind_s: String = arg(params, "kind")?;
            let name: Option<String> = opt(params, "name")?;
            let kind = TrackRole::parse(&kind_s).ok_or_else(|| Error::Parse(format!("unknown layer kind '{kind_s}' (kinds: {})", TrackRole::ALL.iter().map(|k| k.name()).collect::<Vec<_>>().join(", "))))?;
            let id = store.mutate(|s| Ok(s.add_track(kind, name.as_deref())))?;
            let t = store.session.track_by(&id).unwrap();
            Ok(json!({ "ok": true, "track": id, "kind": kind.name(), "channel": t.channel + 1, "summary": describe(&store.session) }))
        }
        "remove_layer" => {
            let tid = track_id(&store.session, params)?;
            store.mutate(|s| {
                if s.tracks.len() <= 1 {
                    return Err(Error::Parse("cannot remove the last layer".into()));
                }
                s.remove_track(&tid);
                Ok(())
            })?;
            Ok(json!({ "ok": true, "summary": describe(&store.session) }))
        }
        "rename_layer" => {
            let tid = track_id(&store.session, params)?;
            let name: String = arg(params, "name")?;
            store.mutate(|s| {
                s.track_by_mut(&tid).unwrap().name = name;
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "set_instrument" => {
            let tid = track_id(&store.session, params)?;
            let inst: String = arg(params, "instrument")?;
            let program = crate::gm::parse_program(&inst).ok_or_else(|| Error::Parse(format!("unknown instrument '{inst}' (General MIDI name or number 0-127, or 'drum kit')")))?;
            store.mutate(|s| {
                s.track_by_mut(&tid).unwrap().instrument = Some(program);
                Ok(())
            })?;
            Ok(json!({ "ok": true, "track": tid, "instrument": crate::gm::program_name(program) }))
        }
        "set_groove" => {
            let groove: String = arg(params, "groove")?;
            let known = ["straight_pop", "swing_16", "boom_bap", "house", "jazz_swing", "tresillo", "none"];
            if !known.contains(&groove.as_str()) {
                return Err(Error::Parse(format!("unknown groove '{groove}' (one of {})", known.join(", "))));
            }
            let track: Option<String> = opt(params, "track")?;
            let tid = match &track {
                Some(_) => Some(track_id(&store.session, params)?),
                None => None,
            };
            store.mutate(|s| {
                match &tid {
                    Some(id) => s.track_by_mut(id).unwrap().groove = Some(groove.clone()),
                    None => s.groove = Some(groove.clone()),
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true, "groove": groove, "track": tid, "hint": "regenerate or humanize the affected layers to hear it" }))
        }
        "list_instruments" => Ok(json!({ "instruments": crate::gm::GM_PROGRAMS.iter().enumerate().map(|(i, n)| format!("{i}: {n}")).collect::<Vec<_>>(), "drums": "128: Drum Kit" })),
        "list_layer_kinds" => Ok(json!({ "kinds": TrackRole::ALL.iter().map(|k| json!({ "kind": k.name(), "description": k.description() })).collect::<Vec<_>>() })),
        "set_arrangement" => {
            let tid = track_id(&store.session, params)?;
            let section: Option<String> = opt(params, "section")?;
            let active: bool = arg(params, "active")?;
            let ids = section_ids(&store.session, &section)?;
            store.mutate(|s| {
                let t = s.track_by_mut(&tid).unwrap();
                for id in ids {
                    if active {
                        t.inactive.remove(&id);
                    } else {
                        t.inactive.insert(id);
                    }
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true, "summary": describe(&store.session) }))
        }
        "set_chords" => {
            let section: String = arg(params, "section")?;
            let notation: String = arg(params, "notation")?;
            let bar = store.session.bar_ticks();
            let key = store.session.key;
            let out = store.mutate(|s| {
                let sec = s.section_mut(&section).ok_or_else(|| Error::UnknownSection(section.clone()))?;
                let ev = parse_chords(&notation, &key, sec.bars, bar)?;
                let rendered = format_chords(&ev, &key, sec.bars, bar);
                sec.chords = ev;
                Ok(rendered)
            })?;
            Ok(json!({ "ok": true, "chords": out }))
        }
        "harmonize" => {
            let section: String = arg(params, "section")?;
            let complexity: Option<String> = opt(params, "complexity")?;
            let count: Option<usize> = opt(params, "count")?;
            let apply: Option<usize> = opt(params, "apply")?;
            let sess = store.session.clone();
            let sec = sess.section(&section).cloned().ok_or_else(|| Error::UnknownSection(section.clone()))?;
            let melody = sess.notes_of_kind(TrackRole::Melody, &sec.id);
            if melody.is_empty() {
                return Err(Error::Parse(format!("section '{}' has no melody to harmonize; write or generate a melody first (or use suggest_chords)", sec.id)));
            }
            let cx = complexity.as_deref().map(crate::generate::harmonize::Complexity::parse);
            let results = crate::generate::harmonize::harmonize(&sess.key, &melody, sec.bars, sess.bar_ticks(), cx, count.unwrap_or(3).clamp(1, 6));
            if results.is_empty() {
                return Err(Error::Parse("could not harmonize".into()));
            }
            let listing: Vec<Value> = results
                .iter()
                .enumerate()
                .map(|(i, h)| json!({ "index": i, "label": h.label, "fit": format!("{:.2}", h.score), "chords": format_chords(&h.events, &sess.key, sec.bars, sess.bar_ticks()) }))
                .collect();
            if let Some(idx) = apply {
                let h = results.get(idx).ok_or_else(|| Error::Parse(format!("apply index {idx} out of range")))?;
                let events = h.events.clone();
                store.mutate(|s| {
                    s.section_mut(&sec.id).unwrap().chords = events;
                    Ok(())
                })?;
            }
            Ok(json!({ "candidates": listing, "applied": apply, "hint": "call again with apply=<index> to set one, then generate chords/bass/drums" }))
        }
        "suggest_chords" => {
            let style: Option<String> = opt(params, "style")?;
            let count: Option<usize> = opt(params, "count")?;
            let seed: Option<u64> = opt(params, "seed")?;
            let list = suggest_progressions(&store.session, style.as_deref(), count.unwrap_or(4), seed.unwrap_or(store.session.seed + store.revision));
            Ok(json!({ "progressions": list.iter().map(|(sym, rom)| json!({ "chords": sym, "roman": rom })).collect::<Vec<_>>() }))
        }
        "set_notes" => {
            let tid = track_id(&store.session, params)?;
            let section: String = arg(params, "section")?;
            let notation: String = arg(params, "notation")?;
            let do_humanize: Option<bool> = opt(params, "humanize")?;
            let from_bar: Option<u32> = opt(params, "from_bar")?;
            let to_bar: Option<u32> = opt(params, "to_bar")?;
            let sess = store.session.clone();
            let sec = sess.section(&section).cloned().ok_or_else(|| Error::UnknownSection(section.clone()))?;
            let kind = sess.track_by(&tid).unwrap().kind;
            let mut notes = if kind.is_pitched() { parse_melody(&notation, 0.8)? } else { parse_drums(&notation)? };
            // A nudge is written in ms; only here do we know the tempo it is against.
            for n in notes.iter_mut() {
                if let Some(ms) = n.nudge_ms.take() {
                    let ticks = ms * (sess.tempo * PPQ as f32) / 60_000.0;
                    n.start = (n.start as f32 + ticks).max(0.0).round() as u32;
                }
            }
            let total = sec.bars * sess.bar_ticks();
            let overflow = notes.iter().filter(|n| n.start >= total).count();
            notes.retain(|n| n.start < total);
            for n in notes.iter_mut() {
                n.len = n.len.min(total - n.start).max(1);
            }
            if do_humanize.unwrap_or(true) {
                let mut hp = HumanizeParams::preset(kind.humanize_base(), &sess.style);
                hp.seed = sess.seed;
                humanize(&mut notes, &hp, kind.humanize_base(), sess.tempo, sess.bar_ticks());
            }
            // A bar range rewrites just those bars, so fixing bar 3 does not mean resending the
            // whole clip (and losing its humanization).
            if let Some(from) = from_bar {
                let bar = sess.bar_ticks();
                let start = (from.saturating_sub(1)) * bar;
                let end = (to_bar.unwrap_or(from) * bar).min(total);
                if start >= end {
                    return Err(Error::Parse(format!("from_bar {from} is not before to_bar {}", to_bar.unwrap_or(from))));
                }
                let mut merged: Vec<crate::Note> = sess.clip(&tid, &sec.id).map(|c| c.notes.clone()).unwrap_or_default();
                merged.retain(|n| n.start < start || n.start >= end);
                for n in notes.iter_mut() {
                    n.start += start;
                }
                notes.retain(|n| n.start < end);
                merged.extend(notes.iter().cloned());
                merged.sort_by_key(|n| (n.start, n.pitch));
                notes = merged;
            }
            let (analysis, _) = clip_report(&sess, &tid, &sec.id, &notes);
            store.mutate(|s| {
                if s.track_by(&tid).unwrap().locked {
                    return Err(Error::Parse(format!("layer '{tid}' is locked")));
                }
                s.set_clip(&tid, &sec.id, Clip::new(notes.clone(), ClipSource::Agent))
            })?;
            let mut msg = analysis;
            if overflow > 0 {
                msg.push_str(&format!("\n  ! {overflow} note(s) beyond the end of the {}-bar section were dropped", sec.bars));
            }
            Ok(json!({ "ok": true, "analysis": msg }))
        }
        "generate" => {
            let tid = track_id(&store.session, params)?;
            let section: String = arg(params, "section")?;
            let gp: GenParams = serde_json::from_value(params.get("params").cloned().unwrap_or(json!({}))).map_err(|e| Error::Parse(format!("params: {e}")))?;
            let clip = store.mutate(|s| generate_track(s, &tid, &section, &gp))?;
            let sec_id = store.session.section(&section).unwrap().id.clone();
            let (analysis, notation) = clip_report(&store.session, &tid, &sec_id, &clip.notes);
            Ok(json!({ "ok": true, "analysis": analysis, "notation": notation }))
        }
        "generate_all" => {
            let section: String = arg(params, "section")?;
            let gp: GenParams = serde_json::from_value(params.get("params").cloned().unwrap_or(json!({}))).map_err(|e| Error::Parse(format!("params: {e}")))?;
            let done = store.mutate(|s| generate_section(s, &section, &gp))?;
            let sec_id = store.session.section(&section).unwrap().id.clone();
            let out: Vec<String> = done.iter().map(|(tid, clip)| clip_report(&store.session, tid, &sec_id, &clip.notes).0).collect();
            Ok(json!({ "ok": true, "analysis": out.join("\n") }))
        }
        "generate_song" => {
            let gp: GenParams = serde_json::from_value(params.get("params").cloned().unwrap_or(json!({}))).map_err(|e| Error::Parse(format!("params: {e}")))?;
            let done = store.mutate(|s| generate_song(s, &gp))?;
            Ok(json!({ "ok": true, "generated": done.len(), "summary": describe(&store.session) }))
        }
        "humanize" => {
            let tid = track_id(&store.session, params)?;
            let section: Option<String> = opt(params, "section")?;
            let kind = store.session.track_by(&tid).unwrap().kind.humanize_base();
            let mut hp = HumanizeParams::preset(kind, &store.session.style);
            if let Some(Value::Object(map)) = params.get("params") {
                let mut base = serde_json::to_value(&hp).unwrap();
                for (k, v) in map {
                    base[k] = v.clone();
                }
                hp = serde_json::from_value(base).map_err(|e| Error::Parse(format!("params: {e}")))?;
            }
            let ids = section_ids(&store.session, &section)?;
            let (tempo, bar) = (store.session.tempo, store.session.bar_ticks());
            store.mutate(|s| {
                let t = s.track_by_mut(&tid).unwrap();
                for id in ids {
                    if let Some(clip) = t.clips.get_mut(&id) {
                        humanize(&mut clip.notes, &hp, kind, tempo, bar);
                        clip.sort();
                    }
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true, "params": hp }))
        }
        "analyze" => {
            let track: Option<String> = opt(params, "track")?;
            let section: Option<String> = opt(params, "section")?;
            let tids: Vec<String> = match track {
                Some(t) => vec![store.session.track_by(&t).map(|x| x.id.clone()).ok_or_else(|| Error::UnknownTrack(t))?],
                None => store.session.tracks.iter().map(|t| t.id.clone()).collect(),
            };
            let mut lines = Vec::new();
            for sid in section_ids(&store.session, &section)? {
                for tid in &tids {
                    let t = store.session.track_by(tid).unwrap();
                    if !t.active_in(&sid) {
                        continue;
                    }
                    let notes = store.session.clip(tid, &sid).map(|c| c.notes.clone()).unwrap_or_default();
                    lines.push(clip_report(&store.session, tid, &sid, &notes).0);
                }
            }
            Ok(json!({ "analysis": lines.join("\n") }))
        }
        "set_lyrics" => {
            let section: String = arg(params, "section")?;
            let lyrics: String = arg(params, "lyrics")?;
            store.mutate(|s| {
                s.section_mut(&section).ok_or_else(|| Error::UnknownSection(section.clone()))?.lyrics = Some(lyrics);
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "set_section" => {
            let section: String = arg(params, "section")?;
            let name: Option<String> = opt(params, "name")?;
            let bars: Option<u32> = opt(params, "bars")?;
            let energy: Option<f32> = opt(params, "energy")?;
            let role: Option<String> = opt(params, "role")?;
            store.mutate(|s| {
                let sec = s.section_mut(&section).ok_or_else(|| Error::UnknownSection(section.clone()))?;
                if let Some(n) = name {
                    sec.name = n;
                }
                if let Some(b) = bars {
                    sec.bars = b.clamp(1, 128);
                }
                if let Some(e) = energy {
                    sec.energy = e.clamp(0.0, 1.0);
                }
                if let Some(r) = role {
                    sec.role = SectionRole::parse(&r).ok_or_else(|| Error::Parse(format!("unknown section role '{r}'")))?;
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true, "summary": describe(&store.session) }))
        }
        "vary" => {
            // Regenerate a section as a variation of itself: same harmony and (for melody) the same
            // motif, a different performance. What `copy_section` should be followed by.
            let section: String = arg(params, "section")?;
            let track: Option<String> = opt(params, "track")?;
            let amount: f32 = opt::<f64>(params, "amount")?.unwrap_or(0.5) as f32;
            let sec_id = store.session.section(&section).map(|s| s.id.clone()).ok_or_else(|| Error::UnknownSection(section.clone()))?;
            let base_seed: u64 = opt(params, "seed")?.unwrap_or_else(|| store.session.seed.wrapping_add((amount * 1000.0) as u64).wrapping_mul(2_654_435_761));
            let ids: Vec<String> = match &track {
                Some(t) => vec![track_id(&store.session, params).map_err(|_| Error::UnknownTrack(t.clone()))?],
                None => store.session.tracks.iter().filter(|t| !t.locked && t.active_in(&sec_id) && t.clips.contains_key(&sec_id)).map(|t| t.id.clone()).collect(),
            };
            let mut done = Vec::new();
            for id in ids {
                let mut gp = GenParams { seed: Some(base_seed.wrapping_add(crate::generate::hash_str(&id))), ..Default::default() };
                // Keep the melody recognisable: develop the motif this section already has.
                if store.session.track_by(&id).map(|t| t.kind) == Some(TrackRole::Melody) {
                    gp.from_section = Some(sec_id.clone());
                }
                let sec = sec_id.clone();
                let id2 = id.clone();
                if store.mutate(|s| generate_track(s, &id2, &sec, &gp).map(|_| ())).is_ok() {
                    done.push(id);
                }
            }
            Ok(json!({ "ok": true, "section": sec_id, "varied": done, "summary": describe(&store.session) }))
        }
        "copy_section" => {
            let from: String = arg(params, "from")?;
            let to: String = arg(params, "to")?;
            store.mutate(|s| {
                let src = s.section(&from).cloned().ok_or_else(|| Error::UnknownSection(from.clone()))?;
                let dst_id = s.section(&to).map(|x| x.id.clone()).ok_or_else(|| Error::UnknownSection(to.clone()))?;
                s.section_mut(&dst_id).unwrap().chords = src.chords.clone();
                for t in s.tracks.iter_mut() {
                    if let Some(c) = t.clips.get(&src.id).cloned() {
                        t.clips.insert(dst_id.clone(), c);
                    }
                    if t.inactive.contains(&src.id) {
                        t.inactive.insert(dst_id.clone());
                    } else {
                        t.inactive.remove(&dst_id);
                    }
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "lock" => {
            let tid = track_id(&store.session, params)?;
            let locked: bool = arg(params, "locked")?;
            store.mutate(|s| {
                s.track_by_mut(&tid).unwrap().locked = locked;
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "clear" => {
            let track: Option<String> = opt(params, "track")?;
            let section: Option<String> = opt(params, "section")?;
            let tids: Vec<String> = match track {
                Some(t) => vec![store.session.track_by(&t).map(|x| x.id.clone()).ok_or_else(|| Error::UnknownTrack(t))?],
                None => store.session.tracks.iter().map(|t| t.id.clone()).collect(),
            };
            let ids = section_ids(&store.session, &section)?;
            store.mutate(|s| {
                for tid in &tids {
                    let t = s.track_by_mut(tid).unwrap();
                    for id in &ids {
                        t.clips.remove(id);
                    }
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "transpose" => {
            let track: Option<String> = opt(params, "track")?;
            let section: Option<String> = opt(params, "section")?;
            let semitones: i32 = arg(params, "semitones")?;
            let tids: Vec<String> = match track {
                Some(t) => vec![store.session.track_by(&t).map(|x| x.id.clone()).ok_or_else(|| Error::UnknownTrack(t))?],
                None => store.session.tracks.iter().filter(|t| t.kind.is_pitched()).map(|t| t.id.clone()).collect(),
            };
            let ids = section_ids(&store.session, &section)?;
            store.mutate(|s| {
                for tid in &tids {
                    let t = s.track_by_mut(tid).unwrap();
                    for id in &ids {
                        if let Some(c) = t.clips.get_mut(id) {
                            for n in c.notes.iter_mut() {
                                n.pitch = (n.pitch as i32 + semitones).clamp(0, 127) as u8;
                            }
                        }
                    }
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "undo" => Ok(json!({ "ok": store.undo(), "summary": describe(&store.session) })),
        "redo" => Ok(json!({ "ok": store.redo(), "summary": describe(&store.session) })),
        "export" => {
            let section: Option<String> = opt(params, "section")?;
            let dir: Option<String> = opt(params, "dir")?;
            let dir = dir.map(std::path::PathBuf::from).unwrap_or_else(crate::midi::default_export_dir);
            let files = crate::midi::export_to_dir(&store.session, section.as_deref(), &dir)?;
            Ok(json!({ "ok": true, "files": files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "hint": "In FL Studio: open the target piano roll, then Tools > Scripts > FLVSTX Import (or drag a .mid from the folder onto a Channel Rack slot)." }))
        }
        "list_scales" => Ok(json!({ "scales": ScaleKind::ALL.iter().map(|s| s.name()).collect::<Vec<_>>() })),
        "checkpoint" => {
            let label: Option<String> = opt(params, "label")?;
            let id = store.checkpoint(label.as_deref().unwrap_or("checkpoint"));
            Ok(json!({ "ok": true, "checkpoint": id }))
        }
        "revert_to_checkpoint" => {
            let id: String = arg(params, "checkpoint")?;
            if !store.revert_to(&id) {
                return Err(Error::Parse(format!("no checkpoint '{id}' (have: {})", store.checkpoints.iter().map(|c| c.id.as_str()).collect::<Vec<_>>().join(", "))));
            }
            Ok(json!({ "ok": true, "summary": describe(&store.session) }))
        }
        _ => Err(Error::Parse(format!("unknown method '{method}'"))),
    }
}

/// Convenience for tests and the CLI: build a whole demo song.
pub fn demo_session(style: &str, key: &str, bars: u32, seed: u64) -> Result<Session> {
    let mut store = Store::new(Session::default());
    dispatch(&mut store, "set_key_tempo", &json!({ "key": key, "style": style, "tempo": match style { s if s.contains("trap") => 140.0, s if s.contains("lofi") => 82.0, s if s.contains("house") => 124.0, s if s.contains("kid") => 104.0, s if s.contains("cine") => 90.0, _ => 110.0 } }))?;
    store.session.seed = seed;
    dispatch(&mut store, "set_form", &json!({ "sections": [ { "name": "Verse", "bars": bars, "energy": 0.5 }, { "name": "Chorus", "bars": bars, "energy": 0.85 } ] }))?;
    let sug = suggest_progressions(&store.session, Some(style), 2, seed);
    dispatch(&mut store, "set_chords", &json!({ "section": "verse", "notation": sug[0].0 }))?;
    dispatch(&mut store, "set_chords", &json!({ "section": "chorus", "notation": sug[1].0 }))?;
    for sec in ["verse", "chorus"] {
        dispatch(&mut store, "generate_all", &json!({ "section": sec, "params": { "seed": seed } }))?;
    }
    Ok(store.session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_flow() {
        let mut store = Store::new(Session::default());
        let r = dispatch(&mut store, "set_form", &json!({ "sections": [ { "name": "Verse", "bars": 8, "chords": "| C | Am | F | G |" }, { "name": "Chorus", "bars": 8 } ] })).unwrap();
        assert_eq!(r["section_ids"].as_array().unwrap().len(), 2);
        dispatch(&mut store, "set_chords", &json!({ "section": "chorus", "notation": "| F | G | C | Am |" })).unwrap();
        let r = dispatch(&mut store, "generate_all", &json!({ "section": "verse" })).unwrap();
        assert!(r["analysis"].as_str().unwrap().contains("melody"));
        let r = dispatch(&mut store, "set_notes", &json!({ "track": "melody", "section": "chorus", "notation": "C5:4 D5:4 E5:2 | G5:4 E5:4 C5:2" })).unwrap();
        assert!(r["ok"].as_bool().unwrap());
        let n = dispatch(&mut store, "get_notes", &json!({ "track": "melody", "section": "chorus" })).unwrap();
        assert_eq!(n["note_count"], 6);
        assert!(dispatch(&mut store, "set_notes", &json!({ "track": "melody", "section": "nope", "notation": "C4:4" })).is_err());
        assert!(store.undo());
        let n = dispatch(&mut store, "get_notes", &json!({ "track": "melody", "section": "chorus" })).unwrap();
        assert_eq!(n["note_count"], 0);
        assert!(store.redo());
        let d = dispatch(&mut store, "get_session", &json!({})).unwrap();
        assert!(d["summary"].as_str().unwrap().contains("Chorus"));
        let dir = std::env::temp_dir().join("flvstx-test-export");
        let e = dispatch(&mut store, "export", &json!({ "dir": dir.display().to_string() })).unwrap();
        assert!(e["files"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn layers_and_arrangement() {
        let mut store = Store::new(Session::default());
        dispatch(&mut store, "set_form", &json!({ "sections": [ { "name": "Intro", "bars": 4 }, { "name": "Verse", "bars": 8, "chords": "| C | Am | F | G |" }, { "name": "Chorus", "bars": 8, "chords": "| F | G | C | Am |" }, { "name": "Build", "bars": 4, "chords": "| G |" }, { "name": "Drop", "bars": 8, "chords": "| F | G | C | Am |" } ] })).unwrap();
        for kind in ["arpeggio", "pad", "pluck", "counter_melody", "harmony", "sub", "percussion"] {
            let r = dispatch(&mut store, "add_layer", &json!({ "kind": kind })).unwrap();
            assert!(r["ok"].as_bool().unwrap(), "{kind}");
        }
        assert_eq!(store.session.tracks.len(), 11);
        assert!(!store.session.track_by("drums").unwrap().active_in("intro"));
        let r = dispatch(&mut store, "generate_song", &json!({ "params": { "seed": 5 } })).unwrap();
        assert!(r["generated"].as_u64().unwrap() > 20);
        for t in &store.session.tracks {
            assert!(!store.session.flatten(&t.id).is_empty(), "{} produced nothing", t.id);
        }
        let v = store.session.clip("melody", "verse").unwrap().notes.clone();
        let c = store.session.clip("melody", "chorus").unwrap().notes.clone();
        assert!(!v.is_empty() && !c.is_empty());
        let d = store.session.clip("drums", "build").unwrap().notes.clone();
        let bar = store.session.bar_ticks();
        assert!(d.iter().filter(|n| n.pitch == 38 && n.start >= bar * 2).count() >= 8, "build should end with a snare roll");
        dispatch(&mut store, "set_arrangement", &json!({ "track": "arpeggio", "section": "verse", "active": false })).unwrap();
        assert!(store.session.flatten("arpeggio").iter().all(|n| n.start < bar * 4 || n.start >= bar * 12));
        dispatch(&mut store, "remove_layer", &json!({ "track": "pluck" })).unwrap();
        assert_eq!(store.session.tracks.len(), 10);
    }

    #[test]
    fn demo_songs_for_all_styles() {
        for style in ["pop", "lofi", "trap", "house", "cinematic", "kids"] {
            let s = demo_session(style, "C major", 8, 3).unwrap();
            for t in &s.tracks {
                assert!(!s.flatten(&t.id).is_empty(), "{style} {}", t.id);
            }
        }
    }

    /// A producer run makes far more changes than undo's 64-deep ring, so "put it back" needs a
    /// mark taken before the run rather than a walk backwards through it.
    #[test]
    fn a_checkpoint_survives_more_changes_than_undo_can_hold() {
        let mut store = Store::new(demo_session("pop", "C major", 8, 2).unwrap());
        let before = store.session.tempo;
        let id = dispatch(&mut store, "checkpoint", &json!({ "label": "before the run" })).unwrap();
        let id = id["checkpoint"].as_str().unwrap().to_string();

        for tempo in 0..100 {
            dispatch(&mut store, "set_key_tempo", &json!({ "tempo": 90.0 + tempo as f64 })).unwrap();
        }
        assert_ne!(store.session.tempo, before);
        assert!(store.history.len() <= 64, "undo cannot reach back that far");

        dispatch(&mut store, "revert_to_checkpoint", &json!({ "checkpoint": id })).unwrap();
        assert_eq!(store.session.tempo, before);
        // ...and the revert is itself one ordinary step, so a mis-click is recoverable.
        assert!(store.undo());
        assert_ne!(store.session.tempo, before);

        assert!(dispatch(&mut store, "revert_to_checkpoint", &json!({ "checkpoint": "nope" })).is_err());
    }
}
