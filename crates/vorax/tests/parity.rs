//! Replays the traces recorded from tli-vorax-simulator's TypeScript (its last open version, 7b4c9ba) in
//! fixtures/ and checks that every state, value, choice and piece of advice comes out the same, so a change can't
//! make the engine play differently by accident; a failure names the first step that plays differently.

use serde_json::{json, Value};
use std::path::PathBuf;
use vorax::advisor::{self, ScreenState};
use vorax::bot::*;
use vorax::game::*;
use vorax::stack::StackVec;
use vorax::types::*;
use vorax::value::{board_value, build_progress};

/// The recorded traces were made with tli-vorax-simulator's linear board value (7b4c9ba), not the trees the program
/// ships: every test plays with that one.
fn linear_board_value() {
    let _ = vorax::data::use_model(Some(include_str!("../../../fixtures/boardValue-linear.json")));
    assert!(matches!(vorax::data().model, Some(vorax::data::ValueModel::Rounds { .. })), "the tests need the linear board value");
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn lines(name: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(fixtures().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn arch(v: &Value) -> Archetype {
    serde_json::from_value(v.clone()).unwrap()
}
fn rarity(v: &Value) -> Rarity {
    serde_json::from_value(v.clone()).unwrap()
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::InstrumentSelect => "instrumentSelect",
        Phase::PotionSelect => "potionSelect",
        Phase::RoundEnd => "roundEnd",
        Phase::GameOver => "gameOver",
    }
}

/// The same shape as the traces' states.
fn snap(s: &GameState) -> Value {
    let a = |a: Archetype| serde_json::to_value(a).unwrap();
    let r = |r: Rarity| serde_json::to_value(r).unwrap();
    json!({
        "tanks": s.tanks.iter().map(|t| t.map(|m| json!([a(m.archetype), r(m.rarity), m.activity, m.quantity]))).collect::<Vec<_>>(),
        "instruments": s.instruments.iter().map(|e| json!([e.id, e.acquired_round])).collect::<Vec<_>>(),
        "round": s.round,
        "phase": phase_name(s.phase),
        "hand": s.hand.iter().map(|c| json!([if c.kind == CardKind::Potion { "potion" } else { "instrument" }, c.id])).collect::<Vec<_>>(),
        "openBox": s.open_box,
        "selected": s.selected,
        "targets": s.targets.as_slice(),
        "potionRefreshes": s.potion_refreshes,
        "instrumentRefreshes": s.instrument_refreshes,
        "thresholds": [s.t8000, s.t28000],
        "pendingPicks": s.pending_picks,
        "rngState": s.rng_state,
        "boardValue": board_value(s),
        "buildProgress": build_progress(s),
        "needsTargets": needs_targets(s).map(|t| match t { vorax::data::Targeting::Select1 => "select1", _ => "select1to2" }),
        "canConfirm": can_confirm(s),
        "canRefresh": can_refresh(s),
    })
}

/// Rebuilds a state from snap() (+ sandbox), as recorded in strategy.jsonl.
fn state_from(v: &Value) -> GameState {
    let tanks: Vec<Option<Monster>> = v["tanks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_array().map(|m| Monster { archetype: arch(&m[0]), rarity: rarity(&m[1]), activity: m[2].as_i64().unwrap(), quantity: m[3].as_i64().unwrap() }))
        .collect();
    let phase = match v["phase"].as_str().unwrap() {
        "instrumentSelect" => Phase::InstrumentSelect,
        "potionSelect" => Phase::PotionSelect,
        "roundEnd" => Phase::RoundEnd,
        _ => Phase::GameOver,
    };
    let mut hand = Hand::new();
    for c in v["hand"].as_array().unwrap() {
        hand.push(Card { kind: if c[0] == "potion" { CardKind::Potion } else { CardKind::Instrument }, id: c[1].as_u64().unwrap() as u8 });
    }
    let mut instruments = Equips::new();
    for e in v["instruments"].as_array().unwrap() {
        instruments.push(Equipped { id: e[0].as_u64().unwrap() as u8, acquired_round: e[1].as_i64().unwrap() as i32 });
    }
    let sandbox = v.get("sandbox").and_then(|s| s.as_object()).map(|s| Sandbox {
        start_round: s["startRound"].as_i64().unwrap() as i32,
        forced_hand: StackVec::from_slice(&s["forcedHand"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect::<Vec<_>>()),
    });
    GameState {
        tanks: tanks.try_into().unwrap(),
        instruments,
        round: v["round"].as_i64().unwrap() as i32,
        phase,
        hand,
        open_box: v["openBox"].as_u64().map(|x| x as u8),
        selected: v["selected"].as_u64().map(|x| x as u8),
        targets: StackVec::from_slice(&v["targets"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect::<Vec<_>>()),
        potion_refreshes: v["potionRefreshes"].as_i64().unwrap() as i32,
        instrument_refreshes: v["instrumentRefreshes"].as_i64().unwrap() as i32,
        t8000: v["thresholds"][0].as_bool().unwrap(),
        t28000: v["thresholds"][1].as_bool().unwrap(),
        pending_picks: v["pendingPicks"].as_i64().unwrap() as i32,
        rng_state: v["rngState"].as_u64().unwrap() as u32,
        sandbox,
    }
}

/// JSON equality with numbers compared as f64 (JS writes 1, Rust 1.0), exactly.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same(p, q))),
        _ => a == b,
    }
}

fn diff(a: &Value, b: &Value) -> String {
    if let (Value::Object(x), Value::Object(y)) = (a, b) {
        let mut keys: Vec<String> = x.keys().chain(y.keys()).filter(|k| !same(x.get(*k).unwrap_or(&Value::Null), y.get(*k).unwrap_or(&Value::Null))).cloned().collect();
        keys.sort();
        keys.dedup();
        return keys.iter().map(|k| format!("{k}: rust {} / ts {}", x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null))).collect::<Vec<_>>().join("\n    ");
    }
    format!("rust {a}\n    ts   {b}")
}

fn setup_from(v: &Value) -> Option<SandboxSetup> {
    let s = v.as_object()?;
    let tanks = s["tanks"].as_array().map(|ts| {
        let v: Vec<Option<Monster>> = ts
            .iter()
            .map(|t| t.as_object().map(|m| Monster { archetype: arch(&m["archetype"]), rarity: rarity(&m["rarity"]), activity: m["activity"].as_i64().unwrap(), quantity: m["quantity"].as_i64().unwrap() }))
            .collect();
        v.try_into().unwrap()
    });
    Some(SandboxSetup {
        tanks,
        instruments: s["instruments"].as_array().unwrap().iter().map(|e| Equipped { id: e["id"].as_u64().unwrap() as u8, acquired_round: e["acquiredRound"].as_i64().unwrap() as i32 }).collect(),
        start_round: s["startRound"].as_i64().unwrap() as i32,
        thresholds_used: (s["thresholdsUsed"]["t8000"].as_bool().unwrap(), s["thresholdsUsed"]["t28000"].as_bool().unwrap()),
        forced_hand: s["forcedHand"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect(),
        seed: s.get("seed").and_then(|x| x.as_u64()).map(|x| x as u32),
    })
}

#[test]
fn engine_traces_match() {
    linear_board_value();
    let mut steps = 0;
    for g in lines("engine.jsonl") {
        let settings = Settings { potion_refreshes: g["settings"]["potionRefreshes"].as_i64().unwrap() as i32, instrument_refreshes: g["settings"]["instrumentRefreshes"].as_i64().unwrap() as i32 };
        let setup = setup_from(&g["setup"]);
        let mut s = create_game(settings, setup.as_ref(), g["seed"].as_u64().map(|x| x as u32));
        let states = g["states"].as_array().unwrap();
        let actions: Vec<Action> = serde_json::from_value(g["actions"].clone()).unwrap();
        let check = |s: &GameState, i: usize, what: &str| {
            let (mine, theirs) = (snap(s), &states[i]);
            assert!(same(&mine, theirs), "game {} {what}:\n    {}", g["game"], diff(&mine, theirs));
        };
        check(&s, 0, "start");
        for (i, a) in actions.iter().enumerate() {
            s = dispatch(&s, *a);
            check(&s, i + 1, &format!("step {} ({a:?})", i + 1));
            steps += 1;
        }
    }
    eprintln!("engine: {steps} steps match");
}

fn ts_opt(v: &Value) -> Opt {
    let acts: Vec<Action> = serde_json::from_value(v.clone()).unwrap();
    match acts[0] {
        Action::Refresh => Opt::Refresh,
        Action::SelectCard { index } => {
            let t: Vec<u8> = acts.iter().filter_map(|a| if let Action::ToggleTarget { tank } = a { Some(*tank as u8) } else { None }).collect();
            Opt::card(index as usize, &t)
        }
        _ => panic!("odd option {v}"),
    }
}

fn tallies(t: &[Tally]) -> Value {
    json!(t.iter().map(|t| json!({"goal": t.goal, "reached": t.reached, "n": t.n})).collect::<Vec<_>>())
}

fn pick_json(p: Pick) -> Value {
    match p {
        Pick::Card(i) => json!(i),
        Pick::Refresh => json!("refresh"),
    }
}

#[test]
fn strategy_matches() {
    linear_board_value();
    let la = advisor::LOOK_AHEAD;
    for (n, c) in lines("strategy.jsonl").iter().enumerate() {
        let s = state_from(&c["state"]);
        let seed = c["seed"].as_u64().unwrap() as u32;
        let eq = |what: &str, mine: Value| assert!(same(&mine, &c[what]), "board {n} {what}:\n    {}", diff(&mine, &c[what]));
        if c["kind"] == "potion" {
            let opts = options(&s);
            let theirs: Vec<Opt> = c["options"].as_array().unwrap().iter().map(ts_opt).collect();
            assert_eq!(opts.as_slice(), &theirs[..], "board {n} options");
            eq("quick", json!(opts.iter().map(|o| expected(&s, o, &Policy::new(8, 0), seed, 0)).collect::<Vec<_>>()));
            let deep = Policy { samples: 2, horizon: 2, roll_samples: Some(2), steer: Some(0.3), value: None };
            eq("deep", json!(opts.iter().take(3).map(|o| expected(&s, o, &deep, seed, 0)).collect::<Vec<_>>()));
            let valued = Policy { samples: 2, horizon: 1, roll_samples: None, steer: None, value: Some(true) };
            eq("valued", json!(opts.iter().take(3).map(|o| expected(&s, o, &valued, seed, 0)).collect::<Vec<_>>()));
            eq("changes", json!(opts.iter().map(|o| match o { Opt::Card { index, targets } => card_changes(&s, *index as usize, targets.as_slice()), _ => true }).collect::<Vec<_>>()));
            let p1 = Policy { samples: 2, horizon: 0, roll_samples: None, steer: None, value: Some(true) };
            let p2 = Policy { samples: 4, horizon: 0, roll_samples: None, steer: Some(0.3), value: None };
            for (k, p) in [p1, p2].iter().enumerate() {
                let ch = choose(&s, p, seed);
                let ts = &c["choose"][k];
                assert_eq!(ch.best, Some(ts_opt(&ts["best"])), "board {n} choose {k} best");
                assert!(same(&json!(ch.value), &ts["value"]), "board {n} choose {k} value {} vs {}", ch.value, ts["value"]);
                assert!(same(&json!(ch.stay), ts.get("stay").unwrap_or(&Value::Null)), "board {n} choose {k} stay");
            }
            let list = shortlist(&s, seed, &la);
            let theirs: Vec<(Opt, Value)> = c["shortlist"].as_array().unwrap().iter().map(|x| (ts_opt(&x["o"]), x["v"].clone())).collect();
            assert_eq!(list.len(), theirs.len(), "board {n} shortlist length");
            for ((o, v), (to, tv)) in list.iter().zip(&theirs) {
                assert_eq!(o, to, "board {n} shortlist option");
                assert!(same(&json!(v), tv), "board {n} shortlist value {v} vs {tv}");
            }
            let o: Vec<Opt> = list.iter().map(|x| x.0).collect();
            eq("futures", tallies(&potion_futures(&s, seed, &la, &o, 0, 2, true)));
            let steer = LookAhead { top: 4, playouts: 2, steer: Some(0.3), inner_samples: None, value: None, halving: None };
            eq("futuresSteer", tallies(&potion_futures(&s, seed, &steer, &o[..2.min(o.len())], 3, 4, false)));
        } else {
            let choices: Vec<Pick> = c["choices"].as_array().unwrap().iter().map(|x| if x == "refresh" { Pick::Refresh } else { Pick::Card(x.as_u64().unwrap() as usize) }).collect();
            let value = Chooser::Ev(Policy { samples: 4, horizon: 0, roll_samples: None, steer: None, value: Some(true) });
            eq("futures", tallies(&instrument_futures(&s, seed, &choices, &value, Goal::Target, 0, 2, true)));
            let plain = Chooser::Ev(Policy::new(2, 0));
            eq("futuresPlain", tallies(&instrument_futures(&s, seed, &choices, &plain, Goal::Target, 1, 3, false)));
            eq("futuresMean", tallies(&instrument_futures(&s, seed, &choices, &plain, Goal::Mean, 0, 1, true)));
            eq("valuePicker", pick_json(value_pick(&s)));
        }
    }
}

#[test]
fn advisor_matches() {
    linear_board_value();
    for (n, c) in lines("advisor.jsonl").iter().enumerate() {
        let screen: ScreenState = serde_json::from_value(c["screen"].clone()).unwrap();
        let mut plan = serde_json::to_value(advisor::advise(&screen, 16, true, true)).unwrap();
        plan["ms"] = json!(0);
        assert!(same(&plan, &c["plan"]), "screen {n} plan advice:\n    {}", diff(&plan, &c["plan"]));
        if !c["quick"].is_null() {
            let mut quick = serde_json::to_value(advisor::advise(&screen, 16, false, false)).unwrap();
            quick["ms"] = json!(0);
            assert!(same(&quick, &c["quick"]), "screen {n} quick advice:\n    {}", diff(&quick, &c["quick"]));
        }
    }
}

#[test]
fn whole_games_match() {
    linear_board_value();
    for c in lines("games.jsonl") {
        let seed = c["seed"].as_u64().unwrap() as u32;
        let end = if c["chooser"] == "value" {
            run_game(seed, &Chooser::Ev(Policy { samples: 2, horizon: 0, roll_samples: None, steer: None, value: Some(true) }), &Picker::Value, None)
        } else {
            run_game(seed, &Chooser::Ev(Policy { samples: 4, horizon: 0, roll_samples: None, steer: Some(0.3), value: None }), &Picker::Value, None)
        };
        let mine = snap(&end);
        assert!(same(&mine, &c["final"]), "game seed {seed} {}:\n    {}", c["chooser"], diff(&mine, &c["final"]));
    }
}
