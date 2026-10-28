// Quiz-Raster: Leitung und Mitsehende in zwei Browsern. Kernprüfung: Antworten erreichen Mitsehende
// erst nach dem Aufdecken, auch nicht über Stand, Live-Kanal oder Export; schreiben darf nur die Leitung.
// node test/quiztest.mjs <basis-url>
import { chromium } from 'playwright'
const base = process.argv[2] || 'http://127.0.0.1:5095'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const ANTWORT = 'Geheimantwort-4711'
const table = `| Kategorie | Punkte | Frage | Antwort |\n| --- | --- | --- | --- |\n| Audio | 100 | Wie lang? | ${ANTWORT} |\n| Audio | 200 | Was heißt Spur? | Eine je Person |\n| KI | 100 | Welcher Dienst? | KI:connect.nrw |`

const b = await chromium.launch()
const lead = await (await b.newContext({ viewport: { width: 1400, height: 900 } })).newPage()
await lead.goto(base + '/new?typ=quiz'); await lead.waitForSelector('#setup:not([hidden])')
const url = lead.url(); const id = url.split('/p/')[1]
check('neues Quiz öffnet Einrichtung für die Leitung', /\/p\/[A-Za-z0-9]{16}$/.test(url) && await lead.isVisible('#lead-tools'))
await lead.fill('#in-title', 'Probe-Quiz'); await lead.fill('#in-teams', 'Rot, Blau'); await lead.fill('#in-table', table)
await lead.click('#q-save'); await lead.waitForSelector('.quiz-cell')
await lead.click('#q-start'); await lead.waitForSelector('.quiz-cell:not(:disabled)')
check('Raster mit 3 Feldern und 2 Kategorien', await lead.locator('.quiz-cell').count() === 3 && await lead.locator('.quiz-cat').count() === 2)

const view = await (await b.newContext({ viewport: { width: 400, height: 800 } })).newPage()
const sse = []
view.on('response', async r => { if (r.url().includes('/quiz/live')) sse.push(r.url()) })
await view.goto(url); await view.waitForSelector('.quiz-cell')
check('Mitsehende: keine Leitungswerkzeuge, Felder nicht klickbar', await view.isHidden('#lead-tools') && await view.locator('button.quiz-cell').count() === 0)

await lead.click('.quiz-cell >> text=100 >> nth=0'); await lead.waitForSelector('#question:not([hidden])')
check('Beamer (Leitung) zeigt die Antwort erst nach dem Aufdecken', await lead.isHidden('#q-answer') && !(await lead.textContent('#q-answer')).includes(ANTWORT))
await view.waitForFunction(() => !document.getElementById('question').hidden, null, { timeout: 5000 })
check('Mitsehende sehen die Frage live', (await view.textContent('#q-text')) === 'Wie lang?')
const html = await view.content()
const state = await view.evaluate(async u => (await fetch(u + '/quiz')).text(), url)
check('Antwort vor dem Aufdecken nicht im Stand der Mitsehenden', !state.includes(ANTWORT) && !state.includes('"setup"'), state.slice(0, 200))
check('Antwort vor dem Aufdecken nicht im Seitenquelltext', !html.includes(ANTWORT))
check('Antwortfeld bei Mitsehenden verborgen', await view.isHidden('#q-answer'))

