//! Which game-log lines mean "the board changed", and when its animations have settled enough to look
//! (src/solver/live/triggers.ts), following the log as it grows (logFollower.ts), and the round when its number
//! can't be read (round.ts).

use crate::tanks::js_number;
use vorax::advisor::ScreenPhase;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    Board,
    Screen,
    Refresh,
    Over,
}

impl Trigger {
    pub fn name(self) -> &'static str {
        match self {
            Trigger::Board => "board",
            Trigger::Screen => "screen",
            Trigger::Refresh => "refresh",
            Trigger::Over => "over",
        }
    }
}

// the server's reply after Confirm or Refresh: the new board
const BOARD: &[&str] = &["ProtoId = 5619"];
// the game screen opens: the first board and the starting instrument offer
const SCREEN: &[&str] = &["S13GamePlayMain Run", "S13GamePlay_C_StartGameBtn"];
const REFRESH: &[&str] = &["S13GamePlayMain_C_RefreshBtn"];
const OVER: &[&str] = &["ProtoId = 5621", "S13GamePlayMain Hide", "S13GamePlayReward_C_FightBtn", "S13GamePlayReward_C_GiveUpBtn"];
const ANIMATION: &str = "Play_UI_S13_KYZ_Card_";
const PLAYER_SOUNDS: &[&str] = &["KYZ_Card_Selected'", "KYZ_Card_Selected_Confirm", "KYZ_Card_Agentia_Use"];
const DONE: &str = "KYZ_Card_Figure_Refresh";
const PLAYER_ACTIONS: &[&str] = &["normal-click SelectBtn", "normal-click ComfirmBtn", "normal-click RefreshBtn", "normal-click RelicBtn"];
const DONE_DELAY_MS: f64 = 150.0;
const SETTLE_MS: f64 = 400.0;
const MAX_MS: f64 = 3000.0;

pub fn classify(line: &str) -> Option<Trigger> {
    for (k, pats) in [(Trigger::Board, BOARD), (Trigger::Screen, SCREEN), (Trigger::Refresh, REFRESH), (Trigger::Over, OVER)] {
        if pats.iter().any(|p| line.contains(p)) {
            return Some(k);
        }
    }
    None
}

pub fn is_animation(line: &str) -> bool {
    line.contains(ANIMATION) && !PLAYER_SOUNDS.iter().any(|p| line.contains(p))
}

/// The time at the start of a log line, "[2026.10.17-14.24.11:563]", in ms (UTC as written); None if it has none.
pub fn log_time(line: &str) -> Option<f64> {
    let b = line.as_bytes();
    if b.len() < 25 || b[0] != b'[' || b[24] != b']' {
        return None;
    }
    let n = |a: usize, z: usize| line.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s, ms) = (n(1, 5)?, n(6, 8)?, n(9, 11)?, n(12, 14)?, n(15, 17)?, n(18, 20)?, n(21, 24)?);
    // days from the civil date (Howard Hinnant's algorithm), as Date.UTC counts them
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((((days * 24 + h) * 60 + mi) * 60 + s) as f64 * 1000.0 + ms as f64)
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    kind: Trigger,
    t0: f64,
    last_anim: f64,
    done_at: Option<f64>,
    acted: bool,
}

/// Decides when to take a picture after the log shows a new board.
#[derive(Default)]
pub struct Settler {
    pending: Option<Pending>,
    pub refreshes: u32,
    pub over: bool,
}

