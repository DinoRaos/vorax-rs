// Times upstream's TypeScript advice on the recorded screens (one core, best of N after a warm-up run), to compare
// with `vorax bench`. Run from upstream/:  npx vite-node ../tools/bench-ts.ts ../fixtures [reps]
import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { advise, type ScreenState } from '@/strategy/advisor'

const dir = resolve(process.argv[2] ?? '../fixtures')
const reps = Number(process.argv[3] ?? 2)
const screens = readFileSync(join(dir, 'advisor.jsonl'), 'utf8').trim().split('\n').map(l => JSON.parse(l).screen as ScreenState)
advise(screens[0], 16, true) // warm-up
let total = 0
screens.forEach((x, i) => {
  let best = Infinity
  for (let r = 0; r < reps; r++) {
    const t = performance.now()
    advise(x, 16, true)
    best = Math.min(best, performance.now() - t)
  }
  total += best
  console.log(`${i}\t${x.phase}\t${best.toFixed(0)} ms`)
})
console.log(`total\t\t${total.toFixed(0)} ms (node ${process.version})`)
