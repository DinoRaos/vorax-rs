// Exports the screen reader's reference data from upstream for the Rust reader (crates/vorax-scan).
// Run from upstream/:  npx vite-node ../tools/export-scan.ts ../data/scan
import { copyFileSync, mkdirSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { POTIONS } from '@/data/potions'
import { INSTRUMENTS } from '@/data/instruments'
import { PROTOCOLS } from '@/data/protocols'
import { ALWAYS_TAKE } from '@/data/xenopanacea'

const out = resolve(process.argv[2] ?? '../data/scan')
mkdirSync(out, { recursive: true })
for (const f of ['data.json', 'learnedIcons.json', 'levels.json']) copyFileSync(join('src/solver/scan', f), join(out, f))
// the names the reader matches printed titles against, per kind of pick (titles.ts NAMES)
writeFileSync(join(out, 'names.json'), JSON.stringify({ potion: [...POTIONS.map(p => p.name), ...ALWAYS_TAKE], instrument: INSTRUMENTS.map(i => i.name), protocol: [...PROTOCOLS] }, null, 1))
console.log(`Exported the reader's data to ${out}`)
