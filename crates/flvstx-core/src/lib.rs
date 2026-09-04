//! flvstx-core — pure-Rust composition engine: session model, music theory, compact notation,
//! rule-based generators, humanization, analysis and MIDI I/O. No plugin or host dependencies.

pub mod analyze;
pub mod generate;
pub mod humanize;
pub mod midi;
pub mod model;
pub mod notation;
pub mod ops;
pub mod theory;
pub mod voicing;

pub use model::*;
pub use theory::{Chord, ChordQuality, Key, ScaleKind};

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
