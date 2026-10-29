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
const leadCtx = await b.newContext({ viewport: { width: 1400, height: 900 } })
// Ton zählen: jeder Buzzer-Ton erzeugt zwei Oszillatoren
await leadCtx.addInitScript(() => { window.__osc = 0; const o = AudioContext.prototype.createOscillator; AudioContext.prototype.createOscillator = function () { window.__osc++; return o.call(this) } })
const lead = await leadCtx.newPage()
await lead.goto(base + '/new?typ=quiz'); await lead.waitForSelector('#setup:not([hidden])')
const url = lead.url()
await lead.fill('#in-title', 'Buzzer-Probe'); await lead.fill('#in-teams', 'Rot, Blau'); await lead.fill('#in-table', table)
check('Vorgabe: 10 Sekunden je Frage', (await lead.inputValue('#in-sek')) === '10')
await lead.fill('#in-sek', '0') // dieser Durchlauf ohne Countdown, er braucht länger als zehn Sekunden je Frage
await lead.click('#q-save'); await lead.waitForSelector('.quiz-cell')
check('Leitung sieht keine Teamwahl', await lead.isHidden('#buzz-wahl'))
check('vor dem Start: Raster gesperrt, „Spiel starten“ sichtbar', await lead.locator('.quiz-cell:not(:disabled)').count() === 0 && await lead.isVisible('#q-start'))

const phone = async team => {
  const p = await (await b.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true })).newPage()
  await p.goto(url); await p.waitForSelector('#buzz-wahl:not([hidden])')
  await p.click(`#buzz-wahl button:text-is("${team}")`)
  return p
}
const rot = await phone('Rot'), blau = await phone('Blau')
check('Handy: Team gewählt, Wechsel möglich', (await rot.textContent('#buzz-wahl')).includes('Team wechseln') && await rot.locator('#buzz-wahl [aria-pressed="true"]').textContent() === 'Rot')

check('vor dem Start: Handy kann Team wechseln, „Das Spiel beginnt gleich“', (await rot.textContent('#buzz-wahl')).includes('Team wechseln') && (await rot.textContent('#buzz-wahl')).includes('Das Spiel beginnt gleich'))
await lead.click('#q-start'); await lead.waitForSelector('.quiz-cell:not(:disabled)')
await rot.waitForFunction(() => !document.getElementById('buzz-wahl').textContent.includes('Team wechseln'), null, { timeout: 5000 }).catch(() => {})
check('nach dem Start: Team fest, kein Wechsel', !(await rot.textContent('#buzz-wahl')).includes('Team wechseln'))
check('Handy ohne offene Frage: Hinweis, worauf es wartet', (await rot.textContent('#buzz-wahl')).includes('sobald die Leitung eine Frage öffnet'))
await lead.click('.quiz-cell >> nth=0'); await lead.waitForSelector('#question:not([hidden])')
for (const p of [rot, blau]) await p.waitForSelector('#buzz-btn:not([hidden]):not([disabled])', { timeout: 15000 })
check('während der Frage kein Warte-Hinweis', !(await rot.textContent('#buzz-wahl')).includes('sobald die Leitung'))
check('Buzzer erscheint auf beiden Handys, nach dem Abgleich bereit', (await blau.textContent('#buzz-btn')) === 'Buzzer')
check('kein Buzzer bei der Leitung', await lead.isHidden('#buzz-btn'))
check('ohne Countdown: keine Uhr', await lead.isHidden('#q-countdown'))

await blau.tap('#buzz-btn'); await sleep(150); await rot.tap('#buzz-btn')
check('nach dem Druck: Buzzer weg, „Gedrückt.“', await blau.isHidden('#buzz-btn') && (await blau.textContent('#buzz-info')) === 'Gedrückt.')
await lead.waitForFunction(() => document.getElementById('buzz-kopf').textContent === 'Buzzer', null, { timeout: 5000 })
const reihe = await lead.locator('#buzz-reihe li').allTextContents()
check('Reihenfolge nach Druckzeit: Blau vor Rot', reihe.length === 2 && reihe[0].startsWith('Blau') && reihe[1].startsWith('Rot'), reihe.join(' / '))
const abstand = Number((reihe[1].match(/\+(\d+) ms/) || [])[1])
check('Abstand etwa wie gedrückt (100 bis 400 ms)', abstand >= 100 && abstand <= 400, reihe[1])
await lead.waitForSelector('#buzz-erster:not([hidden])', { timeout: 5000 }).catch(() => {})
check('Beamer: groß „Zuerst gedrückt · x,xx s: Blau“', /^Zuerst gedrückt · \d+,\d\d sBlau$/.test(await lead.textContent('#buzz-erster')), await lead.textContent('#buzz-erster'))
check('Beamer: Buzzer-Ton einmal je Frage', await lead.evaluate(() => window.__osc) === 2, String(await lead.evaluate(() => window.__osc)))
check('Handys sehen die Einblendung auch', await rot.waitForSelector('#buzz-erster:not([hidden])', { timeout: 5000 }).then(() => true, () => false))
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
check('im laufenden Spiel kein „Team wechseln“', !(await blau.textContent('#buzz-wahl')).includes('Team wechseln'))

