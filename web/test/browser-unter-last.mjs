// Zwei echte Browser im Editor, während Last läuft: Zeit, bis eine getippte Markierung bei B sichtbar ist,
// und lange Aufgaben (>50 ms) im Hauptthread von B. node test/browser-unter-last.mjs <pad-url> <sekunden>
import { chromium } from 'playwright'
const [url, S = '30'] = process.argv.slice(2)
const b = await chromium.launch()
const A = await (await b.newContext()).newPage(); const B = await (await b.newContext()).newPage()
for (const p of [A, B]) { await p.goto(url); await p.waitForSelector('.ProseMirror'); await p.waitForFunction(() => document.querySelector('#statusbar .ok')) }
await B.evaluate(() => { window.__long = []; new PerformanceObserver(l => { for (const e of l.getEntries()) window.__long.push(e.duration) }).observe({ type: 'longtask', buffered: true }) })
await A.click('.ProseMirror'); await A.keyboard.press('Control+End')
const lat = []
const end = Date.now() + (+S) * 1000; let k = 0
while (Date.now() < end) {
  const m = `«b${k}»`; const t0 = Date.now()
  await A.keyboard.type(m)
  const ok = await B.waitForFunction(x => document.querySelector('.ProseMirror')?.textContent.includes(x), m, { timeout: 10000, polling: 'raf' }).then(() => true).catch(() => false)
  lat.push(ok ? Date.now() - t0 : 10000); k++
  await A.waitForTimeout(700)
}
const long = await B.evaluate(() => window.__long)
lat.sort((a, b) => a - b); const q = x => lat[Math.floor(lat.length * x)]
console.log(`Browser A→B: ${lat.length} Eingaben, Median ${q(.5)} ms, p95 ${q(.95)} ms, max ${lat.at(-1)} ms (inkl. Tippen der Markierung)`)
console.log(`Hauptthread B: ${long.length} lange Aufgaben, längste ${Math.round(Math.max(0, ...long))} ms, Summe ${Math.round(long.reduce((a, b) => a + b, 0))} ms in ${S} s`)
await b.close()
