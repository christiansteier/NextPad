// Buzzer im Quiz-Raster: Leitung und zwei Handys (Rot, Blau). Prüft Teamwahl, Uhrabgleich, Reihenfolge nach Druckzeit,
// Sperre nach dem Aufdecken und neue Runde beim nächsten Feld.
// node test/buzzertest.mjs <basis-url>
import { chromium } from 'playwright'
const base = process.argv[2] || 'http://127.0.0.1:5095'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const sleep = ms => new Promise(r => setTimeout(r, ms))
const table = `| Kategorie | Punkte | Frage | Antwort |\n| --- | --- | --- | --- |\n| Audio | 100 | Wie lang? | Fünf Minuten |\n| Audio | 200 | Was heißt Spur? | Eine je Person |`

const b = await chromium.launch()
const lead = await (await b.newContext({ viewport: { width: 1400, height: 900 } })).newPage()
await lead.goto(base + '/new?typ=quiz'); await lead.waitForSelector('#setup:not([hidden])')
const url = lead.url()
await lead.fill('#in-title', 'Buzzer-Probe'); await lead.fill('#in-teams', 'Rot, Blau'); await lead.fill('#in-table', table)
await lead.click('#q-save'); await lead.waitForSelector('.quiz-cell')
check('Leitung sieht keine Teamwahl', await lead.isHidden('#buzz-wahl'))

const phone = async team => {
  const p = await (await b.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true })).newPage()
  await p.goto(url); await p.waitForSelector('#buzz-wahl:not([hidden])')
  await p.click(`#buzz-wahl button:text-is("${team}")`)
  return p
}
const rot = await phone('Rot'), blau = await phone('Blau')
check('Handy: Team gewählt, Wechsel möglich', (await rot.textContent('#buzz-wahl')).includes('Team wechseln') && await rot.locator('#buzz-wahl [aria-pressed="true"]').textContent() === 'Rot')

await lead.click('.quiz-cell >> nth=0'); await lead.waitForSelector('#question:not([hidden])')
for (const p of [rot, blau]) await p.waitForSelector('#buzz-btn:not([hidden]):not([disabled])', { timeout: 15000 })
check('Buzzer erscheint auf beiden Handys, nach dem Abgleich bereit', (await blau.textContent('#buzz-btn')) === 'Buzzer')
check('kein Buzzer bei der Leitung', await lead.isHidden('#buzz-btn'))

await blau.tap('#buzz-btn'); await sleep(150); await rot.tap('#buzz-btn')
check('nach dem Druck: Buzzer weg, „Gedrückt.“', await blau.isHidden('#buzz-btn') && (await blau.textContent('#buzz-info')) === 'Gedrückt.')
await lead.waitForFunction(() => document.getElementById('buzz-kopf').textContent === 'Buzzer', null, { timeout: 5000 })
const reihe = await lead.locator('#buzz-reihe li').allTextContents()
check('Reihenfolge nach Druckzeit: Blau vor Rot', reihe.length === 2 && reihe[0].startsWith('Blau') && reihe[1].startsWith('Rot'), reihe.join(' / '))
const abstand = Number((reihe[1].match(/\+(\d+) ms/) || [])[1])
check('Abstand etwa wie gedrückt (100 bis 400 ms)', abstand >= 100 && abstand <= 400, reihe[1])
check('Leitung: Blau bei der Wertung als Erstes markiert', (await lead.textContent('.quiz-team.is-first .quiz-team-name')) === '1. Blau')
await rot.waitForFunction(() => document.getElementById('buzz-kopf').textContent === 'Buzzer', null, { timeout: 5000 })
check('Handys sehen die Reihenfolge live', (await rot.locator('#buzz-reihe li').first().textContent()).startsWith('Blau'))

const again = await blau.evaluate(async u => {
  const r = await fetch(u + '/quiz/buzzer', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ dev: sessionStorage.getItem('nextpad-geraet'), team: 2, field: 1, t: performance.timeOrigin + performance.now(), off: 0, rtt: 10 }) })
  return r.json()
}, url)
check('zweiter Druck desselben Geräts zählt nicht', again.neu === false, JSON.stringify(again))
const badTeam = await blau.evaluate(async u => (await fetch(u + '/quiz/buzzer', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ dev: 'abcdefgh99', team: 9, field: 1, t: 1, off: 0, rtt: 10 }) })).status, url)
check('unbekanntes Team 400', badTeam === 400, String(badTeam))