await lead.keyboard.press('Space')
await sleep(500)
const closed = await blau.evaluate(async u => (await fetch(u + '/quiz/buzzer', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ dev: 'abcdefgh77', team: 1, field: 1, t: performance.timeOrigin + performance.now(), off: 0, rtt: 10 }) })).status, url)
check('nach dem Aufdecken ist der Buzzer zu (409)', closed === 409, String(closed))
check('nach dem Aufdecken: Einblendung weg', await lead.isHidden('#buzz-erster'))
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
const wechsel = await rot.evaluate(async u => {
  const q = await (await fetch(u + '/quiz', { cache: 'no-store' })).json()
  return (await fetch(u + '/quiz/buzzer', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ dev: sessionStorage.getItem('nextpad-geraet'), team: 2, field: q.current.id, t: performance.timeOrigin + performance.now(), off: 0, rtt: 10 }) })).status
}, url)
check('Server: dasselbe Handy kann nicht für ein anderes Team drücken (409)', wechsel === 409, String(wechsel))
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
// Spiel beenden: Ergebnis sofort, auch mit offenen Feldern; dann neues Spiel
await lead.keyboard.press('Escape'); await lead.waitForSelector('#board:not([hidden])')
check('Ton aus und an im Menü', await (async () => { await lead.click('#q-menu > summary'); await lead.click('#q-ton'); const aus = await lead.textContent('#q-ton'); await lead.click('#q-menu > summary'); await lead.click('#q-ton'); return aus === 'Buzzer-Ton aus' && (await lead.textContent('#q-ton')) === 'Buzzer-Ton an' })())
lead.once('dialog', d => d.accept()); await lead.click('#q-stop')
await lead.waitForSelector('#ende:not([hidden])')
check('Spiel beenden: Ergebnis auf dem Beamer, Raster weg', (await lead.textContent('#ende')).includes('gewinnt') && await lead.isHidden('#board'))
await rot.waitForSelector('#ende:not([hidden])', { timeout: 5000 })
check('Spiel beenden: Handys sehen das Ergebnis, keine Teamwahl', await rot.isHidden('#buzz-wahl'))
check('nach dem Beenden: „Neues Spiel“ statt „Spiel beenden“', await lead.isVisible('#q-neu') && await lead.isHidden('#q-stop'))
lead.once('dialog', d => d.accept()); await lead.click('#q-neu')
await lead.waitForSelector('#q-start:not([hidden])')
check('neues Spiel: Punkte null, Raster gesperrt bis zum Start', (await lead.locator('.quiz-score strong').allTextContents()).every(t => t === '0') && await lead.locator('.quiz-cell:not(:disabled)').count() === 0)
await rot.waitForFunction(() => document.getElementById('buzz-wahl').textContent.includes('Team wechseln'), null, { timeout: 5000 }).catch(() => {})
check('neues Spiel: Handys dürfen wieder das Team wechseln', (await rot.textContent('#buzz-wahl')).includes('Team wechseln'))

