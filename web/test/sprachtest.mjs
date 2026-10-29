// Sprachwahl: eigene Wahl vor Browsersprache, sonst Deutsch; Fehlerseiten, Ablehnungen, Startseite und Hilfe in beiden Sprachen.
// node test/sprachtest.mjs <basis-url>
import { chromium } from 'playwright'
const base = process.argv[2] || 'http://127.0.0.1:5095'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const get = async (p, headers = {}) => { const r = await fetch(base + p, { headers, redirect: 'manual' }); return { r, text: await r.text() } }
const pfad = new URL(base).pathname.replace(/\/$/, '')

// Fehlerseite nach Browsersprache
let s = await get('/gibt-es-nicht')
check('ohne Angabe deutsch', s.r.status === 404 && s.text.includes('<html lang="de"') && s.text.includes('Nicht gefunden') && s.text.includes('Zur Startseite von NextPad'))
s = await get('/gibt-es-nicht', { 'accept-language': 'en-GB,en;q=0.9' })
check('Browser englisch: Fehlerseite englisch', s.text.includes('<html lang="en"') && s.text.includes('Not found') && s.text.includes('This page does not exist.'))
s = await get('/gibt-es-nicht', { 'accept-language': 'fr-FR,fr;q=0.9' })
check('andere Sprache: deutsch', s.text.includes('<html lang="de"'))
s = await get('/p/r.gibtsnicht/history', { 'accept-language': 'en' })
check('Ablehnung für Skripte englisch', s.r.status === 404 && s.text === 'This pad does not exist.', s.text)
s = await get('/p/r.gibtsnicht/history')
check('Ablehnung für Skripte deutsch', s.text === 'Dieses Pad gibt es nicht.', s.text)

// eigene Wahl
s = await get(`/sprache/en?zurueck=${encodeURIComponent(pfad + '/hilfe')}`)
const c = s.r.headers.getSetCookie().find(x => x.startsWith('nextpad-lang=')) || ''
check('Wahl: zurück zur Seite', s.r.status === 303 && s.r.headers.get('location') === pfad + '/hilfe', `${s.r.status} ${s.r.headers.get('location')}`)
check('Wahl: Cookie ein Jahr, auch eingebettet', c.startsWith('nextpad-lang=en;') && /Max-Age=31536000/.test(c) && /SameSite=None/.test(c) && /Secure/.test(c) && /HttpOnly/.test(c), c)
s = await get('/gibt-es-nicht', { 'accept-language': 'en', cookie: 'nextpad-lang=de' })
check('eigene Wahl schlägt Browsersprache', s.text.includes('<html lang="de"'))
s = await get('/gibt-es-nicht', { 'accept-language': 'de', cookie: 'nextpad-lang-moodle=en' })
check('Moodle-Sprache schlägt Browsersprache', s.text.includes('<html lang="en"'))
for (const ziel of ['https://evil.example/', '//evil.example/', '/\\evil.example']) {
  s = await get(`/sprache/de?zurueck=${encodeURIComponent(ziel)}`)
  check(`fremdes Ziel ${ziel}: Startseite`, s.r.headers.get('location') === pfad + '/', s.r.headers.get('location'))
}
s = await get('/sprache/fr')
check('unbekannte Sprache 404', s.r.status === 404)

// Startseite und Hilfe
s = await get('/')
check('Startseite deutsch', s.text.includes('<html lang="de"') && s.text.includes('Öffnen oder anlegen') && s.text.includes('Zum Inhalt springen'))
s = await get('/', { 'accept-language': 'en' })
check('Startseite englisch', s.text.includes('<html lang="en"') && s.text.includes('Open or create') && s.text.includes('Skip to content') && s.text.includes('Idea collection') && !s.text.includes('Öffnen'))
const js = JSON.parse((s.text.match(/<script type="application\/json" id="start-texte">([^<]*)<\/script>/) || [])[1] || '{}')
check('Texte für start.js englisch', js.gestern === 'yesterday' && js.min === '{n} min ago', JSON.stringify(js))
s = await get('/hilfe')
check('Hilfe deutsch', s.text.includes('<html lang="de"') && s.text.includes('Hilfe zu NextPad') && s.text.includes('Einem Pad beitreten'))
s = await get('/hilfe', { 'accept-language': 'en' })
check('Hilfe englisch, mit Addons', s.text.includes('<html lang="en"') && s.text.includes('NextPad help') && s.text.includes('<h2>Idea collection</h2>') && s.text.includes('<h2>Quiz grid</h2>') && !s.text.includes('Einem Pad'))

