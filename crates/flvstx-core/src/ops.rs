//! Session operations: a JSON-in / JSON-out dispatcher used by the plugin (serving the agent's tool
//! calls over IPC) and by the CLI chat harness. Keeping it here means the agent's tools behave the
//! same everywhere and can be tested without a DAW.

use crate::analyze::analyze_clip;
use crate::generate::{chords::suggest_progressions, generate_track, GenParams};
use crate::humanize::{humanize, HumanizeParams};
use crate::model::{Clip, ClipSource, Session, TrackRole};
use crate::notation::{format_chords, format_drums, format_melody, parse_chords, parse_drums, parse_melody};
use crate::theory::{Key, ScaleKind};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Undo-able session store with snapshots.
#[derive(Debug, Default)]
pub struct Store {
    pub session: Session,
    pub history: Vec<Session>,
    pub redo: Vec<Session>,
    /// Bumped on every mutation so UIs know to refresh.
    pub revision: u64,
}

impl Store {
    pub fn new(session: Session) -> Self {
        Store { session, history: Vec::new(), redo: Vec::new(), revision: 0 }
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
    pub energy: Option<f32>,
    #[serde(default)]
    pub chords: Option<String>,
    #[serde(default)]
    pub lyrics: Option<String>,
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

fn role_arg(params: &Value) -> Result<TrackRole> {
    let s: String = arg(params, "track")?;
    TrackRole::parse(&s).ok_or_else(|| Error::UnknownTrack(s))
}

/// Compact, LLM-friendly description of the session.
pub fn describe(session: &Session) -> String {
    let mut s = format!(
        "Key: {} | Tempo: {:.0} BPM | Time: {}/{} | Style: {} | Sections: {}\n",
        session.key.name(),
        session.tempo,
        session.time_sig.num,
        session.time_sig.den,
        session.style,
        session.sections.len()
    );
    let bar = session.bar_ticks();
    for sec in &session.sections {
        s.push_str(&format!("\n## {} (id={}, {} bars, energy {:.2})\n", sec.name, sec.id, sec.bars, sec.energy));
        if sec.chords.is_empty() {
            s.push_str("chords: (none)\n");
        } else {
            s.push_str(&format!("chords: {}\n", format_chords(&sec.chords, &session.key, sec.bars, bar)));
        }
        if let Some(l) = &sec.lyrics {
            s.push_str(&format!("lyrics: {}\n", l.replace('\n', " / ")));
        }
        for t in &session.tracks {
            match t.clips.get(&sec.id) {
                Some(c) if !c.notes.is_empty() => {
                    let src = match &c.source {
                        ClipSource::Generated { .. } => "generated",
                        ClipSource::Agent => "agent",
                        ClipSource::Edited => "edited by user",
                        ClipSource::Imported => "imported",
                    };
                    s.push_str(&format!("{}{}: {} notes ({}{})\n", t.role.name(), if t.locked { " [locked]" } else { "" }, c.notes.len(), src, if t.muted { ", muted" } else { "" }));
                }
                _ => s.push_str(&format!("{}: (empty)\n", t.role.name())),
            }
        }
    }
    s
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
            let role = role_arg(params)?;
            let section: String = arg(params, "section")?;
            let sec = store.session.section(&section).cloned().ok_or_else(|| Error::UnknownSection(section.clone()))?;
            let notes = store.session.clip(role, &sec.id).map(|c| c.notes.clone()).unwrap_or_default();
            let notation = if role == TrackRole::Drums { format_drums(&notes, sec.bars, store.session.bar_ticks()) } else { format_melody(&notes) };
            Ok(json!({ "track": role.name(), "section": sec.id, "notation": notation, "note_count": notes.len(),
                "analysis": analyze_clip(&store.session, role, &sec, &notes).summary() }))
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
                let old_clips: Vec<(TrackRole, std::collections::BTreeMap<String, Clip>)> = s.tracks.iter_mut().map(|t| (t.role, std::mem::take(&mut t.clips))).collect();
                let mut ids = Vec::new();
                for spec in specs {
                    if spec.bars == 0 || spec.bars > 128 {
                        return Err(Error::Parse(format!("section '{}': bars must be 1..128", spec.name)));
                    }
                    let energy = spec.energy.unwrap_or_else(|| default_energy(&spec.name));
                    let id = s.add_section(&spec.name, spec.bars, energy);
                    let bar = s.bar_ticks();
                    let key = s.key;
                    // Reuse chords/clips from an old section with the same id when keeping.
                    if keep.unwrap_or(true) {
                        if let Some(o) = old.iter().find(|o| o.id == id) {
                            if spec.chords.is_none() {
                                s.section_mut(&id).unwrap().chords = o.chords.iter().filter(|c| c.start < spec.bars * bar).cloned().collect();
                            }
                            for (role, clips) in &old_clips {
                                if let Some(c) = clips.get(&id) {
                                    s.track_mut(*role).clips.insert(id.clone(), c.clone());
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
                    ids.push(id);
                }
                Ok(ids)
            })
            .map(|ids| json!({ "ok": true, "section_ids": ids, "summary": describe(&store.session) }))
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
        "suggest_chords" => {
            let style: Option<String> = opt(params, "style")?;
            let count: Option<usize> = opt(params, "count")?;
            let seed: Option<u64> = opt(params, "seed")?;
            let list = suggest_progressions(&store.session, style.as_deref(), count.unwrap_or(4), seed.unwrap_or(store.session.seed + store.revision));
            Ok(json!({ "progressions": list.iter().map(|(sym, rom)| json!({ "chords": sym, "roman": rom })).collect::<Vec<_>>() }))
        }
        "set_notes" => {
            let role = role_arg(params)?;
            let section: String = arg(params, "section")?;
            let notation: String = arg(params, "notation")?;
            let do_humanize: Option<bool> = opt(params, "humanize")?;
            let sess = store.session.clone();
            let sec = sess.section(&section).cloned().ok_or_else(|| Error::UnknownSection(section.clone()))?;
            let mut notes = if role == TrackRole::Drums { parse_drums(&notation)? } else { parse_melody(&notation, 0.8)? };
            let total = sec.bars * sess.bar_ticks();
            let overflow = notes.iter().filter(|n| n.start >= total).count();
            notes.retain(|n| n.start < total);
            for n in notes.iter_mut() {
                n.len = n.len.min(total - n.start).max(1);
            }
            if do_humanize.unwrap_or(true) {
                let mut hp = HumanizeParams::preset(role, &sess.style);
                hp.seed = sess.seed;
                humanize(&mut notes, &hp, role, sess.tempo, sess.bar_ticks());
            }
            let analysis = analyze_clip(&sess, role, &sec, &notes);
            store.mutate(|s| {
                if s.track(role).locked {
                    return Err(Error::Parse(format!("{} track is locked", role.name())));
                }
                s.set_clip(role, &sec.id, Clip::new(notes.clone(), ClipSource::Agent))
            })?;
            let mut msg = analysis.summary();
            if overflow > 0 {
                msg.push_str(&format!("\n  ! {overflow} note(s) beyond the end of the {}-bar section were dropped", sec.bars));
            }
            Ok(json!({ "ok": true, "analysis": msg }))
        }
        "generate" => {
            let role = role_arg(params)?;
            let section: String = arg(params, "section")?;
            let gp: GenParams = serde_json::from_value(params.get("params").cloned().unwrap_or(json!({}))).map_err(|e| Error::Parse(format!("params: {e}")))?;
            let clip = store.mutate(|s| generate_track(s, role, &section, &gp))?;
            let sec = store.session.section(&section).cloned().unwrap();
            let analysis = analyze_clip(&store.session, role, &sec, &clip.notes);
            let notation = if role == TrackRole::Drums { format_drums(&clip.notes, sec.bars, store.session.bar_ticks()) } else { format_melody(&clip.notes) };
            Ok(json!({ "ok": true, "analysis": analysis.summary(), "notation": notation }))
        }
        "generate_all" => {
            let section: String = arg(params, "section")?;
            let gp: GenParams = serde_json::from_value(params.get("params").cloned().unwrap_or(json!({}))).map_err(|e| Error::Parse(format!("params: {e}")))?;
            let mut out = Vec::new();
            store.mutate(|s| {
                for role in [TrackRole::Chords, TrackRole::Melody, TrackRole::Bass, TrackRole::Drums] {
                    if s.track(role).locked {
                        out.push(format!("{}: locked, skipped", role.name()));
                        continue;
                    }
                    let clip = generate_track(s, role, &section, &gp)?;
                    let sec = s.section(&section).cloned().unwrap();
                    out.push(analyze_clip(s, role, &sec, &clip.notes).summary());
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true, "analysis": out.join("\n") }))
        }
        "humanize" => {
            let role = role_arg(params)?;
            let section: Option<String> = opt(params, "section")?;
            let mut hp = HumanizeParams::preset(role, &store.session.style);
            if let Some(p) = params.get("params") {
                if let Value::Object(map) = p {
                    let mut base = serde_json::to_value(&hp).unwrap();
                    for (k, v) in map {
                        base[k] = v.clone();
                    }
                    hp = serde_json::from_value(base).map_err(|e| Error::Parse(format!("params: {e}")))?;
                }
            }
            let sess = store.session.clone();
            store.mutate(|s| {
                let ids: Vec<String> = s.sections.iter().filter(|x| section.as_ref().map(|id| x.id.eq_ignore_ascii_case(id) || x.name.eq_ignore_ascii_case(id)).unwrap_or(true)).map(|x| x.id.clone()).collect();
                if ids.is_empty() {
                    return Err(Error::UnknownSection(section.unwrap_or_default()));
                }
                for id in ids {
                    if let Some(clip) = s.track_mut(role).clips.get_mut(&id) {
                        humanize(&mut clip.notes, &hp, role, sess.tempo, sess.bar_ticks());
                        clip.sort();
                    }
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true, "params": hp }))
        }
        "analyze" => {
            let role: Option<String> = opt(params, "track")?;
            let section: Option<String> = opt(params, "section")?;
            let roles: Vec<TrackRole> = match role {
                Some(r) => vec![TrackRole::parse(&r).ok_or_else(|| Error::UnknownTrack(r))?],
                None => TrackRole::ALL.to_vec(),
            };
            let mut lines = Vec::new();
            for sec in store.session.sections.iter().filter(|x| section.as_ref().map(|id| x.id.eq_ignore_ascii_case(id) || x.name.eq_ignore_ascii_case(id)).unwrap_or(true)) {
                for &r in &roles {
                    let notes = store.session.clip(r, &sec.id).map(|c| c.notes.clone()).unwrap_or_default();
                    lines.push(analyze_clip(&store.session, r, sec, &notes).summary());
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
                Ok(())
            })?;
            Ok(json!({ "ok": true, "summary": describe(&store.session) }))
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
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "lock" => {
            let role = role_arg(params)?;
            let locked: bool = arg(params, "locked")?;
            store.mutate(|s| {
                s.track_mut(role).locked = locked;
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "clear" => {
            let role: Option<String> = opt(params, "track")?;
            let section: Option<String> = opt(params, "section")?;
            store.mutate(|s| {
                let roles: Vec<TrackRole> = match role {
                    Some(r) => vec![TrackRole::parse(&r).ok_or_else(|| Error::UnknownTrack(r))?],
                    None => TrackRole::ALL.to_vec(),
                };
                let ids: Vec<String> = s.sections.iter().filter(|x| section.as_ref().map(|id| x.id.eq_ignore_ascii_case(id) || x.name.eq_ignore_ascii_case(id)).unwrap_or(true)).map(|x| x.id.clone()).collect();
                for r in roles {
                    for id in &ids {
                        s.track_mut(r).clips.remove(id);
                    }
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "transpose" => {
            let role: Option<String> = opt(params, "track")?;
            let section: Option<String> = opt(params, "section")?;
            let semitones: i32 = arg(params, "semitones")?;
            store.mutate(|s| {
                let roles: Vec<TrackRole> = match role {
                    Some(r) => vec![TrackRole::parse(&r).ok_or_else(|| Error::UnknownTrack(r))?],
                    None => vec![TrackRole::Chords, TrackRole::Melody, TrackRole::Bass],
                };
                let ids: Vec<String> = s.sections.iter().filter(|x| section.as_ref().map(|id| x.id.eq_ignore_ascii_case(id) || x.name.eq_ignore_ascii_case(id)).unwrap_or(true)).map(|x| x.id.clone()).collect();
                for r in roles {
                    for id in &ids {
                        if let Some(c) = s.track_mut(r).clips.get_mut(id) {
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
                "hint": "In FL Studio: open the target piano roll, then Tools > Scripts > FLVSTX Import (or drag latest.mid from the folder onto a Channel Rack slot)." }))
        }
        "list_scales" => Ok(json!({ "scales": ScaleKind::ALL.iter().map(|s| s.name()).collect::<Vec<_>>() })),
        _ => Err(Error::Parse(format!("unknown method '{method}'"))),
    }
}

fn default_energy(name: &str) -> f32 {
    let n = name.to_ascii_lowercase();
    if n.contains("intro") {
        0.3
    } else if n.contains("verse") {
        0.5
    } else if n.contains("pre") {
        0.65
    } else if n.contains("chorus") || n.contains("drop") || n.contains("hook") {
        0.85
    } else if n.contains("bridge") {
        0.55
    } else if n.contains("outro") {
        0.3
    } else if n.contains("break") {
        0.35
    } else {
        0.6
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
    fn demo_songs_for_all_styles() {
        for style in ["pop", "lofi", "trap", "house", "cinematic", "kids"] {
            let s = demo_session(style, "C major", 8, 3).unwrap();
            for role in TrackRole::ALL {
                assert!(!s.flatten(role).is_empty(), "{style} {}", role.name());
            }
        }
    }
}
