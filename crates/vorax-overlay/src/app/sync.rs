//! "Sync and rebuild": runs scripts/sync.ps1 (pull upstream, re-record its traces, the parity tests, the rebuild)
//! from the tray, for when upstream changed rules the fetched data can't carry. The script's `[n/6] what` lines
//! are its progress; all it prints goes to %APPDATA%\VoraxRs\sync-output.log.
//!
//! The rebuild writes target\release\vorax-overlay.exe, the file this overlay runs from, and Windows won't let a
//! running program's file be overwritten, only renamed: so its names are moved aside first (and back if the
//! sync stops before a new one is built).

use super::sys::{data_dir, note};
use super::window::Waker;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::Instant;

pub const STEPS: u32 = 6;

pub enum SyncMsg {
    /// the script began step `n` of STEPS
    Step(u32, String),
    /// a line it printed
    Line(String),
    Done(Outcome),
}

pub enum Outcome {
    /// parity passed and the binaries were rebuilt: the script's summary
    Rebuilt(String),
    /// the port no longer plays like upstream: a rule needs porting by hand
    ParityFailed,
    /// it stopped: what it said last
    Failed(String),
}

/// The repository the overlay was built in (where scripts/sync.ps1 is): above target\release when it runs from
/// there, else where it was compiled.
pub fn root() -> Option<PathBuf> {
    let from_exe = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.parent()?.parent()?.to_path_buf()));
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    [from_exe, Some(built)].into_iter().flatten().find(|r| r.join("scripts").join("sync.ps1").is_file())
}

/// The overlay as the rebuild leaves it.
pub fn rebuilt_exe(root: &Path) -> PathBuf {
    root.join("target").join("release").join("vorax-overlay.exe")
}

pub fn output_log() -> PathBuf {
    data_dir().join("sync-output.log")
}

/// The names the build writes the overlay to (cargo hard-links the one in target\release to the one in deps).
fn exe_names(root: &Path) -> [PathBuf; 2] {
    let release = root.join("target").join("release");
    [release.join("vorax-overlay.exe"), release.join("deps").join("vorax_overlay.exe")]
}

fn aside(p: &Path) -> PathBuf {
    p.with_extension("old.exe")
}

/// Moves the overlay's file names out of the build's way (and clears what an earlier sync moved aside).
fn move_aside(root: &Path) {
    for p in exe_names(root) {
        let old = aside(&p);
        let _ = std::fs::remove_file(&old);
        if p.is_file() {
            if let Err(e) = std::fs::rename(&p, &old) {
                note(&format!("sync: couldn't move {} aside: {e}", p.display()));
            }
        }
    }
}

/// Puts back the names the build didn't replace.
fn move_back(root: &Path) {
    for p in exe_names(root) {
        let old = aside(&p);
        if !p.exists() && old.is_file() {
            let _ = std::fs::rename(&old, &p);
        }
    }
}

/// Runs the sync on its own threads, telling `tx` (and waking the window) as it goes.
pub fn start(root: PathBuf, tx: Sender<SyncMsg>, waker: Waker) {
    std::thread::spawn(move || {
        let send = |m: SyncMsg| {
            let _ = tx.send(m);
            waker.wake();
        };
        let outcome = run(&root, &send);
        match &outcome {
            Outcome::Rebuilt(info) => note(&format!("sync: rebuilt ({info})")),
            Outcome::ParityFailed => note("sync: parity failed, the port needs updating"),
            Outcome::Failed(e) => note(&format!("sync failed: {e}")),
        }
        send(SyncMsg::Done(outcome));
    });
}

pub fn run(root: &Path, send: &dyn Fn(SyncMsg)) -> Outcome {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    note("sync: starting scripts/sync.ps1");
    move_aside(root);
    let script = root.join("scripts").join("sync.ps1");
    // PowerShell 7 first: Windows PowerShell 5 turns cargo's stderr into errors under the script's 'Stop'
    let child = ["pwsh.exe", "powershell.exe"].iter().find_map(|shell| {
        Command::new(shell)
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .ok()
    });
    let Some(mut child) = child else {
        move_back(root);
        return Outcome::Failed("PowerShell couldn't be started".into());
    };
    let mut out = std::fs::File::create(output_log()).ok();
    let (lines_tx, lines_rx) = std::sync::mpsc::channel::<String>();
    let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)]
        .into_iter()
        .flatten()
        .map(|pipe| {
            let tx = lines_tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            })
        })
        .collect();
    drop(lines_tx);
    let (mut summary, mut parity_failed, mut last) = (None, false, String::new());
    for line in lines_rx {
        let line = plain(&line);
        if let Some(f) = out.as_mut() {
            let _ = writeln!(f, "{line}");
        }
        // PowerShell frames its errors' lines with '|'
        let text = line.trim().trim_start_matches('|').trim();
        if text.is_empty() {
            continue;
        }
        if let Some((n, what)) = step(text) {
            send(SyncMsg::Step(n, what));
            continue;
        }
        if let Some(i) = text.find("OK: ") {
            summary = Some(text[i + 4..].to_string());
        }
        parity_failed |= text.contains("PARITY FAILED");
        last = text.to_string();
        send(SyncMsg::Line(last.clone()));
    }
    for r in readers {
        let _ = r.join();
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    move_back(root);
    match (ok, summary) {
        (true, Some(s)) => Outcome::Rebuilt(s),
        (true, None) => Outcome::Rebuilt("parity passed, binaries rebuilt".into()),
        _ if parity_failed => Outcome::ParityFailed,
        _ => Outcome::Failed(if last.is_empty() { "the script stopped".into() } else { last }),
    }
}

/// A line without its colours (PowerShell's errors and cargo's come with ANSI escapes: ESC [ ... letter).
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// `[n/6] what` → (n, what)
fn step(line: &str) -> Option<(u32, String)> {
    let rest = line.strip_prefix('[')?;
    let (n, rest) = rest.split_once('/')?;
    let (_, what) = rest.split_once("] ")?;
    Some((n.parse().ok()?, what.to_string()))
}

/// How far along the sync is, 0-1: the steps done, and the one under way creeping on with the time it has
/// taken against the time it usually takes (recording the traces is most of it).
pub fn progress(step: u32, since: Instant) -> f32 {
    // usual seconds per step, 1-6
    const USUAL: [f32; 6] = [5.0, 60.0, 15.0, 150.0, 60.0, 60.0];
    let total: f32 = USUAL.iter().sum();
    let i = (step.clamp(1, STEPS) - 1) as usize;
    let done: f32 = USUAL[..i].iter().sum();
    let t = since.elapsed().as_secs_f32();
    let within = USUAL[i] * (1.0 - (-t / USUAL[i]).exp()) * 0.95;
    ((done + within) / total).clamp(0.0, 0.99)
}
