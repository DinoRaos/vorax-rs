//! Rust port of tidewarden/tli-vorax-simulator's engine, bot and advisor.
//!
//! Read-only advice: like upstream, nothing built on this crate may play the real game (no clicks, key presses
//! or other input automation acting on its advice). See AGENTS.md upstream.

pub mod advisor;
pub mod bot;
pub mod context;
pub mod data;
pub mod effects;
pub mod game;
pub mod rng;
pub mod stack;
pub mod types;
pub mod value;

pub use data::{data, Data};
pub use types::*;
