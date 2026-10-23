//! The worker thread: follows the game's log, and when it says the board changed (and the animations have
//! settled), takes a picture of the game, reads it, asks the advisor and hands the marks to the window.

use super::capture::Capturer;
use super::ocr::Ocr;
use super::sys::{find_game_window, find_log, note};
use super::window::{GameLog, ToUi, ToWorker, Waker};
use crate::pipeline::{Outcome, Pipeline};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use vorax_scan::live::{log_time, Follower, Settler, Trigger};

pub struct Options {
    pub log: Option<PathBuf>,
    pub shelley: i32,
    pub ocr: bool,
    /// --frames <dir>: every picture read is saved there, named by what came of it
    pub frames: Option<PathBuf>,
}

/// How long after a pick the board is looked at again, for the new icon to have landed in the panel.
const RELOOK: Duration = Duration::from_millis(1500);
/// A card still in flight: the board is looked at again this much later, this many times, before its marks are
/// hidden (the board they were for is gone, and no log line may come to say when the new one has landed).
const RETRY: Duration = Duration::from_millis(700);
const RETRIES: u32 = 3;

/// A line the game writes only with its log on (Esc → Account Related → Enable Log): with it on, one comes at
/// least every ~10 s (pings, sounds, clicks); with it off, only a few as the game starts.
const GAME_LINE: &str = "TLLua: Display: [Game]";
/// How long the game can be open without a game line before its log counts as off.
const QUIET: Duration = Duration::from_secs(30);

