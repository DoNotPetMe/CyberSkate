//! Skate 3, hosted for Cyberpunk 2077.
//!
//! The game side (a Cyber Engine Tweaks mod) casts rays around the skater and
//! reads the controller-driven result back every frame; this crate turns those
//! casts into skate collision and grind rails, runs the skate engine on its own
//! thread, and hands back the skater, deck and camera in Night City
//! coordinates.
pub mod coords;
pub mod keyboard;
pub mod rails;
pub mod runtime;
pub mod scan;

pub use coords::Frame;
pub use runtime::{InputSource, Runtime, ScoreState, SkateFrame, Status};
pub use scan::{Collision, PostMemory, Scan};
