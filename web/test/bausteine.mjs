// Tabellen, Bewertungen und Inhaltsverzeichnis: zwei Personen im selben Pad, Export vom Server.
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const shots = process.env.SHOTS || ''
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
const api = async (fn, args) => (await (await fetch(`${base}/api/1.2/${fn}`, { method: 'POST', body: new URLSearchParams({ apikey: key, ...args }) })).json()).data
const G = (await api('createGroup', {})).groupID
const P = (await api('createGroupPad', { groupID: G, padName: 'bs' + Date.now() })).padID
async function person(browser, mapper, name) {
  const a = await api('createAuthorIfNotExistsFor', { authorMapper: mapper, name })
  const s = await api('createSession', { groupID: G, authorID: a.authorID, validUntil: String(Math.floor(Date.now() / 1000) + 3600) })
  const ctx = await browser.newContext({ viewport: { width: 1400, height: 900 } })
  await ctx.addCookies([{ name: 'sessionID', value: s.sessionID, domain: new URL(base).hostname, path: '/' }])
  const page = await ctx.newPage(); page.errors = []; page.on('pageerror', e => page.errors.push(e.message.slice(0, 200)))
  await page.goto(`${base}/p/${P}?nocaret=1`); await page.waitForSelector('.ProseMirror'); await page.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
  page.sid = s.sessionID
  return page
}
const paste = (page, text) => page.evaluate(t => { const dt = new DataTransfer(); dt.setData('text/plain', t); document.querySelector('.ProseMirror').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })) }, text)
const ok = (label, cond) => { console.log((cond ? 'ok   ' : 'FEHL ') + label); if (!cond) process.exitCode = 1 }

const browser = await chromium.launch(process.env.PW_CHANNEL ? { channel: process.env.PW_CHANNEL } : {})
const A = await person(browser, 'bs1', 'Anna'), B = await person(browser, 'bs2', 'Ben')
await A.evaluate(() => window.nextpad.editor.commands.focus('end'))
await paste(A, '# Planung\n\n## Ideen\n\nText davor.\n\n| Idee | Aufwand |\n| --- | --- |\n| Exkursion | hoch |\n| Quiz | gering |\n\n## Termine\n')
await A.waitForTimeout(300)
const h1 = await A.evaluate(() => window.nextpad.editor.getHTML())
ok('Markdown-Tabelle aus der Zwischenablage wird Tabelle', /<table[\s\S]*<th[\s\S]*Idee[\s\S]*<td[\s\S]*Exkursion/.test(h1))

// Tabelle über die Werkzeugleiste, Zeile anfügen über die Auswahl
await A.evaluate(() => window.nextpad.editor.commands.focus('end'))
ok('Tabellen-Auswahl außerhalb einer Tabelle ausgegraut', await A.$eval('select[title="Tabelle bearbeiten"]', s => s.disabled))
await A.click('button[title^="Tabelle einfügen"]'); await A.waitForTimeout(150)
ok('Tabellen-Auswahl in der Tabelle aktiv', !(await A.$eval('select[title="Tabelle bearbeiten"]', s => s.disabled)))
await A.selectOption('select[title="Tabelle bearbeiten"]', 'rowAfter'); await A.waitForTimeout(150)
const rows = await A.evaluate(() => { const t = document.querySelectorAll('.ProseMirror table'); return t[t.length - 1].querySelectorAll('tr').length })
ok('neue Tabelle 3 × 3, eine Zeile angefügt → 4 Zeilen', rows === 4)

// Bewertung: A stimmt hoch, B runter, B nimmt zurück und stimmt hoch
await A.evaluate(() => window.nextpad.editor.commands.focus('end'))
await A.keyboard.type('Vorschlag Exkursion ')
await A.click('button[title^="Bewertung einfügen"]'); await A.waitForTimeout(300)
await B.waitForSelector('.ProseMirror .rating')
await A.click('.ProseMirror .rating .rate.up'); await B.click('.ProseMirror .rating .rate.down'); await A.waitForTimeout(400)
const count = p => p.$eval('.ProseMirror .rating', r => r.querySelector('.up .n').textContent + '/' + r.querySelector('.down .n').textContent)
ok('Stimmen bei A: 1 hoch, 1 runter', (await count(A)) === '1/1')
ok('Stimmen bei B: 1 hoch, 1 runter', (await count(B)) === '1/1')
ok('B sieht die eigene Stimme als gedrückt', await B.$eval('.ProseMirror .rating .down', b => b.getAttribute('aria-pressed') === 'true'))
await B.click('.ProseMirror .rating .rate.up'); await B.waitForTimeout(400)
ok('B wechselt auf hoch: 2 hoch, 0 runter', (await count(A)) === '2/0')
await B.click('.ProseMirror .rating .rate.up'); await B.waitForTimeout(400)
ok('B nimmt die Stimme zurück: 1/0', (await count(A)) === '1/0')