// im Browser: englischer Browser, dann umschalten
const b = await chromium.launch()
const ctx = await b.newContext({ locale: 'en-US' }); const p = await ctx.newPage()
await p.goto(base + '/gibt-es-nicht')
check('Browser: englisch nach Gerät', await p.locator('h1').textContent() === 'Not found' && await p.getAttribute('html', 'lang') === 'en')
await p.goto(`${base}/sprache/de?zurueck=${encodeURIComponent(pfad + '/gibt-es-nicht')}`)
check('Browser: Wahl wirkt sofort', await p.locator('h1').textContent() === 'Nicht gefunden' && await p.getAttribute('html', 'lang') === 'de')
await p.goto(base + '/gibt-es-nicht')
check('Browser: Wahl bleibt gemerkt', await p.locator('h1').textContent() === 'Nicht gefunden')

// Startseite im englischen Browser: Prüfung im Browser und Vorlage englisch
const p2 = await (await b.newContext({ locale: 'en-US' })).newPage()
await p2.goto(base + '/')
await p2.fill('#padname', 'g.test'); await p2.press('#padname', 'Enter')
check('Startseite: Feldprüfung englisch', (await p2.textContent('#padname-error')).includes('must not start'), await p2.textContent('#padname-error'))
await p2.fill('#padname', 'a/b'); await p2.press('#padname', 'Enter')
check('Startseite: ungültige Zeichen englisch', (await p2.textContent('#padname-error')).includes('Allowed are'))
await p2.click('a[href$="vorlage=gruppenarbeit"]'); await p2.waitForSelector('.ProseMirror')
await p2.waitForFunction(() => document.querySelector('.ProseMirror')?.textContent.includes('Who does what'), null, { timeout: 8000 }).catch(() => {})
check('Vorlage englisch', (await p2.textContent('.ProseMirror')).includes('Who does what'))

// Editor: Seite, Werkzeugleiste, Teilnehmende und Datum in der Sprache des Browsers
check('Editor englisch', await p2.getAttribute('html', 'lang') === 'en' && await p2.getAttribute('#toolbar', 'aria-label') === 'Formatting' && await p2.locator('button[title="Bold (Ctrl+B)"]').count() === 1 && await p2.locator('button[title="Participants"]').count() === 1)
await p2.click('button[title="Participants"]')
check('Editor: Teilnehmende englisch', (await p2.textContent('.popup.right')).includes('My name'))
// Einladen ohne Recht auf die Zwischenablage: Link markiert zum Selbstkopieren
await p2.click('button[title="Share and export"]')
check('Einladen englisch', await p2.locator('.einladen button', { hasText: 'Copy link' }).count() === 1 && (await p2.textContent('.einladen .hinweis')) === 'Whoever opens the link can write along.')
await p2.evaluate(() => { Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('gesperrt')) }, configurable: true }) })
await p2.click('.einladen button:has-text("Copy link")')
await p2.waitForSelector('.einladen input.link', { timeout: 4000 }).catch(() => {})
const feld = p2.locator('.einladen input.link')
check('Einladen: ohne Zwischenablage Link im Feld', await feld.count() === 1 && (await feld.inputValue()) === new URL(p2.url()).origin + new URL(p2.url()).pathname && (await p2.textContent('.einladen .status')).startsWith('Copying was not possible'), await feld.count() ? await feld.inputValue() : 'kein Feld')
check('Einladen: Ziele mindestens 44 px', (await p2.locator('.einladen button').first().boundingBox()).height >= 44 && (await p2.locator('.popup a').first().boundingBox()).height >= 44)

