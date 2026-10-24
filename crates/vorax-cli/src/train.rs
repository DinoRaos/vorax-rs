//! Training the board value: simulated games with the built-in odds, every board along the way with how its game
//! ended, and a ridge least-squares fit per round; and the gate a new model must pass against the one in use.
//! Ported from tli-vorax-simulator's scripts/sim/valueData.ts, valueFit.ts and gateLib.ts (7b4c9ba, MIT).

use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use vorax::bot::*;
use vorax::data::data;
use vorax::game::{can_refresh, create_game, dispatch, ultimate_activity};
use vorax::rng::Rng;
use vorax::types::*;
use vorax::value::{feature_count, features, tree_features, TARGET};

const ROUNDS: usize = 13;
/// Games played at once before their boards are added to the sums (bounds the memory the boards take).
const BATCH: u32 = 4096;

/// One round's sums for a least-squares fit: XᵀX (upper triangle of an F×F matrix), Xᵀy, yᵀy and the boards.
struct Sums {
    xtx: Vec<f64>,
    xty: Vec<f64>,
    yy: f64,
    n: f64,
}

impl Sums {
    fn new(f: usize) -> Sums {
        Sums { xtx: vec![0.0; f * f], xty: vec![0.0; f], yy: 0.0, n: 0.0 }
    }

    /// Adds a board: its features (by index, those that aren't 0) and how its game ended.
    fn add(&mut self, f: usize, x: &[(u32, f64)], y: f64) {
        for (a, &(i, xi)) in x.iter().enumerate() {
            let i = i as usize;
            self.xty[i] += xi * y;
            let row = &mut self.xtx[i * f..];
            for &(j, xj) in &x[a..] {
                row[j as usize] += xi * xj;
            }
        }
        self.yy += y * y;
        self.n += 1.0;
    }

    fn a(&self, f: usize, i: usize, j: usize) -> f64 {
        if i <= j {
            self.xtx[i * f + j]
        } else {
            self.xtx[j * f + i]
        }
    }
}

/// A game's boards and how it ended (the score counted up to TARGET, in millions).
struct Game {
    boards: Vec<GameState>,
    y: f64,
}

/// One training game: the steered player (judging boards by the model loaded, or by their score without one),
/// with a share `eps` of moves played at random so boards a better player reaches are seen too.
fn play(seed: u32, eps: f64, smart: bool) -> Game {
    let d = data();
    let chooser = Chooser::Ev(Policy { samples: 2, horizon: 0, roll_samples: None, steer: Some(0.3), value: Some(smart) });
    let mut s = create_game(d.default_settings, None, Some(seed));
    let mut rng = Rng::new(mix(seed, 0xbeef));
    let mut boards = Vec::with_capacity(40);
    for g in 0..300u32 {
        if s.phase == Phase::GameOver {
            break;
        }
        if s.phase != Phase::PotionSelect || s.open_box.is_none() {
            boards.push(s);
        }
        if s.phase == Phase::RoundEnd {
            s = dispatch(&s, Action::Confirm);
            continue;
        }
        if s.phase == Phase::InstrumentSelect {
            let pick = if rng.chance(eps) {
                if can_refresh(&s) && rng.chance(0.3) {
                    Pick::Refresh
                } else {
                    Pick::Card(rng.int(s.hand.len()))
                }
            } else {
                Picker::Value.pick(&s, mix(seed, 5000 + g))
            };
            s = match pick {
                Pick::Refresh => dispatch(&s, Action::Refresh),
                Pick::Card(i) => pick_and_confirm(&s, i),
            };
            continue;
        }
        if rng.chance(eps) {
            let os = options(&s);
            s = if can_refresh(&s) && s.open_box.is_none() && rng.chance(0.2) { dispatch(&s, Action::Refresh) } else { play_opt(&s, &os[rng.int(os.len())]) };
            continue;
        }
        let c = chooser.choose(&s, mix(seed, g));
        if s.open_box.is_none() && c.value <= c.stay.unwrap_or(ultimate_activity(&s) as f64) && can_refresh(&s) {
            s = dispatch(&s, Action::Refresh);
            continue;
        }
        if let Some(o) = c.best {
            s = play_opt(&s, &o);
        }
    }
    Game { boards, y: goal_score(ultimate_activity(&s)) / 1e6 }
}