const denied = await view.evaluate(async u => {
  const r = []
  for (const [p, body] of [['/quiz/feld', { field: 1, revealed: true }], ['/quiz/wertung', { field: 1, team: 1, plus: true }], ['/quiz/rueckgaengig', {}], ['/quiz/fragen', { table: 'a|1|b|c', teams: 'x' }]])
    r.push((await fetch(u + p, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })).status)
  return r
}, url)
check('Mitsehende dürfen nichts schreiben (403)', denied.every(s => s === 403), denied.join(','))
const noOrigin = await fetch(`${base}/p/${id}/quiz/feld`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{"field":1,"revealed":true}' })
check('ohne Origin und Cookie abgelehnt', noOrigin.status === 403 || noOrigin.status === 401, String(noOrigin.status))

await lead.keyboard.press('Space')
await view.waitForFunction(() => !document.getElementById('q-answer').hidden, null, { timeout: 5000 })
check('nach Aufdecken (Leertaste) sehen Mitsehende die Antwort live', (await view.textContent('#q-answer')) === ANTWORT)

await lead.click('.quiz-team >> nth=1 >> button.minus')
await lead.waitForFunction(() => document.querySelector('.quiz-score:nth-child(2) strong')?.textContent === '-100')
check('Minus: Frage bleibt offen, Blau −100', await lead.isVisible('#question'))
await lead.click('.quiz-team >> nth=0 >> button.plus')
await lead.waitForSelector('#board:not([hidden])')
check('Plus: zurück zum Raster, Feld gespielt, Rot 100', await lead.locator('.quiz-cell.is-played').count() === 1 && (await lead.textContent('.quiz-score:nth-child(1) strong')) === '100')
await view.waitForFunction(() => document.querySelectorAll('.quiz-cell.is-played').length === 1, null, { timeout: 5000 })
check('Mitsehende sehen Punktestand und gespieltes Feld live', (await view.textContent('.quiz-score:nth-child(1) strong')) === '100')

await lead.keyboard.press('z')
await lead.waitForFunction(() => document.querySelectorAll('.quiz-cell.is-played').length === 0)
check('Z nimmt letzte Wertung zurück, Feld wieder offen', (await lead.textContent('.quiz-score:nth-child(1) strong')) === '0' && (await lead.textContent('.quiz-score:nth-child(2) strong')) === '-100')

await lead.click('.quiz-cell >> text=200'); await lead.waitForSelector('#question:not([hidden])')
await lead.click('text=Niemand, Feld schließen'); await lead.waitForSelector('#board:not([hidden])')
check('Niemand schließt das Feld ohne Punkte', await lead.locator('.quiz-cell.is-played').count() === 1)

const exp = await (await fetch(`${base}/p/${id}/export/md`)).text()
const expView = await view.evaluate(async u => (await fetch(u + '/export/md')).text(), url)
check('Export nur Punktestand, ohne Fragen und Antworten', expView.includes('Probe-Quiz') && expView.includes('Rot') && !expView.includes(ANTWORT) && !expView.includes('Wie lang'), expView)
check('Export ohne Sitzung abgelehnt oder ohne Antworten', !exp.includes(ANTWORT))

await lead.reload(); await lead.waitForSelector('.quiz-cell')
check('Stand übersteht Neuladen', await lead.locator('.quiz-cell.is-played').count() === 1 && (await lead.textContent('.quiz-score:nth-child(2) strong')) === '-100')
await lead.click('#q-menu > summary'); await lead.click('#q-edit'); await lead.fill('#in-table', '| a | b |')
lead.once('dialog', d => d.accept()); await lead.click('#q-save')
await lead.waitForSelector('#setup-error:not([hidden])')
check('falsche Tabelle: Fehlermeldung, Stand bleibt', (await lead.textContent('#setup-error')).includes('vier Spalten') && await lead.locator('.quiz-cell.is-played').count() === 1)
check('Live-Kanal geöffnet', sse.length > 0)

// Gespielte Felder gesperrt, auch serverseitig
await lead.click('#q-menu > summary'); await lead.click('#q-edit')
check('gespieltes Feld ist gesperrt', await lead.locator('.quiz-cell.is-played').first().isDisabled())
const played = await lead.evaluate(async u => { const d = await (await fetch(u + '/quiz')).json(); return d.board.find(f => f.played).id }, url)
const reopen = await lead.evaluate(async ([u, f]) => (await fetch(u + '/quiz/feld', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ field: f }) })).status, [url, played])
check('gespielte Frage lässt sich nicht wieder öffnen (409)', reopen === 409, String(reopen))
const wertungZu = await lead.evaluate(async u => {
  const q = await (await fetch(u + '/quiz', { cache: 'no-store' })).json()
  const zu = q.board.find(f => !f.played)
  return (await fetch(u + '/quiz/wertung', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ field: zu.id, team: 1, plus: true }) })).status
}, url)
check('nicht geöffnete Frage lässt sich nicht werten (409)', wertungZu === 409, String(wertungZu))

// Während einer Frage nur Aufdecken, Wertung, Vollbild
await lead.click('.quiz-cell >> text=100 >> nth=0'); await lead.waitForSelector('#question:not([hidden])')
await lead.click('#q-menu > summary')
check('während der Frage: im Menü nur Vollbild und Ton, kein Rückgängig, QR, Bearbeiten', await lead.isHidden('#q-undo') && await lead.isHidden('#q-qr') && await lead.isHidden('#q-edit') && await lead.isVisible('#q-full') && await lead.isVisible('#q-ton'))
await lead.click('h1')
check('Klick daneben schließt das Menü', !(await lead.evaluate(() => document.getElementById('q-menu').open)))
await lead.click('.quiz-team >> nth=0 >> button.plus'); await lead.waitForSelector('#board:not([hidden])')
check('geänderter Punktestand wird hervorgehoben', await lead.locator('.quiz-score.is-changed').count() === 1)
await lead.click('#q-menu > summary')
check('nach der Frage: Werkzeuge wieder da', await lead.isVisible('#q-undo'))
await lead.click('#q-menu > summary')

// Letzte Frage: niemand → Abschluss
await lead.click('.quiz-cell:not(:disabled)'); await lead.waitForSelector('#question:not([hidden])')
await lead.click('text=Niemand, Feld schließen')
await lead.waitForSelector('#ende:not([hidden])')
check('Abschluss nach der letzten Frage, Raster ausgeblendet', await lead.isHidden('#board') && (await lead.textContent('.quiz-sieg')) === 'Rot gewinnt')
check('Platzierung in richtiger Reihenfolge', (await lead.locator('.quiz-rang li').allTextContents()).join('|') === 'Rot100|Blau-100')
const nach = await lead.locator('.quiz-nach li').allTextContents()
check('Nachbesprechung: Fragen mit Abzug oder ohne Lösung', nach.length === 3 && nach.some(x => x.includes('Wie lang?')) && nach.some(x => x.includes('Welcher Dienst?')), nach.join(' / '))
await view.waitForSelector('#ende:not([hidden])', { timeout: 5000 })
check('Mitsehende sehen den Abschluss, ohne Antworten', (await view.textContent('#ende')).includes('Rot gewinnt') && !(await view.textContent('#ende')).includes(ANTWORT))
await lead.keyboard.press('z')
await lead.waitForSelector('#board:not([hidden])')
check('Rückgängig holt die letzte Frage zurück', await lead.isHidden('#ende'))
await b.close()
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