// Umschalter: im Editor zurück ins selbe Pad, auf der Hilfe zurück auf die Hilfe; Seiten, die noch deutsch sind, ohne Umschalter
const padUrl = p2.url()
check('Umschalter im Editor', await p2.locator('#toolbar .sprachwahl a[aria-current="true"]').textContent() === 'EN')
await p2.click('#toolbar .sprachwahl a[hreflang="de"]'); await p2.waitForSelector('.ProseMirror')
check('Umschalter: selbes Pad, jetzt deutsch', p2.url() === padUrl && await p2.locator('button[title="Fett (Strg+B)"]').count() === 1, p2.url())
await p2.goto(base + '/hilfe')
check('Umschalter auf der Hilfe: Wahl gemerkt', await p2.locator('.sprachwahl a[aria-current="true"]').textContent() === 'DE' && await p2.locator('.sprachwahl a').count() === 2)
await p2.click('.sprachwahl a[hreflang="en"]'); await p2.waitForSelector('h1')
check('Umschalter: Hilfe englisch', new URL(p2.url()).pathname === pfad + '/hilfe' && await p2.locator('h1').textContent() === 'NextPad help', p2.url())
await p2.goto(base + '/new?typ=ideen'); await p2.waitForSelector('#add-form:not([hidden])', { timeout: 8000 }).catch(() => {})
check('Ideensammlung englisch mit Umschalter', await p2.getAttribute('html', 'lang') === 'en' && (await p2.textContent('#prompt')) === 'Ideas and questions' && (await p2.textContent('#add-hint')).startsWith('No name.') && await p2.locator('.sprachwahl a[aria-current="true"]').textContent() === 'EN')
await p2.fill('#add-text', '   '); await p2.click('#add-form button[type=submit]')
check('Ideensammlung: Meldung im Browser englisch', (await p2.textContent('#add-error')) === 'Please enter an idea or question.')
await p2.fill('#add-text', 'Erste Idee'); await p2.click('#add-form button[type=submit]')
await p2.waitForFunction(() => /open entry/.test(document.getElementById('ideas-count')?.textContent || ''), null, { timeout: 8000 }).catch(() => {})
check('Ideensammlung: Zähler englisch', (await p2.textContent('#ideas-count')) === '1 open entry', await p2.textContent('#ideas-count'))
const ideenPfad = new URL(p2.url()).pathname
const wand = await p2.request.get(base + ideenPfad.replace(pfad, '') + '/leinwand')
check('Leinwand englisch', (await wand.text()).includes('<html lang="en"') && (await wand.text()).includes('Board · NextPad'))
const abl = await p2.request.post(base + ideenPfad.replace(pfad, '') + '/ideen', { data: { text: ' ' }, headers: { origin: new URL(base).origin } })
check('Ideensammlung: Ablehnung englisch', abl.status() === 400 && (await abl.text()) === 'Please enter a text.', `${abl.status()} ${await abl.text()}`)
await p2.click('.sprachwahl a[hreflang="de"]'); await p2.waitForSelector('#add-form:not([hidden])')
check('Ideensammlung: zurück auf Deutsch, selbe Sammlung', new URL(p2.url()).pathname === ideenPfad && (await p2.textContent('#ideas-count')) === '1 offener Beitrag')