impl Settler {
    pub fn feed<S: AsRef<str>>(&mut self, lines: &[S], now: f64) {
        let start = |kind, now| Pending { kind, t0: now, last_anim: now, done_at: None, acted: false };
        for line in lines {
            let line = line.as_ref();
            match classify(line) {
                Some(Trigger::Screen) => {
                    self.over = false;
                    self.refreshes = 0;
                    self.pending = Some(start(Trigger::Screen, now));
                }
                Some(Trigger::Over) => {
                    self.over = true;
                    self.pending = None;
                }
                Some(Trigger::Refresh) => self.refreshes += 1,
                Some(Trigger::Board) => {
                    if !self.over {
                        self.pending = Some(start(Trigger::Board, now));
                    }
                }
                None => {
                    if let Some(p) = &mut self.pending {
                        if PLAYER_ACTIONS.iter().any(|a| line.contains(a)) {
                            p.acted = true;
                        } else if line.contains(DONE) {
                            p.done_at = Some(now);
                        } else if is_animation(line) {
                            p.last_anim = now;
                        }
                    }
                }
            }
        }
    }

    /// A picture to take now (Board or Screen), at most once per trigger.
    pub fn due(&mut self, now: f64) -> Option<Trigger> {
        let p = self.pending?;
        let settled = p.acted || p.done_at.is_some_and(|d| now - d >= DONE_DELAY_MS) || now - p.last_anim >= SETTLE_MS;
        if settled || now - p.t0 >= MAX_MS {
            self.pending = None;
            return Some(p.kind);
        }
        None
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }
}

/// Follows a growing log: each poll returns the complete lines written since the last one. The log can be over
/// a gigabyte, so the first poll only notes where it ends; at most `max_read` bytes are read per poll.
pub struct Follower {
    path: std::path::PathBuf,
    offset: Option<u64>,
    rest: Vec<u8>,
    max_read: u64,
}

impl Follower {
    pub fn new(path: std::path::PathBuf) -> Follower {
        Follower { path, offset: None, rest: Vec::new(), max_read: 1 << 20 }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn poll(&mut self) -> Vec<String> {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut f) = std::fs::File::open(&self.path) else { return vec![] };
        let Ok(size) = f.metadata().map(|m| m.len()) else { return vec![] };
        let Some(mut offset) = self.offset else {
            self.offset = Some(size);
            return vec![];
        };
        // a new log (the game restarted): from its start
        if size < offset {
            offset = 0;
            self.rest.clear();
        }
        let mut skipped = false;
        if size - offset > self.max_read {
            offset = size - self.max_read;
            self.rest.clear();
            skipped = true;
        }
        if size == offset {
            self.offset = Some(offset);
            return vec![];
        }
        let mut buf = Vec::with_capacity((size - offset) as usize);
        if f.seek(SeekFrom::Start(offset)).is_err() || f.take(size - offset).read_to_end(&mut buf).is_err() {
            return vec![];
        }
        self.offset = Some(offset + buf.len() as u64);
        let mut all = std::mem::take(&mut self.rest);
        all.extend_from_slice(&buf);
        let mut parts: Vec<&[u8]> = all.split(|&b| b == b'\n').collect();
        self.rest = parts.pop().unwrap_or_default().to_vec();
        if skipped && !parts.is_empty() {
            parts.remove(0);
        }
        parts.iter().map(|l| String::from_utf8_lossy(l.strip_suffix(b"\r").unwrap_or(l)).into_owned()).collect()
    }
}

/// The round to advise on when its number wasn't read for sure (an effect over it, a card raised in front of it).
/// It used to fall back to round 1, so the advice played as if twelve rounds were left. Within a game the round
/// only moves on a step at a time, so the round of the board before (`before`) stands in, or the reader's
/// `guess` when it is that round or the next.
pub fn carried_round(before: Option<i32>, phase: ScreenPhase, guess: Option<&str>) -> i32 {
    let Some(before) = before.filter(|&r| r != 0) else { return 1 };
    // rounds 11–13 are the Surgical Protocols; a card or instrument offer after them is a new game
    if phase == ScreenPhase::Protocol {
        return before.max(11);
    }
    if before > 10 {
        return 1;
    }
    let g = guess.map_or(f64::NAN, js_number);
    if g == before as f64 || g == (before + 1) as f64 {
        g as i32
    } else {
        before
    }
}