/// The sums over games `from..to`, per round; games are played on every core, a batch at a time.
fn gather(from: u32, to: u32, eps: f64, smart: bool, what: &str) -> Vec<Sums> {
    let f = feature_count(data());
    let mut sums: Vec<Sums> = (0..ROUNDS).map(|_| Sums::new(f)).collect();
    let t = Instant::now();
    let mut at = from;
    while at < to {
        let end = (at + BATCH).min(to);
        let games: Vec<Game> = (at..end).into_par_iter().map(|seed| play(seed, eps, smart)).collect();
        sums.par_iter_mut().enumerate().for_each(|(r, sum)| {
            let mut x = Vec::new();
            for g in &games {
                for b in g.boards.iter().filter(|b| b.round.clamp(1, ROUNDS as i32) as usize == r + 1) {
                    features(data(), b, &mut x);
                    sum.add(f, &x, g.y);
                }
            }
        });
        at = end;
        let done = (at - from) as f64;
        let rate = done / t.elapsed().as_secs_f64();
        eprint!("\r{what}: {} of {} games ({rate:.0}/s, {:.0} s left)   ", at - from, to - from, (to - at) as f64 / rate);
    }
    eprintln!();
    sums
}

/// Every board of games `from..to` as raw numbers for decision trees (tree_features, then how its game ended), as
/// little-endian float32 rows in `path`, and `path`.json saying how many columns and rows.
pub fn rows(from: u32, to: u32, eps: f64, smart: bool, path: &str) {
    use std::io::Write;
    let d = data();
    let file = std::fs::File::create(path).unwrap_or_else(|e| crate::fail(&format!("{path}: {e}")));
    let mut out = std::io::BufWriter::with_capacity(1 << 22, file);
    let t = Instant::now();
    let (mut n, mut columns) = (0u64, 0usize);
    let mut at = from;
    while at < to {
        let end = (at + BATCH).min(to);
        let chunks: Vec<Vec<u8>> = (at..end)
            .into_par_iter()
            .map(|seed| {
                let g = play(seed, eps, smart);
                let mut bytes = Vec::new();
                for b in &g.boards {
                    for v in tree_features(d, b).into_iter().chain([g.y]) {
                        bytes.extend_from_slice(&(v as f32).to_le_bytes());
                    }
                }
                bytes
            })
            .collect();
        if columns == 0 {
            columns = tree_features(d, &create_game(d.default_settings, None, Some(1))).len() + 1;
        }
        for c in &chunks {
            out.write_all(c).unwrap_or_else(|e| crate::fail(&format!("{path}: {e}")));
            n += (c.len() / 4 / columns) as u64;
        }
        at = end;
        let rate = (at - from) as f64 / t.elapsed().as_secs_f64();
        eprint!("\rboards of {} of {} games ({rate:.0}/s)   ", at - from, to - from);
    }
    eprintln!();
    out.flush().unwrap_or_else(|e| crate::fail(&format!("{path}: {e}")));
    let meta = serde_json::json!({ "columns": columns, "rows": n, "games": to - from, "seed0": from, "eps": eps });
    std::fs::write(format!("{path}.json"), meta.to_string()).unwrap_or_else(|e| crate::fail(&format!("{path}.json: {e}")));
    println!("wrote {path}: {n} boards of {} games, {columns} columns", to - from);
}

/// Solves (A + λI) w = b by Cholesky (λ left off the bias); A symmetric positive semi-definite.
fn solve(s: &Sums, f: usize, lambda: f64) -> Vec<f64> {
    let mut l = vec![0.0; f * f];
    for i in 0..f {
        for j in 0..=i {
            let mut v = s.a(f, i, j) + if i == j && i > 0 { lambda } else { 0.0 } + if i == j { 1e-9 } else { 0.0 };
            for k in 0..j {
                v -= l[i * f + k] * l[j * f + k];
            }
            l[i * f + j] = if i == j { v.max(1e-12).sqrt() } else { v / l[j * f + j] };
        }
    }
    let mut z = vec![0.0; f];
    for i in 0..f {
        let mut v = s.xty[i];
        for k in 0..i {
            v -= l[i * f + k] * z[k];
        }
        z[i] = v / l[i * f + i];
    }
    let mut w = vec![0.0; f];
    for i in (0..f).rev() {
        let mut v = z[i];
        for k in i + 1..f {
            v -= l[k * f + i] * w[k];
        }
        w[i] = v / l[i * f + i];
    }
    w
}

/// Squared error of weights `w` on a set's sums: yᵀy − 2wᵀXᵀy + wᵀXᵀXw.
fn sse(s: &Sums, f: usize, w: &[f64]) -> f64 {
    let (mut quad, mut lin) = (0.0, 0.0);
    for i in 0..f {
        lin += w[i] * s.xty[i];
        let mut row = 0.0;
        for j in 0..f {
            row += s.a(f, i, j) * w[j];
        }
        quad += w[i] * row;
    }
    s.yy - 2.0 * lin + quad
}

/// Six significant digits, as the shipped model keeps them.
fn sig6(v: f64) -> f64 {
    format!("{v:.5e}").parse().unwrap_or(v)
}

