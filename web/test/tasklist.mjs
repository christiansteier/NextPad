// Aufgabenlisten: Kürzel beim Tippen, Markdown-Einfügen, Abhaken mit Sync, Exporte, Rückweg getHTML → setHTML.
// node test/tasklist.mjs <basis-url> <api-schlüssel-datei>
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
const [base, keyFile] = process.argv.slice(2)
const key = readFileSync(keyFile, 'utf8').split('\n').find(l => l.trim() && !l.startsWith('#')).trim().split(/\s+/)[0]
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const api = async (fn, p) => (await (await fetch(`${base}/api/1.2.15/${fn}`, { method: 'POST', body: new URLSearchParams({ apikey: key, ...p }) })).json())
const b = await chromium.launch()
const A = await (await b.newContext()).newPage(), B = await (await b.newContext()).newPage()
await A.goto(`${base}/new`); await A.waitForSelector('.ProseMirror'); const url = A.url(); const pad = decodeURIComponent(url.split('/p/')[1])
await B.goto(url); await B.waitForSelector('.ProseMirror')
await A.click('.ProseMirror'); await A.keyboard.type('[ ] Folien vorbereiten'); await A.keyboard.press('Enter'); await A.keyboard.type('Quellen sammeln')
await A.waitForTimeout(500)
check('Kürzel [ ] erzeugt Aufgabenliste', await A.locator('ul[data-type="taskList"] li').count() === 2, String(await A.locator('ul[data-type="taskList"] li').count()))
await B.waitForFunction(() => document.querySelectorAll('ul[data-type="taskList"] li').length === 2, null, { timeout: 5000 }).catch(() => {})
check('B sieht die Aufgaben', await B.locator('ul[data-type="taskList"] li').count() === 2)
await A.locator('ul[data-type="taskList"] li').first().locator('input[type=checkbox]').click()
await B.waitForFunction(() => document.querySelector('ul[data-type="taskList"] li')?.getAttribute('data-checked') === 'true', null, { timeout: 5000 }).catch(() => {})
check('Abhaken kommt bei B an', await B.locator('ul[data-type="taskList"] li').first().getAttribute('data-checked') === 'true')
// Markdown einfügen über die Zwischenablage
await A.keyboard.press('Control+End'); await A.keyboard.press('Enter'); await A.keyboard.press('Enter')
await A.evaluate(() => { const dt = new DataTransfer(); dt.setData('text/plain', '- [ ] Protokoll schreiben\n- [x] Raum buchen'); document.querySelector('.ProseMirror').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })) })
await A.waitForTimeout(600)
const items = await A.locator('ul[data-type="taskList"] li').count()
const checked = await A.locator('ul[data-type="taskList"] li[data-checked="true"]').count()
check('Markdown-Einfügen erzeugt Aufgaben (4, davon 2 erledigt)', items === 4 && checked === 2, `${items}/${checked}`)
await A.waitForTimeout(1500)
const md = await (await fetch(`${base}/p/${encodeURIComponent(pad)}/export/md`)).text()
check('Markdown-Export', md.includes('- [x] Folien vorbereiten') && md.includes('- [ ] Quellen sammeln') && md.includes('- [x] Raum buchen'), md.slice(0, 200))
const txt = (await api('getText', { padID: pad })).data?.text || ''
check('Text-Export', txt.includes('- [x] Folien') && txt.includes('- [ ] Quellen'), txt.slice(0, 120))
const html = (await api('getHTML', { padID: pad })).data?.html || ''
check('HTML-Export mit Kontrollkästchen', html.includes('data-type="taskList"') && html.includes('data-checked="true"') && html.includes('type="checkbox"'))
const pdf = await fetch(`${base}/p/${encodeURIComponent(pad)}/export/pdf`)
check('PDF-Export', pdf.status === 200 && (await pdf.arrayBuffer()).byteLength > 5000)
// Rückweg wie bei Moodle-Sicherung: HTML in neues Pad
const pad2 = 'tasklist-roundtrip-' + Date.now()
await api('createPad', { padID: pad2 }); const sr = await api('setHTML', { padID: pad2, html })
const md2 = await (await fetch(`${base}/p/${encodeURIComponent(pad2)}/export/md`)).text()
check('Rückweg getHTML → setHTML erhält Häkchen', sr.code === 0 && md2.includes('- [x] Folien vorbereiten') && md2.includes('- [ ] Quellen sammeln'), md2.slice(0, 160))
await api('deletePad', { padID: pad2 }); await api('deletePad', { padID: pad })
await b.close(); console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
