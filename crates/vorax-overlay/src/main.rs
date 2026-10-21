//! Vorax overlay: follows the game's log, reads the Surgical Preparation screen when the board changes, and marks
//! the card and tanks the advisor would pick, over the game. Read-only: it never clicks or types for you.
//!
//!   vorax-overlay                         run over the game (Windows)
//!   vorax-overlay --preview in.png out.png   draw the overlay onto a saved screenshot
//! Options: --shelley N (Intern Shelley level, 1-6; default 6), --log <UE_game.log>, --no-ocr, --console

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod draw;
mod pipeline;
#[cfg(windows)]
mod app;

use std::path::PathBuf;

pub fn arg<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--preview") {
        let (Some(input), Some(output)) = (args.get(i + 1), args.get(i + 2)) else {
            eprintln!("usage: vorax-overlay --preview <screenshot.png> <out.png> [--shelley N]");
            std::process::exit(2);
        };
        if let Err(e) = preview(&PathBuf::from(input), &PathBuf::from(output), arg(&args, "--shelley").unwrap_or(6), arg(&args, "--accent").unwrap_or(0)) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(windows)]
    app::run(&args);
    #[cfg(not(windows))]
    eprintln!("The live overlay runs on Windows; --preview works anywhere.");
}

/// The overlay drawn onto a screenshot, as it would sit over the game.
fn preview(input: &std::path::Path, output: &std::path::Path, shelley: i32, accent: usize) -> Result<(), String> {
    let frame = pipeline::decode_png(input)?;
    let fonts = draw::Fonts::system().ok_or("no system font")?;
    let mut p = pipeline::Pipeline::new(shelley);
    let t = std::time::Instant::now();
    let plan = match p.run(&frame, None, None) {
        pipeline::Outcome::Advice { reading, advice, plan, read_ms } => {
            eprintln!("read in {read_ms:.0} ms, advised in {} ms: {}", advice.ms, plan.pill.as_ref().map_or(String::new(), |p| format!("{} · {}", p.main, p.sub)));
            eprintln!("board: {}", serde_json::to_string(&reading.state).unwrap());
            plan
        }
        pipeline::Outcome::Animating => return Err("a card is flying off: nothing to advise".into()),
        pipeline::Outcome::NotGame => return Err("not the Surgical Preparation screen".into()),
    };
    eprintln!("total {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    // a windowed capture's marks sit inside its frame
    let (ox, oy) = vorax_scan::window::windowed_game(&frame).map_or((0, 0), |a| (a.x as usize, a.y as usize));
    let (gw, gh) = vorax_scan::window::windowed_game(&frame).map_or((frame.width, frame.height), |a| (a.w as usize, a.h as usize));
    let overlay = draw::render(&plan, gw as u32, gh as u32, &fonts, draw::accent(accent));
    let mut out = frame.data.clone();
    let src = overlay.data();
    for y in 0..gh {
        for x in 0..gw {
            let s = (y * gw + x) * 4;
            let d = ((y + oy) * frame.width + x + ox) * 4;
            let a = src[s + 3] as f32 / 255.0;
            for k in 0..3 {
                out[d + k] = (src[s + k] as f32 + out[d + k] as f32 * (1.0 - a)).round().min(255.0) as u8;
            }
        }
    }
    pipeline::encode_png(output, frame.width as u32, frame.height as u32, &out)
}