// Countdown, nächstes Team nach falscher Antwort, Statistik
const l2 = await (await b.newContext({ viewport: { width: 1400, height: 900 } })).newPage()
await l2.goto(base + '/new?typ=quiz'); await l2.waitForSelector('#setup:not([hidden])')
const url2 = l2.url()
await l2.fill('#in-title', 'Countdown-Probe'); await l2.fill('#in-teams', 'Rot, Blau'); await l2.fill('#in-table', table); await l2.fill('#in-sek', '4')
await l2.click('#q-save'); await l2.waitForSelector('#q-start:not([hidden])')
const p2 = async team => { const p = await (await b.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true })).newPage(); await p.goto(url2); await p.waitForSelector('#buzz-wahl:not([hidden])'); await p.click(`#buzz-wahl button:text-is("${team}")`); return p }
const rot2 = await p2('Rot'), blau2 = await p2('Blau')
await l2.click('#q-start'); await l2.waitForSelector('.quiz-cell:not(:disabled)')
for (const p of [rot2, blau2]) await p.waitForSelector('#buzz-wahl :text("sobald die Leitung")', { timeout: 15000 }).catch(() => {})
await sleep(1500) // Uhrabgleich vor dem Öffnen
await l2.click('.quiz-cell >> nth=0'); await l2.waitForSelector('#question:not([hidden])')
await l2.waitForSelector('#q-countdown:not([hidden])', { timeout: 3000 }).catch(() => {})
const c0 = Number(await l2.textContent('#q-countdown'))
check('Countdown auf dem Beamer zählt ab 4', c0 >= 3 && c0 <= 4, String(c0))
const rest = await rot2.evaluate(async u => (await (await fetch(u + '/quiz', { cache: 'no-store' })).json()).current, url2)
check('Server liefert sekunden und restMs', rest.sekunden === 4 && rest.restMs > 0 && rest.restMs <= 4000, JSON.stringify({ s: rest.sekunden, r: rest.restMs }))
for (const p of [rot2, blau2]) await p.waitForSelector('#buzz-btn:not([hidden]):not([disabled])', { timeout: 3000 })
await blau2.tap('#buzz-btn'); await sleep(150); await rot2.tap('#buzz-btn')
await l2.waitForFunction(() => document.querySelectorAll('#buzz-reihe li').length === 2 && document.getElementById('buzz-erster').textContent, null, { timeout: 5000 }).catch(() => {})
check('nach dem Druck: Uhr weg, Team groß', await l2.isHidden('#q-countdown') && (await l2.textContent('#buzz-erster')).endsWith('Blau'), `${await l2.isHidden('#q-countdown')} ${await l2.textContent('#buzz-erster')}`)
await l2.click('.quiz-team >> nth=1 >> button.minus')
await l2.waitForFunction(() => document.getElementById('buzz-erster').textContent.startsWith('Jetzt dran'), null, { timeout: 5000 }).catch(() => {})
check('nach falscher Antwort: „Jetzt dran: Rot“', (await l2.textContent('#buzz-erster')) === 'Jetzt dranRot', await l2.textContent('#buzz-erster'))
const r2 = await l2.locator('#buzz-reihe li').allTextContents()
check('Reihenfolge: Blau als falsch markiert, Rot hervorgehoben', r2[0].includes('falsch geantwortet') && (await l2.textContent('#buzz-reihe li.is-first')).startsWith('Rot'), r2.join(' / '))
check('Wertung: „Dran: Rot“', (await l2.textContent('.quiz-team.is-first .quiz-team-name')) === 'Dran: Rot')
await l2.click('.quiz-team >> nth=0 >> button.plus'); await l2.waitForSelector('#board:not([hidden])')
// zweites Feld: niemand drückt, die Zeit läuft ab
await l2.click('.quiz-cell:not(:disabled)'); await l2.waitForSelector('#question:not([hidden])')
await rot2.waitForSelector('#buzz-btn:not([hidden]):not([disabled])', { timeout: 3000 }).catch(() => {})
await l2.waitForFunction(() => document.getElementById('q-countdown').textContent === 'Zeit um', null, { timeout: 7000 }).catch(() => {})
check('Countdown abgelaufen: „Zeit um“', (await l2.textContent('#q-countdown')) === 'Zeit um')
await rot2.waitForFunction(() => document.getElementById('buzz-info').textContent === 'Die Zeit ist um.', null, { timeout: 3000 }).catch(() => {})
check('Handy: Buzzer weg, „Die Zeit ist um.“', await rot2.isHidden('#buzz-btn') && (await rot2.textContent('#buzz-info')) === 'Die Zeit ist um.')
await sleep(300) // jenseits der 50 ms Kulanz auf die Druckzeit
const spaet = await rot2.evaluate(async u => {
  const q = await (await fetch(u + '/quiz', { cache: 'no-store' })).json()
  return (await fetch(u + '/quiz/buzzer', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ dev: sessionStorage.getItem('nextpad-geraet'), team: 1, field: q.current.id, t: performance.timeOrigin + performance.now(), off: 0, rtt: 10 }) })).status
}, url2)
check('Server: Druck nach Ablauf abgelehnt (409)', spaet === 409, String(spaet))
await l2.getByRole('button', { name: 'Niemand, Feld schließen' }).click(); await l2.waitForSelector('#ende:not([hidden])', { timeout: 5000 })
const ende = await l2.textContent('#ende')
check('Ergebnis: Buzzer-Statistik mit schnellstem Team und Mittelwerten', /Am schnellsten: (Rot|Blau) mit \d+,\d\d s/.test(ende) && ende.includes('Rot: im Mittel') && ende.includes('Blau: im Mittel'), ende)
await b.close()
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