// Inhaltsverzeichnis: Seitenleiste und Baustein
await A.click('button[title="Inhaltsverzeichnis ein/aus"]'); await A.waitForTimeout(200)
const side = await A.$$eval('#tocbar li a', as => as.map(a => a.textContent))
ok('Seitenleiste listet die Überschriften', JSON.stringify(side) === JSON.stringify(['Planung', 'Ideen', 'Termine']))
await A.evaluate(() => window.nextpad.editor.commands.focus('start'))
await A.click('#tocbar .toc-insert'); await A.waitForTimeout(300)
await B.waitForSelector('.ProseMirror nav.toc li a')
ok('Baustein erscheint bei B mit Überschriften', (await B.$$eval('.ProseMirror nav.toc li a', as => as.length)) === 3)
await B.evaluate(() => window.nextpad.editor.commands.focus('end')); await B.keyboard.press('Enter'); await B.keyboard.type('Fazit')
await B.evaluate(() => window.nextpad.editor.chain().setHeading({ level: 2 }).run()); await A.waitForTimeout(500)
ok('neue Überschrift erscheint im Baustein und in der Seitenleiste', (await A.$$eval('.ProseMirror nav.toc li a', as => as.length)) === 4 && (await A.$$eval('#tocbar li a', as => as.length)) === 4)
if (shots) { await A.screenshot({ path: shots + '/hell.png' }); await A.click('button[title="Dunkler Modus ein/aus"]'); await A.waitForTimeout(200); await A.screenshot({ path: shots + '/dunkel.png' }); await A.click('button[title="Dunkler Modus ein/aus"]'); await A.setViewportSize({ width: 420, height: 800 }); await A.waitForTimeout(200); await A.screenshot({ path: shots + '/schmal.png' }); await A.setViewportSize({ width: 1400, height: 900 }) }

// Export vom Server
await A.waitForTimeout(500)
const exp = async fmt => (await fetch(`${base}/p/${encodeURIComponent(P)}/export/${fmt}`, { headers: { Cookie: 'sessionID=' + A.sid } }))
const md = await (await exp('md')).text()
ok('Markdown-Export: Tabelle', md.includes('| Idee | Aufwand |\n| --- | --- |\n| Exkursion | hoch |'))
ok('Markdown-Export: Bewertung', md.includes('Vorschlag Exkursion 👍 1 · 👎 0'))
ok('Markdown-Export: Inhaltsverzeichnis', md.includes('**Inhaltsverzeichnis**') && md.includes('  - Fazit'))
const html = await (await exp('html')).text()
ok('HTML-Export: Sprungmarken und Stimmen', html.includes('href="#abschnitt-4"') && /data-votes="[^"]+:1"/.test(html))
const pdf = await exp('pdf')
ok('PDF-Export gelingt', pdf.ok && (await pdf.arrayBuffer()).byteLength > 1000)
if (shots) { const fs = await import('node:fs'); fs.writeFileSync(shots + '/export.pdf', Buffer.from(await (await exp('pdf')).arrayBuffer())); fs.writeFileSync(shots + '/export.md', md) }

// Kopieren einer Bewertung erzeugt eine neue Kennung
const ids = await A.evaluate(() => { const ed = window.nextpad.editor; const ids = []; ed.state.doc.descendants(n => { if (n.type.name === 'rating') ids.push(n.attrs.id) }); return ids })
await A.evaluate(() => { const dt = new DataTransfer(); dt.setData('text/html', document.querySelector('.ProseMirror p .rating').closest('p').outerHTML.replace(/<button[\s\S]*<\/button>/, '')); window.nextpad.editor.commands.focus('end'); document.querySelector('.ProseMirror').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })) })
await A.waitForTimeout(200)
const ids2 = await A.evaluate(() => { const ids = []; window.nextpad.editor.state.doc.descendants(n => { if (n.type.name === 'rating') ids.push(n.attrs.id) }); return ids })
ok('eingefügte Bewertung hat eigene Kennung', ids2.length === ids.length + 1 && new Set(ids2).size === ids2.length)

ok('keine Skriptfehler', !A.errors.length && !B.errors.length); if (A.errors.length || B.errors.length) console.log(A.errors, B.errors)
await browser.close()
