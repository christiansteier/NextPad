// Zeitleiste: wem das freie Pad gehört, stellt einen früheren Stand wieder her; ein Gast sieht das Pad live, darf es aber nicht.
// node test/wiederherstelltest.mjs <basis-url>
import { chromium } from 'playwright'
const base = process.argv[2] || 'http://127.0.0.1:5095'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const sleep = ms => new Promise(r => setTimeout(r, ms))
const b = await chromium.launch()
const A = await (await b.newContext()).newPage(), B = await (await b.newContext()).newPage()
const errs = []; A.on('pageerror', e => errs.push(e.message))
await A.goto(base + '/new'); await A.waitForSelector('.ProseMirror'); const url = A.url()
await A.click('.ProseMirror'); await sleep(200); await A.keyboard.type('Erste Fassung')
await sleep(1500)
await A.keyboard.press('Control+A'); await A.keyboard.type('Zweite Fassung, ganz anders')
await sleep(1500)
await B.goto(url); await B.waitForSelector('.ProseMirror')
await A.click('button[title="Frühere Stände ansehen"]'); await A.waitForSelector('#timeline .tl-slider')
check('Besitzer: Knopf „wiederherstellen“ da, beim jetzigen Stand gesperrt', await A.isVisible('.tl-restore') && await A.isDisabled('.tl-restore'))
// Punkt suchen, an dem genau die erste Fassung steht
const max = Number(await A.getAttribute('.tl-slider', 'max'))
let ziel = -1
for (let k = max; k >= 0; k--) {
  await A.fill('.tl-slider', String(k))
  if ((await A.textContent('#timeline .snapshot')).trim() === 'Erste Fassung') { ziel = k; break }
}
check('Zeitleiste zeigt die erste Fassung', ziel >= 0)
A.once('dialog', d => d.accept())
await A.click('.tl-restore')
await A.waitForSelector('#timeline', { state: 'detached', timeout: 5000 }).catch(() => {})
await A.waitForFunction(() => document.querySelector('.ProseMirror').textContent.trim() === 'Erste Fassung', null, { timeout: 5000 }).catch(() => {})
check('nach dem Wiederherstellen: Editor zeigt die erste Fassung', (await A.textContent('.ProseMirror')).trim() === 'Erste Fassung', await A.textContent('.ProseMirror'))
await B.waitForFunction(() => document.querySelector('.ProseMirror').textContent.trim() === 'Erste Fassung', null, { timeout: 5000 }).catch(() => {})
check('anderer Browser sieht es live', (await B.textContent('.ProseMirror')).trim() === 'Erste Fassung')
await A.click('button[title="Frühere Stände ansehen"]'); await A.waitForSelector('#timeline .tl-info')
await A.waitForFunction(() => /Wiederhergestellt/.test(document.querySelector('#timeline .tl-info').textContent), null, { timeout: 5000 }).catch(() => {})
check('Zeitleiste: letzter Stand „Wiederhergestellt“, die zweite Fassung bleibt erreichbar', (await A.textContent('#timeline .tl-info')).includes('Wiederhergestellt'), await A.textContent('#timeline .tl-info'))
await B.click('button[title="Frühere Stände ansehen"]'); await B.waitForSelector('#timeline .tl-slider')
check('Gast: kein Knopf', await B.isHidden('.tl-restore'))
const st = await B.evaluate(async u => (await fetch(u + '/wiederherstellen', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ seq: 1 }) })).status, url)
check('Gast: Server lehnt ab (403)', st === 403, String(st))
const fremd = await fetch(url + '/wiederherstellen', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ seq: 1 }) })
check('ohne Origin abgelehnt', fremd.status === 403, String(fremd.status))
const weg = await A.evaluate(async u => (await fetch(u + '/wiederherstellen', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ seq: 999999 }) })).status, url)
check('unbekannter Punkt: 404', weg === 404, String(weg))
const stand = await A.evaluate(async u => {
  const list = await (await fetch(u + '/snapshots')).json()
  const r = await fetch(u + '/wiederherstellen', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ ts: list[0].ts }) })
  return [list.length, r.status]
}, url)
check('Vollstand (ts) lässt sich ebenso wiederherstellen', stand[0] > 0 && stand[1] === 200, JSON.stringify(stand))
check('keine Skriptfehler', errs.length === 0, errs.join(' | '))
await b.close()
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