/// The weights for each round: per round, the ridge fit (of six strengths) that predicts the check games best.
fn fit(train: &[Sums], check: &[Sums]) -> Vec<Vec<f64>> {
    let f = feature_count(data());
    let fitted: Vec<(Vec<f64>, String)> = (0..ROUNDS)
        .into_par_iter()
        .map(|r| {
            let (t, c) = (&train[r], &check[r]);
            if t.n == 0.0 {
                return (vec![0.0; f], format!("round {:>2}: no boards", r + 1));
            }
            let mut best: Option<(f64, Vec<f64>, f64)> = None;
            for k in [1e-3, 1e-2, 1e-1, 1.0, 10.0, 100.0] {
                let lambda = k * t.n * 1e-4;
                let w = solve(t, f, lambda);
                let err = sse(c, f, &w);
                if best.as_ref().is_none_or(|b| err < b.2) {
                    best = Some((lambda, w, err));
                }
            }
            let (lambda, w, err) = best.unwrap();
            let sst = c.yy - c.xty[0].powi(2) / c.n.max(1.0);
            let line = format!(
                "round {:>2}: {} boards to learn from, {} to check | typical miss {:.0} | explains {:.1}% (spread {:.0}) | ridge {lambda:.2e}",
                r + 1,
                t.n,
                c.n,
                (err / c.n).sqrt() * 1e6,
                100.0 * (1.0 - err / sst),
                (sst / c.n).sqrt() * 1e6
            );
            (w.into_iter().map(sig6).collect(), line)
        })
        .collect();
    for (_, line) in &fitted {
        println!("{line}");
    }
    fitted.into_iter().map(|(w, _)| w).collect()
}

/// `vorax train`: plays the games, fits the weights and writes the model.
pub fn train(games: u32, check: u32, seed0: u32, eps: f64, smart: bool, out: &str) {
    let t = Instant::now();
    let train = gather(seed0, seed0 + games, eps, smart, "training games");
    let held = gather(seed0 + games, seed0 + games + check, eps, smart, "check games");
    let rounds = fit(&train, &held);
    let json = serde_json::json!({ "rounds": rounds });
    std::fs::write(out, serde_json::to_string(&json).unwrap()).unwrap_or_else(|e| crate::fail(&format!("{out}: {e}")));
    println!("wrote {out} ({games} games to learn from, {check} to check, {:.0} s)", t.elapsed().as_secs_f64());
}

/// Plays `games` games from `seed0` with `chooser` and `picker` on every core, with progress: each seed's score.
pub fn scores(games: u32, seed0: u32, chooser: &Chooser, picker: &Picker) -> Vec<(u32, i64)> {
    let done = AtomicUsize::new(0);
    let t = Instant::now();
    let step = (games as usize / 100).max(1);
    let out = (seed0..seed0 + games)
        .into_par_iter()
        .map(|seed| {
            let score = ultimate_activity(&run_game(seed, chooser, picker, None));
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n % step == 0 {
                let rate = n as f64 / t.elapsed().as_secs_f64();
                eprint!("\r{n} of {games} games ({rate:.1}/s, {:.0} s left)   ", (games as usize - n) as f64 / rate);
            }
            (seed, score)
        })
        .collect();
    eprintln!();
    out
}

/// At least this many more games must reach TARGET (net), or 1% of the games if that's more.
const MARGIN: usize = 5;
/// The average counted up to TARGET may be at most this much lower.
const SLACK: f64 = 5_000.0;

/// Chance that a split at least this uneven of the games one reached and the other didn't comes by luck.
fn luck(gained: usize, lost: usize) -> f64 {
    let (n, k) = (gained + lost, gained.min(lost));
    if n == 0 {
        return 1.0;
    }
    // in logs: thousands of games overflow 2ⁿ and the binomial coefficients
    let (mut tail, mut log_c) = (0.0f64, 0.0f64);
    let half = n as f64 * std::f64::consts::LN_2;
    for i in 0..=k {
        tail += (log_c - half).exp();
        log_c += ((n - i) as f64).ln() - ((i + 1) as f64).ln();
    }
    (2.0 * tail).min(1.0)
}

fn read_scores(path: &str) -> std::collections::HashMap<u32, i64> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| crate::fail(&format!("{path}: {e}")));
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        })
        .collect()
}

