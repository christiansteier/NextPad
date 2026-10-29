// Markdown als Eingabe: Einfügen aus der Zwischenablage (nur text/plain) und der Dialog.
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
const api = async (fn, args) => (await (await fetch(`${base}/api/1.2/${fn}`, { method: 'POST', body: new URLSearchParams({ apikey: key, ...args }) })).json()).data
// Gruppe und Pad legt der Test selbst an, damit er ohne Vorlagedatei läuft
const G = (await api('createGroup', {})).groupID
const P = (await api('createGroupPad', { groupID: G, padName: 'md' + Date.now() })).padID
const a = await (await fetch(`${base}/api/1.2/createAuthorIfNotExistsFor`, { method: 'POST', body: new URLSearchParams({ apikey: key, authorMapper: 'md1', name: 'Md Tester' }) })).json()
const s = await (await fetch(`${base}/api/1.2/createSession`, { method: 'POST', body: new URLSearchParams({ apikey: key, groupID: G, authorID: a.data.authorID, validUntil: String(Math.floor(Date.now() / 1000) + 3600) }) })).json()
const browser = await chromium.launch(); const ctx = await browser.newContext()
await ctx.addCookies([{ name: 'sessionID', value: s.data.sessionID, domain: new URL(base).hostname, path: '/' }])
const page = await ctx.newPage(); const errors = []; page.on('pageerror', e => errors.push(e.message.slice(0, 150)))
await page.goto(`${base}/p/${P}`); await page.waitForSelector('.ProseMirror'); await page.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
await page.evaluate(() => window.nextpad.editor.commands.focus('end')); await page.waitForTimeout(200); await page.keyboard.press('Enter')
// 1. Einfügen simulieren (nur text/plain)
await page.evaluate(() => { const dt = new DataTransfer(); dt.setData('text/plain', '# Aus der Zwischenablage\n\n- erster Punkt\n- **fetter** Punkt\n\n1. nummeriert\n\nEin *kursiver* Satz mit `Code`.'); const ev = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }); document.querySelector('.ProseMirror').dispatchEvent(ev) })
await page.waitForTimeout(400)
const html = await page.evaluate(() => window.nextpad.editor.getHTML())
check('Einfügen: Überschrift', /<h1[ >]/.test(html) && html.includes('Aus der Zwischenablage'))
check('Einfügen: Listen', html.includes('<ul>') && html.includes('<ol>'))
check('Einfügen: fett und kursiv', html.includes('<strong>') && html.includes('<em>'))
// 2. Dialog
await page.click('button[title^="Markdown-Text einfügen"]'); await page.waitForSelector('#mddialog textarea')
await page.fill('#mddialog textarea', '## Aus dem Dialog\n\n- Dialogpunkt'); await page.click('#mddialog button[value="ok"]'); await page.waitForTimeout(300)
const html2 = await page.evaluate(() => window.nextpad.editor.getHTML())
check('Dialog: Überschrift und Liste', /<h2[ >]/.test(html2) && html2.includes('Aus dem Dialog') && html2.includes('Dialogpunkt'))
// 3. Klartext ohne Markdown bleibt Klartext
await page.evaluate(() => { const dt = new DataTransfer(); dt.setData('text/plain', 'Nur ein Satz ohne Auszeichnung.'); document.querySelector('.ProseMirror').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })) })
await page.waitForTimeout(300)
check('Klartext unverändert', (await page.evaluate(() => window.nextpad.editor.getText())).includes('Nur ein Satz ohne Auszeichnung.'))
check('Markdown-Export enthält die Überschrift', (await (await fetch(`${base}/p/${encodeURIComponent(P)}/export/md`, { headers: { Cookie: 'sessionID=' + s.data.sessionID } })).text()).includes('# Aus der Zwischenablage'))
await browser.close(); check('keine Fehler im Browser', !errors.length, errors.join(' / '))
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
