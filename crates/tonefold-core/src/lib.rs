//! tonefold-core — pure-Rust composition engine: session model, music theory, compact notation,
//! rule-based generators, humanization, analysis and MIDI I/O. No plugin or host dependencies.

pub mod analyze;
pub mod generate;
pub mod gm;
pub mod humanize;
pub mod midi;
pub mod model;
pub mod notation;
pub mod ops;
pub mod playback;
#[cfg(feature = "render")]
pub mod render;
pub mod theory;
pub mod voicing;

pub use model::*;
pub use theory::{Chord, ChordQuality, Key, ScaleKind};
pub use generate::SongCtx;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unknown section '{0}'")]
    UnknownSection(String),
    #[error("unknown track '{0}'")]
    UnknownTrack(String),
    #[error("midi error: {0}")]
    Midi(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Per-user data folder: the soundfont, exports, the installed composer and logs.
/// Windows `%LOCALAPPDATA%\Tonefold`, macOS `~/Library/Application Support/Tonefold`,
/// elsewhere `$XDG_DATA_HOME/tonefold` (or `~/.local/share/tonefold`).
pub fn data_dir() -> std::path::PathBuf {
    use std::path::PathBuf;
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let dir = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Tonefold"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library").join("Application Support").join("Tonefold"))
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".local").join("share"))).map(|d| d.join("tonefold"))
    };
    dir.unwrap_or_else(|| std::env::temp_dir().join("Tonefold"))
}

/// `Contents/Resources` of the macOS app bundle this executable runs from, if any (the app itself
/// is `Contents/MacOS/Tonefold`, the CLI `Contents/Resources/bin/tonefold-cli`).
pub fn bundle_resources() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents = exe.ancestors().find(|a| a.file_name().is_some_and(|n| n == "Contents") && a.parent().is_some_and(|p| p.extension().is_some_and(|e| e == "app")))?;
    Some(contents.join("Resources"))
}

/// Reads `TONEFOLD_<name>`, falling back to `FLVSTX_<name>` from before the rename.
pub fn env_var(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(format!("TONEFOLD_{name}")).or_else(|| std::env::var_os(format!("FLVSTX_{name}")))
}