await lead.keyboard.press('Space')
await sleep(500)
const closed = await blau.evaluate(async u => (await fetch(u + '/quiz/buzzer', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ dev: 'abcdefgh77', team: 1, field: 1, t: performance.timeOrigin + performance.now(), off: 0, rtt: 10 }) })).status, url)
check('nach dem Aufdecken ist der Buzzer zu (409)', closed === 409, String(closed))
const noOrigin = await fetch(url + '/quiz/buzzer', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })
check('ohne Origin abgelehnt', noOrigin.status === 403, String(noOrigin.status))

await lead.click('.quiz-team >> nth=1 >> button.plus'); await lead.waitForSelector('#board:not([hidden])')
await lead.click('.quiz-cell:not(:disabled)'); await lead.waitForSelector('#question:not([hidden])')
await rot.waitForSelector('#buzz-btn:not([hidden]):not([disabled])', { timeout: 8000 })
check('nächste Frage: neue Runde, Buzzer wieder da, Reihenfolge leer', await lead.isHidden('#buzz-reihe') && await rot.isVisible('#buzz-btn'))

// Seite neu geladen, Team ist gespeichert, Frage läuft schon (z. B. zurück von der Messung oder neu aus Moodle)
await blau.reload(); await blau.waitForSelector('#buzz-wahl:not([hidden])')
check('nach dem Neuladen: Team noch gewählt', await blau.locator('#buzz-wahl [aria-pressed="true"]').textContent() === 'Blau')
await blau.waitForSelector('#buzz-btn:not([hidden]):not([disabled])', { timeout: 8000 }).catch(() => {})
check('nach dem Neuladen: Buzzer wird bereit, ohne Team neu zu wählen', (await blau.textContent('#buzz-btn')) === 'Buzzer' && await blau.isEnabled('#buzz-btn'), await blau.textContent('#buzz-btn'))
await blau.tap('#buzz-btn')
await lead.waitForFunction(() => [...document.querySelectorAll('#buzz-reihe li')].some(li => li.textContent.startsWith('Blau')), null, { timeout: 5000 }).catch(() => {})
check('nach dem Neuladen: Druck kommt bei der Leitung an', (await lead.locator('#buzz-reihe li').allTextContents()).some(t => t.startsWith('Blau')))
// Weg über die Messung: Handy geht zur Messung und zurück ins Quiz
await rot.goto(url + '/messung'); await rot.waitForSelector('#m-handy:not([hidden])'); await rot.goBack(); await rot.waitForSelector('#quiz-title')
await rot.waitForSelector('#buzz-btn:not([hidden]):not([disabled])', { timeout: 8000 }).catch(() => {})
check('zurück von der Messung: Buzzer bereit', await rot.isVisible('#buzz-btn') && await rot.isEnabled('#buzz-btn'))
// Wachhalten: ohne Unterstützung Hinweis, mit Unterstützung keiner
const ohne = await b.newContext({ viewport: { width: 390, height: 844 } })
await ohne.addInitScript(() => { Object.defineProperty(Navigator.prototype, 'wakeLock', { get: () => undefined }) })
const po = await ohne.newPage(); await po.goto(url); await po.waitForSelector('#buzz-wahl:not([hidden])')
await po.click('#buzz-wahl button:text-is("Rot")')
await po.waitForSelector('#buzz-wach:not([hidden])', { timeout: 3000 }).catch(() => {})
check('ohne Wake Lock: Hinweis zur automatischen Sperre', await po.isVisible('#buzz-wach') && (await po.textContent('#buzz-wach')).includes('Automatische Sperre'))
const mit = await b.newContext({ viewport: { width: 390, height: 844 } })
await mit.addInitScript(() => { Object.defineProperty(Navigator.prototype, 'wakeLock', { get: () => ({ request: async () => ({ release: async () => {} }) }) }) })
const pm = await mit.newPage(); await pm.goto(url); await pm.waitForSelector('#buzz-wahl:not([hidden])')
await pm.click('#buzz-wahl button:text-is("Rot")'); await sleep(300)
check('mit Wake Lock: kein Hinweis', await pm.isHidden('#buzz-wach'))
await b.close()
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
