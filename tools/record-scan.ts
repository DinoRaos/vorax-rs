// Records upstream's screen reader on its test frames, for the Rust reader's parity tests (fixtures/scan.jsonl).
// Each frame is read as it is, shrunk to 3/4 (another screen size), and through a gamma curve (an HDR-like
// capture the colour correction must undo); with pictures, without (names left to the titles), and with
// stand-in title texts. Run from upstream/:  npx vite-node ../tools/record-scan.ts ../fixtures
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { decodePng } from '../upstream/scripts/scan/png'
import { scan } from '@/solver/scan/scan'
import { readTitles, titleCrop } from '@/solver/scan/titles'
import { readSlots, handAt } from '@/solver/scan/cards'
import { layout } from '@/solver/scan/layout'
import { resize, type Img } from '@/solver/scan/pixels'
import { cropTo, windowedGame } from '@/solver/live/windowCrop'
import { trackInstruments, newTracker } from '@/solver/track'
import raw from '@/solver/scan/data.json'
import { unpack, type PackedScanData } from '@/solver/scan/data'
import type { Reading } from '@/solver/types'

const out = resolve(process.argv[2] ?? '../fixtures')
const data = unpack(raw as unknown as PackedScanData)
const dir = 'tests/fixtures/frames'

/** A gamma curve as a lookup table, saved with the readings so the Rust side applies the very same one. */
const GAMMA = Array.from({ length: 256 }, (_, v) => Math.round(255 * Math.pow(v / 255, 0.75)))
const gamma = (img: Img): Img => {
  const d = new Uint8ClampedArray(img.data.length)
  for (let i = 0; i < d.length; i += 4) {
    d[i] = GAMMA[img.data[i]]
    d[i + 1] = GAMMA[img.data[i + 1]]
    d[i + 2] = GAMMA[img.data[i + 2]]
    d[i + 3] = img.data[i + 3]
  }
  return { width: img.width, height: img.height, data: d }
}
const variants: Record<string, (img: Img) => Img> = {
  original: img => img,
  shrunk: img => resize(img, Math.round(img.width * 0.75), Math.round(img.height * 0.75)),
  gamma,
}

/** A picture's checksum: the sum of every byte, and of every byte times its position (catches shifts). */
const sum = (img: Img) => {
  let a = 0, b = 0
  for (let i = 0; i < img.data.length; i++) {
    a += img.data[i]
    b = (b + img.data[i] * ((i % 9973) + 1)) % 1_000_000_007
  }
  return [img.width, img.height, a, b]
}

const strip = (r: Reading) => JSON.parse(JSON.stringify(r))

/** Stand-in text readers: the names as read, the names cut and misspelt, nothing at all (pictures decide). */
const misspell = (n: string) => n.replace(/a/, 'o').replace(/e(?=[^e]*$)/, 'c').slice(0, Math.max(6, n.length - 4))
const readers: Record<string, (names: string[]) => (i: number) => string> = {
  exact: names => i => names[i] ?? '',
  rough: names => i => (names[i] ? `${misspell(names[i])}\nsome card text below` : ''),
  none: () => () => '',
}

