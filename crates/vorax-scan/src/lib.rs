//! Rust port of tidewarden/tli-vorax-simulator's screen reader (src/solver/scan, track.ts, live/windowCrop.ts):
//! a frame of the Surgical Preparation screen into the advisor's ScreenState. Read-only: it looks at pictures.

pub mod cards;
pub mod data;
pub mod instruments;
pub mod layout;
pub mod levels;
pub mod live;
pub mod pixels;
pub mod reading;
pub mod scan;
pub mod tanks;
pub mod text;
pub mod titles;
pub mod track;
pub mod window;

pub use data::{scan_data, ScanData};
pub use pixels::Img;
pub use reading::Reading;
pub use scan::{scan, scan_upstream};
