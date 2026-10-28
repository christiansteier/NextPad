// Ideensammlung: zwei Personen, Beiträge und Stimmen live, eine Stimme je Person, Moderation nur für die Anlegerin
// node test/ideentest.mjs <basis-url>
import { chromium } from 'playwright'
const base = process.argv[2] || 'http://127.0.0.1:5093'
let fails = 0
const check = (name, cond, info = '') => { console.log((cond ? 'ok   ' : 'FAIL ') + name + (info ? '  ' + info : '')); if (!cond) fails++ }
const b = await chromium.launch()
const a = await (await b.newContext()).newPage()
const g = await (await b.newContext()).newPage()

await a.goto(base + '/')
check('Startseite ohne „Leeres Pad“', !(await a.textContent('.templates')).includes('Leeres Pad'))
check('fünf Kacheln (mit Quiz-Raster)', (await a.locator('.templates li').count()) === 5)
await a.click('a[href$="/new?typ=ideen"]')
await a.waitForSelector('#add-form:not([hidden])')
const url = a.url()
check('Sammlung angelegt', /\/p\/[A-Za-z0-9]+$/.test(url), url)
check('Moderation sichtbar für die Anlegerin', await a.isVisible('#prompt-edit'))
await a.click('#prompt-edit')
await a.fill('#prompt-input', 'Was soll in die nächste Sitzung?'); await a.click('#prompt-form button')
await a.fill('#add-text', 'Erste Idee'); await a.click('#add-form button[type=submit]')
await a.fill('#add-text', 'Zweite Idee'); await a.click('#add-form button[type=submit]')
await a.waitForFunction(() => document.querySelectorAll('#open-list li').length === 2)

await g.goto(url)
await g.waitForFunction(() => document.querySelectorAll('#open-list li').length === 2)
check('Frage bei der zweiten Person', (await g.textContent('h1')).includes('nächste Sitzung'))
check('keine Moderation für die zweite Person', !(await g.isVisible('#prompt-edit')) && (await g.locator('.mod-tools').count()) === 0)
// live: neue Idee der Anlegerin erscheint ohne Neuladen
await a.fill('#add-text', 'Dritte Idee'); await a.click('#add-form button[type=submit]')
await g.waitForFunction(() => document.querySelectorAll('#open-list li').length === 3, null, { timeout: 5000 }).then(() => check('neuer Beitrag kommt live an', true)).catch(() => check('neuer Beitrag kommt live an', false))
// Stimme für „Zweite Idee“
await g.click('#open-list li:has-text("Zweite Idee") .vote')
await a.waitForFunction(() => document.querySelector('#open-list li')?.textContent.includes('Zweite Idee'), null, { timeout: 5000 }).then(() => check('Stimme kommt live an, Beitrag rückt nach oben', true)).catch(() => check('Stimme kommt live an, Beitrag rückt nach oben', false))
// doppelt abstimmen über die Schnittstelle zählt nicht doppelt
const n = await g.evaluate(async () => {
  const api = location.pathname + '/ideen'
  const items = (await (await fetch(api)).json()).items
  const id = items.find(i => i.text === 'Zweite Idee').id
  for (let k = 0; k < 3; k++) await fetch(`${api}/${id}/stimme`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{"on":true}' })
  return (await (await fetch(api)).json()).items.find(i => i.id === id).votes
})
check('eine Stimme je Person', n === 1, `Stimmen: ${n}`)
const st = await g.evaluate(async () => (await fetch(location.pathname + '/ideen/1/moderation', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{"hidden":true}' })).status)
check('Moderation durch andere abgewiesen', st === 403, `Status ${st}`)
// Anlegerin markiert erledigt und blendet aus
await a.click('#open-list li:has-text("Erste Idee") button[aria-label="Als erledigt markieren"]')
await a.click('#open-list li:has-text("Dritte Idee") button[aria-label="Ausblenden"]')
await g.waitForFunction(() => document.getElementById('done-n').textContent === '1' && document.querySelectorAll('#open-list li').length === 1, null, { timeout: 5000 }).then(() => check('erledigt und ausgeblendet kommen live an', true)).catch(() => check('erledigt und ausgeblendet kommen live an', false))
check('ausgeblendet für andere unsichtbar', !(await g.textContent('main')).includes('Dritte Idee'))
check('ausgeblendet für die Anlegerin sichtbar', (await a.textContent('#hidden-list')).includes('Dritte Idee'))
// Leinwand: Top-Beitrag zuerst, QR-Code für die Anlegerin, live
const w = await (await b.newContext()).newPage()
await w.context().addCookies(await a.context().cookies())
await w.goto(url + '/leinwand'); await w.waitForSelector('.wall-item')
check('Leinwand zeigt offene Beiträge, beliebtester zuerst', (await w.textContent('.wall-item')).includes('Zweite Idee') && (await w.locator('.wall-item').count()) === 1)
check('Leinwand mit QR-Code', (await w.locator('.wall-join svg').count()) === 1)
const fresh = await (await b.newContext()).newPage(); await fresh.goto(url + '/leinwand')
check('Leinwand im frischen Browser (Beamer) mit QR-Code', (await fresh.locator('.wall-join svg').count()) === 1); await fresh.context().close()
await a.fill('#add-text', 'Vierte Idee'); await a.click('#add-form button[type=submit]')
await w.waitForFunction(() => document.querySelectorAll('.wall-item').length === 2, null, { timeout: 5000 }).then(() => check('Leinwand live', true)).catch(() => check('Leinwand live', false))
// Lese-Link
check('Mitmachen als Knopf, kein Lese-Link mehr', await a.isVisible('#share-copy') && (await a.locator('#share-ro').count()) === 0)
check('Kopfzeile nennt die Bearbeitungsansicht', (await a.textContent('#view-kicker')).includes('Bearbeitungsansicht'))
const roId = await a.evaluate(() => JSON.parse(document.getElementById('nextpad-config').textContent).readonlyId)
await g.goto(`${base}/p/${roId}`); await g.waitForFunction(() => document.querySelectorAll('#open-list li').length === 2)
check('Lese-Link ohne Eingabe und Abstimmung', !(await g.isVisible('#add-form')) && (await g.locator('.vote').count()) === 0)
// Export
const csv = await (await a.request.get(url + '/export/csv')).text()
check('CSV ohne ausgeblendete Beiträge', csv.includes('Zweite Idee') && !csv.includes('Dritte Idee'))
await b.close()
console.log(`\n${fails ? fails + ' fehlgeschlagen' : 'alle bestanden'}`)
process.exit(fails ? 1 : 0)