const lines: string[] = []
const t0 = Date.now()
for (const f of readdirSync(dir).filter(f => f.endsWith('.png')).sort()) {
  const frame = decodePng(readFileSync(join(dir, f)))
  lines.push(JSON.stringify({ kind: 'window', file: f, area: windowedGame(frame) }))
  for (const [v, make] of Object.entries(variants)) {
    // a windowed capture is read inside its frame, as the Solver does
    const base = make(frame)
    const vArea = windowedGame(base)
    const img = vArea ? cropTo(base, vArea) : base
    const pictures = scan(img, data, {}, { pictures: true })
    const names = scan(img, data, {}, { pictures: false })
    // the titles and their pictures at the interface scale the board read at, as the Solver reads them (useScan.ts)
    const l = layout(img, names.ui)
    const size = names.handSize ?? (pictures.handRanks?.length || 3)
    const crops = handAt(l, size).map(c => sum(titleCrop(img, c, l.k)))
    const titled: Record<string, Reading> = {}
    for (const [rname, make] of Object.entries(readers)) {
      const textOf = make(pictures.state.hand)
      let i = 0
      const readText = async () => textOf(i++)
      titled[rname] = strip(await readTitles(img, names, readText, async (phase, sz, slots) => {
        const { fields, problems, ranks } = readSlots(img, layout(img, names.ui), sz, phase, slots, data)
        return Object.fromEntries(slots.map(j => [j, { ranks: ranks[j], field: fields[`hand${j}`], unknown: problems.some(p => p.kind === 'unknownArt' && p.field === `hand${j}`) }]))
      }))
    }
    lines.push(JSON.stringify({ kind: 'frame', file: f, variant: v, gamma: v === 'gamma' ? GAMMA : null, size: [img.width, img.height], windowed: vArea, pictures: strip(pictures), names: strip(names), crops, titled }))
    console.log(`${f} ${v}: ${pictures.state.hand.join(', ') || '(no hand)'} · ${pictures.problems.map(p => p.kind).join(',') || 'ok'}`)
  }
}

// the instrument tracker through a made-up game: readings as the reader would give them
const R = (phase: 'potion' | 'instrument', round: number, hand: string[], instruments: string[], ranks: [string, number][][]): Reading =>
  ({ state: { tanks: [null, null, null, null, null, null], hand, instruments, phase, round }, fields: {}, problems: [], instrumentRanks: ranks }) as Reading
const steps: [Reading, string | undefined][] = [
  [R('instrument', 1, ['Incubation Sac', 'Contracted Claws', 'Prismatic Liver'], [], []), 'screen'],
  [R('potion', 1, ['Xeno-Hormone', 'Slough Solution', 'Eggshell Powder'], ['Incubation Sac'], [[['Incubation Sac', 0.95], ['Contracted Claws', 0.6]]]), 'board'],
  [R('potion', 2, ['Xeno-Hormone', 'Slough Solution', 'Eggshell Powder'], ['Incubation Sac'], [[['Incubation Sac', 0.95], ['Contracted Claws', 0.6]]]), 'board'],
  [R('instrument', 3, ['Sinew Garrote', 'Fouled Scraper', 'Nettle Toggle'], ['Incubation Sac'], [[['Incubation Sac', 0.95]]]), 'board'],
  [R('potion', 4, ['Bewitching Tincture', 'Xenopurge Powder', 'Rousing Salts'], ['Incubation Sac', 'Fouled Scraper'], [[['Incubation Sac', 0.95]], [['Fouled Scraper', 0.62], ['Nettle Toggle', 0.58]]]), 'board'],
  [R('potion', 5, ['Bewitching Tincture', 'Xenopurge Powder', 'Rousing Salts'], ['Incubation Sac', 'Fouled Scraper'], [[['Incubation Sac', 0.95]], [['Fouled Scraper', 0.91], ['Nettle Toggle', 0.5]]]), 'board'],
  [R('potion', 6, ['Bewitching Tincture'], ['Incubation Sac', 'Fouled Scraper', 'Canine Teeth File', 'Eyelid Speculum'], []), 'board'],
]
let t = newTracker()
const tracked = steps.map(([r, trigger]) => {
  const res = trackInstruments(t, r, trigger)
  t = res.tracker
  return { reading: r, trigger: trigger ?? null, result: JSON.parse(JSON.stringify(res)) }
})
lines.push(JSON.stringify({ kind: 'track', steps: tracked }))

writeFileSync(join(out, 'scan.jsonl'), lines.join('\n') + '\n')
console.log(`scan: ${lines.length} records in ${((Date.now() - t0) / 1000).toFixed(1)} s`)
