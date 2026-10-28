// Aufgaben per Markdown: „- [ ] “, „[ ] “ und „[x] “ getippt sowie eingefügt werden zu Aufgaben zum Abhaken.
// NEXTPAD_URL=… NEXTPAD_KEY_FILE=… node test/aufgabentest.mjs
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const api = async (fn, args) => (await (await fetch(`${base}/api/1.2/${fn}`, { method: 'POST', body: new URLSearchParams({ apikey: key, ...args }) })).json()).data
const G = (await api('createGroup', {})).groupID
const P = (await api('createGroupPad', { groupID: G, padName: 'aufg' + Date.now() })).padID
const A = (await api('createAuthorIfNotExistsFor', { authorMapper: 'aufg1', name: 'Aufgaben-Test' })).authorID
const S = (await api('createSession', { groupID: G, authorID: A, validUntil: String(Math.floor(Date.now() / 1000) + 3600) })).sessionID
const browser = await chromium.launch(); const ctx = await browser.newContext()
await ctx.addCookies([{ name: 'sessionID', value: S, domain: new URL(base).hostname, path: '/' }])
const page = await ctx.newPage(); const errors = []; page.on('pageerror', e => errors.push(e.message.slice(0, 150)))
await page.goto(`${base}/p/${P}`); await page.waitForSelector('.ProseMirror'); await page.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
// aus dem Dokument gelesen, nicht aus der Ansicht (die Kästchen zeichnet der Editor selbst)
const items = () => page.evaluate(() => [...new DOMParser().parseFromString(window.nextpad.editor.getHTML(), 'text/html').querySelectorAll('li[data-type="taskItem"]')].map(li => `${li.dataset.checked === 'true' ? 'x' : ' '}:${li.textContent.trim()}`))
const ende = async () => { for (let i = 0; i < 3; i++) await page.keyboard.press('Enter') }
// wie im Markdown-Test: kurz warten, bis der erste Abgleich durch ist (sonst gehen die ersten Zeichen verloren)
await page.evaluate(() => window.nextpad.editor.commands.focus('end')); await page.waitForTimeout(200)
await page.keyboard.type('- [ ] mit Strich'); await page.keyboard.press('Enter'); await page.keyboard.type('zweite Aufgabe'); await ende()
let it = await items()
check('„- [ ] “ getippt wird zur Aufgabe', it[0] === ' :mit Strich', it.join(' | '))
check('Enter darin legt die nächste Aufgabe an', it[1] === ' :zweite Aufgabe', it.join(' | '))
check('kein Rest „[ ]“ im Text', !(await page.evaluate(() => document.querySelector('.ProseMirror').textContent)).includes('[ ]'))
await page.keyboard.type('- [x] erledigt mit Strich'); await ende()
await page.keyboard.type('[ ] ohne Strich'); await ende()
await page.keyboard.type('[x] erledigt ohne Strich'); await ende()
it = await items()
check('„- [x] “ getippt ist erledigt', it.includes('x:erledigt mit Strich'), it.join(' | '))
check('„[ ] “ und „[x] “ ohne Strich', it.includes(' :ohne Strich') && it.includes('x:erledigt ohne Strich'), it.join(' | '))
await page.keyboard.type('- normaler Punkt'); await ende()
check('normale Aufzählung bleibt Aufzählung', await page.evaluate(() => [...document.querySelectorAll('.ProseMirror ul:not([data-type]) li')].some(li => li.textContent.includes('normaler Punkt'))))
await page.evaluate(() => { const dt = new DataTransfer(); dt.setData('text/plain', '- [ ] eingefügt offen\n- [x] eingefügt erledigt'); document.querySelector('.ProseMirror').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })) })
await page.waitForTimeout(400)
it = await items()
check('eingefügtes Markdown wird zu Aufgaben', it.includes(' :eingefügt offen') && it.includes('x:eingefügt erledigt'), it.join(' | '))
await page.waitForTimeout(600)
const md = await (await fetch(`${base}/p/${encodeURIComponent(P)}/export/md`, { headers: { Cookie: 'sessionID=' + S } })).text()
check('Markdown-Export mit - [ ] und - [x]', md.includes('- [ ] mit Strich') && md.includes('- [x] erledigt mit Strich'), md.slice(0, 300))
check('keine Fehler auf der Seite', !errors.length, errors.join(' / '))
await browser.close()
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