pub fn run(opts: Options, tx: Sender<ToUi>, waker: Waker, rx: Receiver<ToWorker>) {
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    let send = |m: ToUi| {
        let _ = tx.send(m);
        waker.wake();
    };
    let capturer = match Capturer::new() {
        Ok(c) => c,
        Err(e) => {
            note(&format!("no screen capture: {e}"));
            return;
        }
    };
    let ocr = if opts.ocr {
        match Ocr::new() {
            Ok(o) => {
                note(&format!("card titles read with Windows OCR ({})", o.language));
                Some(o)
            }
            Err(e) => {
                note(&format!("no Windows OCR ({e}): cards read by their pictures"));
                None
            }
        }
    } else {
        None
    };
    if let Some(dir) = &opts.frames {
        note(&format!("saving every picture read to {}", dir.display()));
    }
    let mut pipe = Pipeline::new(opts.shelley);
    let mut follower: Option<Follower> = None;
    let mut looked_for_log = Instant::now() - Duration::from_secs(60);
    let mut settler = Settler::default();
    let mut last_poll = Instant::now() - Duration::from_secs(1);
    let mut relook_at: Option<Instant> = None;
    // a look again at a card in flight: when, for which trigger, and how many have been made
    let mut retry: Option<(Instant, Option<&'static str>)> = None;
    let mut retries = 0;
    let mut was_over = false;
    // the game's log: when it last had a game line, since when the game is open, and what the window was told
    let mut game_line_at: Option<Instant> = None;
    let mut game_open_since: Option<Instant> = None;
    let mut game_log: Option<GameLog> = None;
    let mut last_log_check = Instant::now() - Duration::from_secs(1);
    let t0 = Instant::now();
    let now_ms = || t0.elapsed().as_secs_f64() * 1000.0;

    loop {
        let mut manual = false;
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                ToWorker::Quit => return,
                ToWorker::Snap => manual = true,
                ToWorker::Shelley(level) => {
                    pipe.shelley = level;
                    if let Some((plan, w, h)) = pipe.readvise() {
                        send(ToUi::Show(plan, w, h));
                    }
                }
            }
        }
        if follower.is_none() && looked_for_log.elapsed() > Duration::from_secs(10) {
            looked_for_log = Instant::now();
            let found = opts.log.clone().or_else(find_log);
            match found {
                Some(p) => {
                    note(&format!("following the game's log: {}", p.display()));
                    // a game already open with its log on: known at once, not half a minute later
                    if let Some(age) = last_game_line_age(&p) {
                        game_line_at = Instant::now().checked_sub(age);
                    }
                    follower = Some(Follower::new(p));
                }
                None => note("UE_game.log not found (start the game, or pass --log <path>)"),
            }
        }
        if last_poll.elapsed() >= Duration::from_millis(250) {
            last_poll = Instant::now();
            if let Some(f) = &mut follower {
                let lines = f.poll();
                if !lines.is_empty() {
                    settler.feed(&lines, now_ms());
                    if lines.iter().any(|l| l.contains(GAME_LINE)) {
                        game_line_at = Some(Instant::now());
                    }
                }
            }
            if settler.over && !was_over {
                note("the game's Vorax screen closed");
                send(ToUi::Clear);
                retry = None;
            }
            was_over = settler.over;
        }
        if last_log_check.elapsed() >= Duration::from_secs(1) {
            last_log_check = Instant::now();
            game_open_since = if find_game_window().is_some() { game_open_since.or(Some(Instant::now())) } else { None };
            let state = match (&follower, game_open_since) {
                (None, _) => GameLog::Missing,
                (Some(f), _) if !f.path().is_file() => GameLog::Missing,
                (_, None) => GameLog::NoGame,
                _ if game_line_at.is_some_and(|t| t.elapsed() < QUIET) => GameLog::On,
                (_, Some(t)) if t.elapsed() < QUIET => GameLog::Checking,
                _ => GameLog::Off,
            };
            if game_log != Some(state) {
                match state {
                    GameLog::On => note("the game's log has game lines: boards are read by themselves"),
                    GameLog::Off => note("the game is open but its log has no game lines for 30 s: Enable Log is off (Esc → Account Related)"),
                    GameLog::Missing => note("the game's log isn't there"),
                    _ => {}
                }
                game_log = Some(state);
                send(ToUi::GameLog(state));
            }
        }
        let due = settler.due(now_ms());
        let relook = relook_at.is_some_and(|t| Instant::now() >= t);
        let again = retry.filter(|(t, _)| Instant::now() >= *t);
        if due.is_some() || manual || relook || again.is_some() {
            relook_at = None;
            retry = None;
            let trigger = match due {
                Some(Trigger::Screen) => Some("screen"),
                Some(_) => Some("board"),
                None => again.and_then(|(_, t)| t),
            };
            if due.is_some() || manual {
                retries = 0;
            }
            // the look again after a pick, to settle the new icon: the board hasn't changed, so the marks stay up
            // while it reads and only change if what it finds does
            let quiet = relook && due.is_none() && !manual && again.is_none();
            let animating = read(&capturer, ocr.as_ref(), &mut pipe, trigger, manual || relook, quiet, opts.frames.as_deref(), &send);
            if !animating || quiet {
                retries = 0;
            } else if retries < RETRIES {
                retries += 1;
                retry = Some((Instant::now() + RETRY, trigger));
            } else {
                retries = 0;
                note(&format!("still a card in flight after {RETRIES} more looks: the marks are hidden until the next board"));
                send(ToUi::Clear);
            }
            if pipe.wants_relook() {
                relook_at = Some(Instant::now() + RELOOK);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// How long ago the log's last game line was written, from the log's last megabyte; None without one.
fn last_game_line_age(path: &Path) -> Option<Duration> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(size.saturating_sub(1 << 20))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let at = String::from_utf8_lossy(&buf).lines().rev().find(|l| l.contains(GAME_LINE)).and_then(log_time)?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64() * 1000.0;
    Some(Duration::from_millis((now - at).max(0.0) as u64))
}

/// Takes a picture, reads it and advises; true when a card was in flight (nothing read).
#[allow(clippy::too_many_arguments)]
fn read(capturer: &Capturer, ocr: Option<&Ocr>, pipe: &mut Pipeline, trigger: Option<&str>, now: bool, quiet: bool, frames: Option<&Path>, send: &dyn Fn(ToUi)) -> bool {
    let Some(game) = find_game_window() else {
        note("the game's window isn't open");
        return false;
    };
    if !quiet {
        send(ToUi::Busy);
    }
    let t = Instant::now();
    let frame = if now { capturer.grab(game) } else { capturer.grab_stable(game) };
    let frame = match frame {
        Ok(Some(f)) => f,
        Ok(None) => {
            note("no picture from the game's window");
            send(ToUi::Unbusy);
            return false;
        }
        Err(e) => {
            note(&format!("capture failed: {e}"));
            send(ToUi::Unbusy);
            return false;
        }
    };
    let grabbed = t.elapsed().as_millis();
    let mut read_text = |img: &vorax_scan::pixels::Img| ocr.and_then(|o| o.read(img).ok()).unwrap_or_default();
    let reader: crate::pipeline::TextReader = if ocr.is_some() { Some(&mut read_text) } else { None };
    let outcome = pipe.run(&frame, trigger, reader);
    if pipe.resumed {
        note("the Vorax screen opened again on the game under way: it goes on (instruments kept)");
    }
    let kind = match &outcome {
        Outcome::Advice { .. } => "board",
        Outcome::Animating => "animating",
        Outcome::NotGame => "notgame",
        Outcome::Closing => "closing",
    };
    let animating = matches!(outcome, Outcome::Animating);
    match outcome {
        Outcome::Advice { reading, advice, plan, read_ms } => {
            let mv = plan.pill.as_ref().map_or("(nothing to advise)".to_string(), |p| format!("{} · {}", p.main, p.sub));
            note(&format!(
                "{} {}x{}: picture {grabbed} ms, read {read_ms:.0} ms, advice {} ms → {mv}; board {}",
                trigger.unwrap_or("manual"),
                frame.width,
                frame.height,
                advice.ms,
                serde_json::to_string(&reading.state).unwrap_or_default()
            ));
            let (w, h) = vorax_scan::window::windowed_game(&frame).map_or((frame.width, frame.height), |a| (a.w as usize, a.h as usize));
            send(ToUi::Show(plan, w, h));
        }
        // the board changed and a card is still moving: the marks stay hidden (the pill dimmed) while it looks again,
        // rather than the old ones blinking back between looks (2-3 flashes while long effects play out)
        Outcome::Animating => {
            note(&format!("{}: picture {grabbed} ms, a card was flying off: looking again", trigger.unwrap_or("manual")));
        }
        Outcome::Closing => {
            note(&format!("{}: the game closing after the protocols, not a board", trigger.unwrap_or("manual")));
            send(ToUi::Clear);
        }
        Outcome::NotGame => {
            note(&format!("{}: not the Surgical Preparation screen", trigger.unwrap_or("manual")));
            send(ToUi::Clear);
        }
    }
    if let Some(dir) = frames {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let path = dir.join(format!("{ms}-{}-{kind}.png", trigger.unwrap_or("manual")));
        let saved = std::fs::create_dir_all(dir).map_err(|e| e.to_string()).and_then(|_| crate::pipeline::encode_png(&path, frame.width as u32, frame.height as u32, &frame.data));
        if let Err(e) = saved {
            note(&format!("frame not saved: {e}"));
        }
    }
    animating
}