// Quiz-Raster auf Englisch: Leitung, Tabellenprüfung, Raster
const p4 = await (await b.newContext({ locale: 'en-GB' })).newPage()
await p4.goto(base + '/new?typ=quiz'); await p4.waitForSelector('#setup:not([hidden])', { timeout: 8000 }).catch(() => {})
check('Quiz englisch', await p4.getAttribute('html', 'lang') === 'en' && (await p4.textContent('#quiz-kicker')) === 'Quiz grid · Lead' && (await p4.textContent('#setup-h')) === 'Questions and teams' && await p4.locator('.sprachwahl').count() === 1)
await p4.fill('#in-teams', 'A, B'); await p4.fill('#in-table', 'nur eine Spalte'); p4.once('dialog', d => d.dismiss()); await p4.click('#q-save')
await p4.waitForSelector('#setup-error:not([hidden])', { timeout: 8000 }).catch(() => {})
check('Quiz: Tabellenprüfung englisch', (await p4.textContent('#setup-error')).startsWith('Line has fewer than four columns'), await p4.textContent('#setup-error'))
await p4.fill('#in-table', '| Category | Points | Question | Answer |\n| --- | --- | --- | --- |\n| Audio | 100 | How long? | 5 min |'); p4.once('dialog', d => d.dismiss()); await p4.click('#q-save')
await p4.waitForSelector('.quiz-cell', { timeout: 8000 }).catch(() => {})
check('Quiz: englische Kopfzeile übersprungen, Raster englisch', await p4.locator('.quiz-cell').count() === 1 && (await p4.getAttribute('.quiz-cell', 'aria-label')) === 'Audio, 100 points' && (await p4.textContent('#q-start')) === 'Start game')
const quizPfad = new URL(p4.url()).pathname
await p4.goto(new URL(p4.url()).origin + quizPfad + '/messung'); await p4.waitForSelector('#m-lead:not([hidden])', { timeout: 8000 }).catch(() => {})
await p4.waitForFunction(() => (document.getElementById('m-urteil')?.textContent || '').length > 0, null, { timeout: 8000 }).catch(() => {})
check('Messung englisch', await p4.getAttribute('html', 'lang') === 'en' && (await p4.textContent('h1')) === 'Buzzer measurement' && (await p4.textContent('#m-urteil')).startsWith('Not enough data yet') && (await p4.textContent('#m-signal')) === 'Send signal', await p4.textContent('#m-urteil'))
const quizMd = await p4.request.get(base + quizPfad.replace(pfad, '') + '/export/md', { headers: { 'accept-language': 'en-GB' } })
check('Quiz-Export englisch', (await quizMd.text()).includes('\nScores\n'), await quizMd.text())

// Exporte in der Sprache der Anfrage: Druckansicht, Ideen-CSV mit Komma, PDF
const druck = await p4.request.get(base + new URL(padUrl).pathname.replace(pfad, '') + '/export/print', { headers: { 'accept-language': 'en-GB' } })
const druckText = await druck.text()
check('Druckansicht englisch', druckText.includes('<html lang="en"') && druckText.includes('Print view.') && druckText.includes('Back to the pad'), `${druck.status()} ${druckText.slice(0, 300)}`)
const csvEn = await p4.request.get(base + ideenPfad.replace(pfad, '') + '/export/csv', { headers: { 'accept-language': 'en-GB' } })
check('Ideen-CSV englisch, mit Komma', (await csvEn.text()).startsWith('\ufeffNo,Entry,Votes,Done\r\n1,"Erste Idee",0,no') && /ideas\.csv/.test(csvEn.headers()['content-disposition']), await csvEn.text())
const ctx3 = await b.newContext({ locale: 'de-DE' }); await ctx3.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: new URL(base).origin }); const p3 = await ctx3.newPage()
const csvDe = await p3.request.get(base + ideenPfad.replace(pfad, '') + '/export/csv')
check('Ideen-CSV deutsch, mit Semikolon', (await csvDe.text()).startsWith('\ufeffNr;Beitrag;Stimmen;Erledigt\r\n1;"Erste Idee";0;nein') && /ideen\.csv/.test(csvDe.headers()['content-disposition']), await csvDe.text())
const pdfEn = await p4.request.get(base + new URL(padUrl).pathname.replace(pfad, '') + '/export/pdf', { headers: { 'accept-language': 'en-GB' } })
check('PDF-Export englisch liefert PDF', pdfEn.status() === 200 && (await pdfEn.body()).subarray(0, 5).toString() === '%PDF-', String(pdfEn.status()))
await p3.goto(base + '/open?padname=sprachtest-editor'); await p3.waitForSelector('.ProseMirror')
check('Editor deutsch', await p3.getAttribute('html', 'lang') === 'de' && await p3.locator('button[title="Fett (Strg+B)"]').count() === 1 && await p3.locator('button[title="Teilnehmende"]').count() === 1)
await p3.click('button[title="Teilen und exportieren"]'); await p3.click('.einladen button:has-text("Link kopieren")')
await p3.waitForFunction(() => document.querySelector('.einladen .status')?.textContent === 'Link kopiert', null, { timeout: 4000 }).catch(() => {})
const ablage = await p3.evaluate(() => navigator.clipboard.readText()).catch(e => String(e))
check('Einladen: Link in der Zwischenablage, Rückmeldung', ablage === new URL(p3.url()).origin + new URL(p3.url()).pathname && (await p3.textContent('.einladen .status')) === 'Link kopiert', ablage)
await b.close()
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
