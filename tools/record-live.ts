// Records upstream's log settler on its fixture log (tests/fixtures/vorax-log-excerpt.log), replayed with each
// line fed at its own timestamp and the settler checked every 50 ms, for the Rust port (fixtures/live.json).
// Run from upstream/:  npx vite-node ../tools/record-live.ts ../fixtures
import { readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { classify, createSettler, logTime } from '@/solver/live/triggers'

const out = resolve(process.argv[2] ?? '../fixtures')
const lines = readFileSync('tests/fixtures/vorax-log-excerpt.log', 'utf8').split('\n').map(l => (l.endsWith('\r') ? l.slice(0, -1) : l)).filter(Boolean)
const s = createSettler()
const snaps: { kind: string; at: number }[] = []
const kinds = lines.map(l => classify(l))
let now = logTime(lines[0]) ?? 0
let i = 0
const end = (logTime(lines[lines.length - 1]) ?? now) + 4000
while (now <= end) {
  const batch: string[] = []
  while (i < lines.length && (logTime(lines[i]) ?? now) <= now) batch.push(lines[i++])
  if (batch.length) s.feed(batch, now)
  const due = s.due(now)
  if (due) snaps.push({ kind: due, at: now })
  now += 50
}
writeFileSync(join(out, 'live.json'), JSON.stringify({ kinds, snaps, refreshes: s.refreshes, over: s.over }))
console.log(`live: ${snaps.length} snaps, ${s.refreshes} refreshes, over ${s.over}`)
