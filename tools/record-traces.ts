// Records reference traces from upstream's TypeScript engine, bot and advisor, for the Rust parity tests.
// Run from upstream/:  npx vite-node ../tools/record-traces.ts ../fixtures [games]
//   engine.jsonl    random games (normal and sandbox starts): every action and the full state after it
//   strategy.jsonl  sampled boards: options, expected values, choices, shortlists, futures, pickers
//   advisor.jsonl   screens: the Solver's advice (look-ahead, all futures) and the quick advice
//   games.jsonl     whole games played by the smarter bot, start to end
import { mkdirSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { canConfirm, canRefresh, cardChanges, createGame, dispatch, DEFAULT_SETTINGS, needsTargets } from '@/engine/game'
import { createRng } from '@/engine/rng'
import type { Action, GameState, SandboxSetup, Settings } from '@/engine/types'
import { INSTRUMENTS, instrument } from '@/data/instruments'
import { ARCHETYPES, RARITIES, createMonster } from '@/data/archetypes'
import { potion } from '@/data/potions'
import { boardValue } from '@/strategy/value'
import { buildProgress } from '@/strategy/steer'
import { choose, evChooser, expected, instrumentFutures, options, potionFutures, runGame, shortlist, valuePicker } from '@/strategy/bot'
import { advise, LOOK_AHEAD, type ScreenState } from '@/strategy/advisor'

const out = resolve(process.argv[2] ?? '../fixtures')
const GAMES = Number(process.argv[3] ?? 300)
mkdirSync(out, { recursive: true })

const snap = (s: GameState) => ({
  tanks: s.tanks.map(t => (t ? [t.archetype, t.rarity, t.activity, t.quantity] : null)),
  instruments: s.instruments.map(i => [i.id, i.acquiredRound]),
  round: s.round,
  phase: s.phase,
  hand: s.hand.map(c => [c.kind, c.id]),
  openBox: s.openBox,
  selected: s.selected,
  targets: s.targets,
  potionRefreshes: s.potionRefreshes,
  instrumentRefreshes: s.instrumentRefreshes,
  thresholds: [s.thresholds.t8000, s.thresholds.t28000],
  pendingPicks: s.pendingPicks,
  rngState: s.rngState,
  boardValue: boardValue(s),
  buildProgress: buildProgress(s),
  needsTargets: needsTargets(s),
  canConfirm: canConfirm(s),
  canRefresh: canRefresh(s),
})

/** A sandbox start: random instruments (some acquired later, so periodic ones fire on other rounds), round, refreshes. */
function randomSetup(r: ReturnType<typeof createRng>): { settings: Settings; setup: SandboxSetup } {
  const n = 1 + r.int(3)
  const ids = r.sample(INSTRUMENTS.map(d => d.id), n)
  const startRound = 1 + r.int(10)
  const tanks = r.chance(0.5)
    ? null
    : Array.from({ length: 6 }, () => (r.chance(0.7) ? createMonster(ARCHETYPES[r.int(4)], RARITIES[r.int(4)], r.int(300), r.int(300)) : null))
  return {
    settings: { potionRefreshes: r.int(4), instrumentRefreshes: r.int(3) },
    setup: {
      tanks,
      instruments: ids.map(id => ({ id, acquiredRound: 1 + r.int(startRound) })),
      startRound,
      thresholdsUsed: { t8000: r.chance(0.3), t28000: r.chance(0.2) },
      forcedHand: r.chance(0.2) ? r.sample([...Array(42).keys()], 3) : [],
      seed: r.int(2 ** 31),
    },
  }
}

// ---------------------------------------------------------------------------- engine
const engineLines: string[] = []
const sampled: GameState[] = []
for (let g = 1; g <= GAMES; g++) {
  const choose = createRng(g * 7919 + 13)
  const sandbox = g % 2 === 0
  const { settings, setup } = sandbox ? randomSetup(choose) : { settings: DEFAULT_SETTINGS, setup: undefined }
  const seed = sandbox ? undefined : g
  let s = createGame(settings, setup, seed)
  const actions: Action[] = []
  const states = [snap(s)]
  const step = (a: Action) => {
    s = dispatch(s, a).state
    actions.push(a)
    states.push(snap(s))
  }
  let guard = 0
  while (s.phase !== 'gameOver' && guard++ < 400) {
    if ((s.phase === 'potionSelect' || s.phase === 'instrumentSelect') && choose.chance(0.1)) sampled.push({ ...s, log: [] })
    if (choose.chance(0.08)) step({ type: 'refresh' })
    step({ type: 'selectCard', index: choose.int(Math.max(1, s.hand.length + (choose.chance(0.05) ? 1 : 0))) })
    let k = 0
    while (needsTargets(s) && k++ < 6) {
      step({ type: 'toggleTarget', tank: choose.int(6) })
      if (canConfirm(s) && choose.chance(0.6)) break
    }
    step({ type: 'confirm' })
  }
  engineLines.push(JSON.stringify({ game: g, seed: seed ?? null, settings, setup: setup ?? null, actions, states }))
}
writeFileSync(join(out, 'engine.jsonl'), engineLines.join('\n') + '\n')
console.log(`engine: ${GAMES} games, ${engineLines.reduce((n, l) => n + JSON.parse(l).actions.length, 0)} actions; ${sampled.length} boards sampled`)

// ---------------------------------------------------------------------------- strategy
const SEED = 0x5eed
const pick = (n: number) => sampled.filter((_, i) => i % Math.max(1, Math.floor(sampled.length / n)) === 0).slice(0, n)
const stratLines: string[] = []
for (const [i, s] of pick(60).entries()) {
  const seed = (SEED + i * 31) >>> 0
  const base = { state: snapFull(s), seed }
  if (s.phase === 'potionSelect') {
    const opts = options(s)
    const quick = opts.map(o => expected(s, o, { samples: 8, horizon: 0 }, seed))
    const deep = opts.slice(0, 3).map(o => expected(s, o, { samples: 2, horizon: 2, rollSamples: 2, steer: 0.3 }, seed))
    const valued = opts.slice(0, 3).map(o => expected(s, o, { samples: 2, horizon: 1, value: true }, seed))
    const changes = opts.map(o => cardChanges(s, (o[0] as { index: number }).index, o.flatMap(a => (a.type === 'toggleTarget' ? [a.tank] : []))))
    const c1 = choose(s, { samples: 2, horizon: 0, value: true }, seed)
    const c2 = choose(s, { samples: 4, horizon: 0, steer: 0.3 }, seed)
    const list = shortlist(s, seed, LOOK_AHEAD)
    const fut = potionFutures(s, seed, LOOK_AHEAD, list.map(c => c.o), 0, 2)
    const futSteer = potionFutures(s, seed, { top: 4, playouts: 2, steer: 0.3 }, list.slice(0, 2).map(c => c.o), 3, 4)
    stratLines.push(JSON.stringify({ ...base, kind: 'potion', options: opts, quick, deep, valued, changes, choose: [c1, c2], shortlist: list, futures: fut, futuresSteer: futSteer }))
  } else {
    const choices = [...s.hand.map((_, k) => k), ...(canRefresh(s) ? ['refresh' as const] : [])]
    const fut = instrumentFutures(s, seed, choices, evChooser({ samples: 4, horizon: 0, value: true }), 'target', 0, 2)
    const futPlain = instrumentFutures(s, seed, choices, evChooser({ samples: 2, horizon: 0 }), 'target', 1, 3)
    const futMean = instrumentFutures(s, seed, choices, evChooser({ samples: 2, horizon: 0 }), 'mean', 0, 1)
    stratLines.push(JSON.stringify({ ...base, kind: 'instrument', choices, futures: fut, futuresPlain: futPlain, futuresMean: futMean, valuePicker: valuePicker(s, seed) }))
  }
}
writeFileSync(join(out, 'strategy.jsonl'), stratLines.join('\n') + '\n')
console.log(`strategy: ${stratLines.length} boards`)

/** The whole state (rngState, sandbox) so the Rust side can rebuild it exactly. */
function snapFull(s: GameState) {
  return { ...snap(s), sandbox: s.sandbox ? { startRound: s.sandbox.startRound, forcedHand: s.sandbox.forcedHand } : null }
}

// ---------------------------------------------------------------------------- advisor
const toScreen = (s: GameState): ScreenState => ({
  tanks: s.tanks.map(t => (t ? { ...t } : null)),
  hand: s.hand.map(c => (c.kind === 'potion' ? potion(c.id).name : instrument(c.id)!.name)),
  instruments: s.instruments.map(i => instrument(i.id)!.name),
  instrumentRounds: s.instruments.map(i => i.acquiredRound),
  phase: s.phase === 'potionSelect' ? 'potion' : 'instrument',
  round: s.round,
  potionRefreshes: s.potionRefreshes,
  instrumentRefreshes: s.instrumentRefreshes,
})
const advLines: string[] = []
const ADVISED = Number(process.env.ADVISED ?? 10)
const t0 = Date.now()
for (const s of pick(ADVISED * 3).filter((x, i) => x.openBox === null && i % 3 === 0).slice(0, ADVISED)) {
  const screen = toScreen(s)
  const t = Date.now()
  const plan = advise(screen, 16, true)
  const ms = Date.now() - t
  const quick = screen.phase === 'potion' ? advise(screen, 16, false) : null
  const strip = (a: ReturnType<typeof advise> | null) => (a ? { ...a, ms: 0 } : null)
  advLines.push(JSON.stringify({ screen, plan: strip(plan), quick: strip(quick), jsMs: ms }))
}
writeFileSync(join(out, 'advisor.jsonl'), advLines.join('\n') + '\n')
console.log(`advisor: ${advLines.length} screens in ${((Date.now() - t0) / 1000).toFixed(1)} s`)

// ---------------------------------------------------------------------------- whole games
const gameLines: string[] = []
const tg = Date.now()
for (let seed = 1; seed <= 4; seed++) {
  const end = runGame(seed, evChooser({ samples: 2, horizon: 0, value: true }), valuePicker)
  gameLines.push(JSON.stringify({ seed, chooser: 'value', final: snap(end) }))
  const end2 = runGame(seed, evChooser({ samples: 4, horizon: 0, steer: 0.3 }), valuePicker)
  gameLines.push(JSON.stringify({ seed, chooser: 'steer', final: snap(end2) }))
}
writeFileSync(join(out, 'games.jsonl'), gameLines.join('\n') + '\n')
console.log(`games: ${gameLines.length} in ${((Date.now() - tg) / 1000).toFixed(1)} s`)
