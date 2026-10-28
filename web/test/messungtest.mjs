// Buzzer-Messung: Leitung und drei Handys in getrennten Browsern. Prüft Ablauf, Rechte und dass keine Gerätekennung herausgeht.
// node test/messungtest.mjs <basis-url>
import { chromium } from 'playwright'
const base = process.argv[2] || 'http://127.0.0.1:5095'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const sleep = ms => new Promise(r => setTimeout(r, ms))

const b = await chromium.launch()
const lead = await (await b.newContext({ viewport: { width: 1400, height: 900 } })).newPage()
await lead.goto(base + '/new?typ=quiz'); await lead.waitForSelector('#setup:not([hidden])')
const quiz = lead.url()
await lead.click('#q-menu > summary')
check('Quiz zeigt der Leitung den Weg zur Messung (im Menü)', await lead.isVisible('#q-messung'))
// Mitsehende im Quiz: Knopf zum Mitmessen erst, wenn die Leitung die Messseite öffnet
const seher = await (await b.newContext({ viewport: { width: 390, height: 844 } })).newPage()
await seher.goto(quiz); await seher.waitForSelector('#quiz-title')
await sleep(500)
check('Mitsehende: kein Mitmessen-Knopf ohne laufende Messung', await seher.isHidden('#viewer-tools'))
await lead.click('#q-messung'); await lead.waitForSelector('#m-lead:not([hidden])')
await seher.waitForSelector('#viewer-tools:not([hidden])', { timeout: 10000 })
check('Mitsehende: Knopf „An der Messung teilnehmen“ erscheint von selbst', (await seher.textContent('#q-mitmessen')) === 'An der Messung teilnehmen')
await seher.click('#q-mitmessen'); await seher.waitForSelector('#m-handy:not([hidden])')
check('Knopf führt auf die Messung als Handy', seher.url() === quiz + '/messung')
await seher.close()
const url = lead.url()
check('Leitung sieht Steuerung, nicht den Buzzer', url === quiz + '/messung' && await lead.isHidden('#m-handy'))
check('QR-Code führt auf die Messung', (await lead.textContent('#qr-box')).trim().endsWith('/messung'))

const phones = []
for (let i = 0; i < 3; i++) {
  const p = await (await b.newContext({ viewport: { width: 390, height: 844 }, userAgent: i === 0 ? 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)' : 'Mozilla/5.0 (Linux; Android 14)' })).newPage()
  await p.goto(url); await p.waitForSelector('#m-handy:not([hidden])')
  phones.push(p)
}
check('Handys sehen keine Leitungswerkzeuge', await phones[0].isHidden('#m-lead'))
for (const p of phones) await p.click('#m-start')
await phones[0].waitForFunction(() => /Abgleiche/.test(document.getElementById('m-status').textContent), null, { timeout: 15000 })
check('Handy gleicht ab', /Misst mit: \d+ Abgleiche/.test(await phones[0].textContent('#m-status')))
await sleep(7000)
await lead.waitForFunction(() => document.getElementById('m-aktiv').textContent === '3', null, { timeout: 10000 })
check('Leitung zählt drei aktive Geräte', true)

for (let r = 1; r <= 3; r++) {
  await lead.keyboard.press('Space')
  for (const p of phones) await p.waitForSelector('#m-buzzer.is-jetzt', { timeout: 8000 })
  for (const [i, p] of phones.entries()) { await sleep(40 * i); await p.click('#m-buzzer') }
  await phones[2].waitForFunction(() => /Gedrückt nach/.test(document.getElementById('m-reaktion').textContent))
  await sleep(1200)
}
check('Handy zeigt die eigene Reaktionszeit', /Gedrückt nach \d+ ms/.test(await phones[0].textContent('#m-reaktion')))
check('Buzzer nach dem Druck wieder gesperrt', await phones[0].isDisabled('#m-buzzer'))

const daten = await lead.evaluate(async u => (await fetch(u + '/daten')).json(), url)
check('Auswertung: 3 Geräte, 3 Runden, alle gewertet', daten.geraete === 3 && daten.runden === 3 && daten.vergleich.runden === 3, JSON.stringify(daten.vergleich))
check('Auswertung: Laufzeit, Uhr und Signal gemessen', daten.laufzeit.median != null && daten.uhr.p95 != null && daten.signal.median != null && daten.eingang.median != null, JSON.stringify(daten))
check('Geräteart grob erkannt', daten.liste.map(g => g.art).sort().join() === 'Android,Android,iOS', JSON.stringify(daten.liste))
check('je Gerät drei Drücke', daten.liste.every(g => g.druecke === 3))
await lead.waitForFunction(() => document.getElementById('m-urteil').textContent.length > 0)
check('Urteil steht auf der Seite', (await lead.textContent('#m-urteil')).length > 20)

const dev = await phones[0].evaluate(() => sessionStorage.getItem('nextpad-messung'))
const csv = await lead.evaluate(async u => (await fetch(u + '/export.csv')).text(), url)
check('CSV mit Abgleichen und Runden', csv.startsWith('zeile,geraet') && /\nabgleich,/.test(csv) && /\nrunde,\d+,(iOS|Android),3,/.test(csv), csv.slice(0, 300))
check('keine Gerätekennung in Auswertung und CSV', dev && !JSON.stringify(daten).includes(dev) && !csv.includes(dev))

const denied = await phones[1].evaluate(async u => [
  (await fetch(u + '/daten')).status, (await fetch(u + '/export.csv')).status,
  (await fetch(u + '/signal', { method: 'POST' })).status, (await fetch(u + '/loeschen', { method: 'POST' })).status,
], url)
check('Handys: keine Auswertung, kein Signal, kein Löschen (403)', denied.every(s => s === 403), denied.join(','))
const bad = await phones[1].evaluate(async u => [
  (await fetch(u + '/probe', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ dev: 'x', rtt: 1, rttMed: 2, off: 0 }) })).status,
  (await fetch(u + '/druck', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ dev: 'abcdefgh12', runde: 99, t: 1, off: 0, rtt: 1 }) })).status,
], url)
check('ungültige Kennung 400, unbekannte Runde 404', bad.join() === '400,404', bad.join())
const noOrigin = await fetch(url + '/probe', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ dev: 'abcdefgh12', rtt: 1, rttMed: 2, off: 0 }) })
check('ohne Origin abgelehnt', noOrigin.status === 403, String(noOrigin.status))

lead.once('dialog', d => d.accept()); await lead.click('#m-loeschen')
await lead.waitForFunction(() => document.getElementById('m-aktiv').textContent !== '3' || true)
await sleep(500)
const leer = await lead.evaluate(async u => (await fetch(u + '/daten')).json(), url)
check('Löschen: Runden und Drücke weg', leer.runden === 0 && leer.vergleich.runden === 0, JSON.stringify({ r: leer.runden, g: leer.geraete }))
// verlässt die Leitung die Messseite, verschwindet der Knopf nach spätestens 10 s plus 15 s
await lead.goto(quiz); await lead.waitForSelector('#quiz-title')
const spaeter = await (await b.newContext()).newPage()
await spaeter.goto(quiz); await spaeter.waitForSelector('#quiz-title')
await spaeter.waitForSelector('#viewer-tools', { state: 'hidden', timeout: 30000 }).catch(() => {})
check('ohne Leitung auf der Messseite: Knopf wieder weg', await spaeter.isHidden('#viewer-tools'))
await b.close()
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
