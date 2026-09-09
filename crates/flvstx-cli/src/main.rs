//! flvstx-cli — developer harness. Renders demo songs, runs session operations from the command line,
//! and hosts a terminal chat with the agent sidecar (see `chat`).

use anyhow::{bail, Context, Result};
use flvstx_core::midi::{export_to_dir, write_smf, MidiTrack};
use flvstx_core::ops::{demo_session, describe, dispatch, Store};
use flvstx_core::Session;
use std::path::PathBuf;

const USAGE: &str = "flvstx-cli <command> [args]

  demo [--style S] [--key K] [--bars N] [--seed N] [--out FILE.mid]   render a demo song
  render SESSION.json OUT.mid                                          render a saved session to MIDI
  wav SESSION.json OUT.wav [--section ID] [--soundfont FILE.sf2]       bounce audio through the soundfont
  export SESSION.json [DIR]                                            write latest.json/.mid for the FL script
  op SESSION.json METHOD '{json params}'                               run one session operation and save
  describe SESSION.json                                                print the compact summary
  chat [--port N] [SESSION.json]                                       terminal chat via the agent sidecar
";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// Reads a bare session or a `.flvstx` project (which wraps one).
fn load(path: &str) -> Result<Session> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let v: serde_json::Value = serde_json::from_str(&text).with_context(|| format!("parsing {path}"))?;
    let inner = if v.get("session").is_some() { v["session"].clone() } else { v };
    Ok(serde_json::from_value(inner).with_context(|| format!("{path} is not a FLVSTX session"))?)
}

/// Writes a `.flvstx` project (openable in the app) or a bare session, by extension. An existing
/// project's chat and selection are preserved.
fn save(path: &str, s: &Session) -> Result<()> {
    if !path.to_ascii_lowercase().ends_with(".flvstx") {
        std::fs::write(path, serde_json::to_string_pretty(s)?)?;
        return Ok(());
    }
    let mut doc = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(|v| v.get("session").is_some())
        .unwrap_or_else(|| serde_json::json!({ "chat": [], "ui_scale": 1.5 }));
    doc["session"] = serde_json::to_value(s)?;
    if doc.get("selected_section").and_then(|x| x.as_str()).is_none() {
        if let Some(first) = s.sections.first() {
            doc["selected_section"] = serde_json::Value::String(first.id.clone());
        }
    }
    std::fs::write(path, serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

fn render(session: &Session, out: &PathBuf) -> Result<()> {
    type TrackData = (String, u8, Vec<flvstx_core::Note>, Vec<(u32, flvstx_core::AutoTarget, f32)>, u8);
    let all: Vec<TrackData> = session
        .tracks
        .iter()
        .map(|t| (t.id.clone(), t.channel, session.flatten(&t.id), session.flatten_automation(&t.id), t.bend_range))
        .collect();
    let tracks: Vec<MidiTrack> = all.iter().map(|(id, ch, n, a, br)| MidiTrack { name: id, channel: *ch, notes: n, automation: a, bend_range: *br }).collect();
    write_smf(out, session.tempo, (session.time_sig.num, session.time_sig.den), &tracks)?;
    println!("wrote {}", out.display());
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        print!("{USAGE}");
        return Ok(());
    };
    match cmd.as_str() {
        "demo" => {
            let style = flag(&args, "--style").unwrap_or_else(|| "pop".into());
            let key = flag(&args, "--key").unwrap_or_else(|| "C major".into());
            let bars: u32 = flag(&args, "--bars").map(|b| b.parse()).transpose()?.unwrap_or(8);
            let seed: u64 = flag(&args, "--seed").map(|b| b.parse()).transpose()?.unwrap_or(1);
            let out = PathBuf::from(flag(&args, "--out").unwrap_or_else(|| format!("demo-{}.mid", style.replace(' ', "_"))));
            let session = demo_session(&style, &key, bars, seed)?;
            println!("{}", describe(&session));
            render(&session, &out)?;
            if let Some(sp) = flag(&args, "--save") {
                save(&sp, &session)?;
            }
        }
        "wav" => {
            let session = load(args.get(1).context("SESSION.json")?)?;
            let out = PathBuf::from(args.get(2).context("OUT.wav")?);
            let section = flag(&args, "--section");
            let sf_path = flag(&args, "--soundfont").map(PathBuf::from).unwrap_or_else(flvstx_core::render::default_soundfont_path);
            let sf = flvstx_core::render::load_soundfont(&sf_path).map_err(anyhow::Error::msg)?;
            let (l, r) = flvstx_core::render::render_stereo(&session, section.as_deref(), &[], &sf).map_err(anyhow::Error::msg)?;
            let (peak, gain) = flvstx_core::render::peak_and_gain(&l, &r);
            flvstx_core::render::write_wav(&out, &l, &r, gain).map_err(anyhow::Error::msg)?;
            println!("wrote {} ({:.1}s, peak {:.2})", out.display(), l.len() as f32 / flvstx_core::render::SAMPLE_RATE as f32, peak);
        }
        "render" => {
            let session = load(args.get(1).context("SESSION.json")?)?;
            render(&session, &PathBuf::from(args.get(2).context("OUT.mid")?))?;
        }
        "export" => {
            let session = load(args.get(1).context("SESSION.json")?)?;
            let dir = args.get(2).map(PathBuf::from).unwrap_or_else(flvstx_core::midi::default_export_dir);
            for f in export_to_dir(&session, None, &dir)? {
                println!("wrote {}", f.display());
            }
        }
        "describe" => {
            let session = load(args.get(1).context("SESSION.json")?)?;
            println!("{}", describe(&session));
        }
        "op" => {
            let path = args.get(1).context("SESSION.json")?;
            let session = if std::path::Path::new(path).exists() { load(path)? } else { Session::default() };
            let method = args.get(2).context("METHOD")?;
            let params: serde_json::Value = serde_json::from_str(args.get(3).map(String::as_str).unwrap_or("{}"))?;
            let mut store = Store::new(session);
            let out = dispatch(&mut store, method, &params)?;
            println!("{}", serde_json::to_string_pretty(&out)?);
            save(path, &store.session)?;
        }
        "chat" => {
            let port: u16 = flag(&args, "--port").map(|p| p.parse()).transpose()?.unwrap_or(7878);
            let session_path = args.iter().skip(1).find(|a| a.ends_with(".json")).cloned();
            let session = match &session_path {
                Some(p) if std::path::Path::new(p).exists() => load(p)?,
                _ => Session::default(),
            };
            flvstx_ipc::chat::run_terminal_chat(session, port, session_path)?;
        }
        _ => bail!("unknown command '{cmd}'\n{USAGE}"),
    }
    Ok(())
}