/// `vorax gate`: a new model's games (the challenger) against the one in use (the champion), seed by seed.
pub fn gate(champion: &str, challenger: &str) {
    let (a, b) = (read_scores(champion), read_scores(challenger));
    let mut both: Vec<(i64, i64)> = b.iter().filter_map(|(seed, &sb)| a.get(seed).map(|&sa| (sa, sb))).collect();
    both.sort();
    let n = both.len();
    let cap = |x: i64| x.min(TARGET) as f64;
    let reached = |pick: fn(&(i64, i64)) -> i64| both.iter().filter(|g| pick(g) >= TARGET).count();
    let average = |pick: fn(&(i64, i64)) -> i64| both.iter().map(|g| cap(pick(g))).sum::<f64>() / n.max(1) as f64;
    let (ra, rb) = (reached(|g| g.0), reached(|g| g.1));
    let (aa, ab) = (average(|g| g.0), average(|g| g.1));
    let gained = both.iter().filter(|g| g.1 >= TARGET && g.0 < TARGET).count();
    let lost = both.iter().filter(|g| g.0 >= TARGET && g.1 < TARGET).count();
    let pass = n > 0 && gained as i64 - lost as i64 >= MARGIN.max(n.div_ceil(100)) as i64 && ab >= aa - SLACK;
    let pct = |x: usize| 100.0 * x as f64 / n.max(1) as f64;
    let k = TARGET / 1000;
    println!("{n} games, the same seeds for both   champion  {champion}   challenger  {challenger}");
    println!("reached {k}k:              {ra} ({:.1}%)   {rb} ({:.1}%)", pct(ra), pct(rb));
    println!("average counted to {k}k:   {aa:.0}   {ab:.0}");
    println!("the challenger reached {k}k in {gained} games the champion didn't, and missed {lost} it reached (chance of a split like this by luck: {:.1}%)", 100.0 * luck(gained, lost));
    println!("pass={}", if pass { "yes" } else { "no" });
}

/// A sequential test (Wald's SPRT, as chess engines test their changes): the challenger plays the champion's seeds a
/// batch at a time, and after each batch the evidence so far is weighed (the games it reached TARGET in and the
/// champion didn't, against the other way round); it stops once that's clear either way, so a clear result takes a
/// fraction of the games. H0: each such game goes either way alike; H1: the challenger takes a share `p1` of them
/// (0.55: about 2 more games in a hundred reaching TARGET). Errors at most 5% each way.
pub fn duel(champion: &str, chooser: &Chooser, picker: &Picker, max_games: u32, p1: f64, out: Option<&str>) {
    let champ = read_scores(champion);
    let mut seeds: Vec<u32> = champ.keys().copied().collect();
    seeds.sort();
    seeds.truncate(max_games as usize);
    let (alpha, beta) = (0.05f64, 0.05f64);
    let (upper, lower) = (((1.0 - beta) / alpha).ln(), (beta / (1.0 - alpha)).ln());
    let (win, loss) = ((p1 / 0.5).ln(), ((1.0 - p1) / 0.5).ln());
    let (mut gained, mut lost, mut played) = (0u32, 0u32, Vec::new());
    let t = Instant::now();
    let batch = (rayon::current_num_threads() * 5).max(20);
    let mut verdict = "no decision within the games given";
    for chunk in seeds.chunks(batch) {
        let scores: Vec<(u32, i64)> = chunk.par_iter().map(|&seed| (seed, ultimate_activity(&run_game(seed, chooser, picker, None)))).collect();
        for &(seed, score) in &scores {
            let c = champ[&seed];
            gained += (score >= TARGET && c < TARGET) as u32;
            lost += (c >= TARGET && score < TARGET) as u32;
        }
        played.extend(scores);
        let llr = gained as f64 * win + lost as f64 * loss;
        eprint!("\r{} games ({:.0} s): {gained} gained, {lost} lost, evidence {llr:+.2} (better at {upper:+.2}, not better at {lower:+.2})   ", played.len(), t.elapsed().as_secs_f64());
        if llr >= upper {
            verdict = "BETTER";
            break;
        }
        if llr <= lower {
            verdict = "not better";
            break;
        }
    }
    eprintln!();
    let n = played.len();
    let reached = |f: &dyn Fn(&(u32, i64)) -> i64| played.iter().filter(|g| f(g) >= TARGET).count();
    let (rc, rn) = (reached(&|g| champ[&g.0]), reached(&|g| g.1));
    let pct = |x: usize| 100.0 * x as f64 / n.max(1) as f64;
    println!("{n} games: champion {rc} ({:.1}%), challenger {rn} ({:.1}%) reached {}k; {gained} gained, {lost} lost", pct(rc), pct(rn), TARGET / 1000);
    println!("verdict: {verdict} (sequential test, p1 {p1}, 5% error each way) after {:.0} s", t.elapsed().as_secs_f64());
    if let Some(path) = out {
        let text: String = played.iter().map(|(seed, score)| format!("{seed} {score}\n")).collect();
        std::fs::write(path, text).unwrap_or_else(|e| crate::fail(&format!("{path}: {e}")));
    }
}
