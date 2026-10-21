// Exports everything the Rust port reads from upstream as plain JSON (Rust can't read TypeScript).
// Run from upstream/ so its `@/` alias resolves:  npx vite-node ../tools/export-data.ts ../data
// Card and instrument *effects* are code and are ported by hand (crates/vorax/src/effects.rs); this file carries
// their ids, names and timing so the Rust side can refuse to run when upstream adds or renames one.
import { writeFileSync, mkdirSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { execSync } from 'node:child_process'
import { POTIONS } from '@/data/potions'
import { INSTRUMENTS } from '@/data/instruments'
import { LIVE_WEIGHTS } from '@/data/liveWeights'
import STRENGTH from '@/data/instrumentStrength.json'
import BOARD_VALUE from '@/data/boardValue.json'
import { DEFAULT_SETTINGS } from '@/engine/game'

const out = resolve(process.argv[2] ?? '../data')
mkdirSync(out, { recursive: true })

const game = {
  // the upstream commit this was exported from: the overlay's updater compares rule files against it
  commit: execSync('git rev-parse HEAD').toString().trim(),
  odds: LIVE_WEIGHTS.updated,
  supremeChance: LIVE_WEIGHTS.supremeChance,
  defaultSettings: DEFAULT_SETTINGS,
  potions: POTIONS.map(p => ({ id: p.id, name: p.name, rarity: p.rarity, targeting: p.targeting, hasEffect: !!p.effect })),
  instruments: INSTRUMENTS.map(d => ({ id: d.id, name: d.name, pool: d.pool, timing: d.timing })),
  potionOffer: LIVE_WEIGHTS.potionOffer,
  boxOffer: LIVE_WEIGHTS.boxOffer,
  instrumentOffer: LIVE_WEIGHTS.instrumentOffer,
  spawnRarity: LIVE_WEIGHTS.spawnRarity,
  strength: STRENGTH.strength,
}

writeFileSync(join(out, 'game.json'), JSON.stringify(game, null, 2) + '\n')
writeFileSync(join(out, 'boardValue.json'), JSON.stringify(BOARD_VALUE))
console.log(`Exported ${POTIONS.length} potions, ${INSTRUMENTS.length} instruments, odds ${LIVE_WEIGHTS.updated} to ${out}`)
