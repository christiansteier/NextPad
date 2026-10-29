// Einrücken: Tab und die Knöpfe rücken Absätze ein, in Listen verschachteln sie weiter. Prüft auch Abgleich und Export.
// NEXTPAD_URL=… NEXTPAD_KEY_FILE=… node test/einruecktest.mjs
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const api = async (fn, args) => (await (await fetch(`${base}/api/1.2/${fn}`, { method: 'POST', body: new URLSearchParams({ apikey: key, ...args }) })).json()).data
const G = (await api('createGroup', {})).groupID
const P = (await api('createGroupPad', { groupID: G, padName: 'einr' + Date.now() })).padID
const A = (await api('createAuthorIfNotExistsFor', { authorMapper: 'einr1', name: 'Einrück-Test' })).authorID
const S = (await api('createSession', { groupID: G, authorID: A, validUntil: String(Math.floor(Date.now() / 1000) + 3600) })).sessionID
const browser = await chromium.launch()
const open = async () => {
  const ctx = await browser.newContext()
  await ctx.addCookies([{ name: 'sessionID', value: S, domain: new URL(base).hostname, path: '/' }])
  const p = await ctx.newPage()
  await p.goto(`${base}/p/${P}`); await p.waitForSelector('.ProseMirror'); await p.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
  return p
}
const page = await open(); const errors = []; page.on('pageerror', e => errors.push(e.message.slice(0, 150)))
// Erst tippen, wenn der Editor den Fokus hat: nach focus() und nach Klicks auf die Werkzeugleiste kommt er einen Tick später
const imText = () => page.waitForFunction(() => document.activeElement?.classList.contains('ProseMirror'), null, { timeout: 5000 })
const indents = () => page.evaluate(() => [...document.querySelectorAll('.ProseMirror > p')].map(p => p.getAttribute('data-indent') || '0').join(','))
await page.evaluate(() => window.nextpad.editor.commands.focus('end')); await imText(); await page.waitForTimeout(150)
await page.keyboard.type('Erster Absatz'); await page.keyboard.press('Enter'); await page.keyboard.type('Zweiter Absatz')
await page.keyboard.press('Tab')
check('Tab rückt den Absatz ein', await indents() === '0,1', await indents())
check('Fokus bleibt im Text', await page.evaluate(() => document.activeElement.classList.contains('ProseMirror')))
const pad = await page.evaluate(() => getComputedStyle(document.querySelectorAll('.ProseMirror > p')[1]).paddingLeft)
check('eingerückt um 2em', pad === '30px', pad)
await page.keyboard.press('Tab'); await page.keyboard.press('Shift+Tab')
check('Tab, dann Umschalt+Tab: Stufe 1', await indents() === '0,1', await indents())
await page.click('button[title^="Einrücken"]')
check('Knopf Einrücken: Stufe 2', await indents() === '0,2', await indents())
await page.click('button[title^="Ausrücken"]'); await page.click('button[title^="Ausrücken"]'); await page.click('button[title^="Ausrücken"]')
check('Ausrücken geht nicht unter 0', await indents() === '0,0', await indents())
await imText(); await page.keyboard.press('Tab')
// markierte Absätze rücken gemeinsam ein
await page.evaluate(() => window.nextpad.editor.commands.selectAll()); await imText(); await page.keyboard.press('Tab')
check('Markierung: alle Absätze eine Stufe weiter', await indents() === '1,2', await indents())
await page.evaluate(() => window.nextpad.editor.commands.focus('end')); await imText(); await page.waitForTimeout(150)
await page.keyboard.press('Enter'); await page.keyboard.type('- Punkt eins'); await page.keyboard.press('Enter'); await page.keyboard.type('Punkt zwei')
await page.keyboard.press('Tab')
check('in der Liste verschachtelt Tab weiter', await page.evaluate(() => !!document.querySelector('.ProseMirror ul ul li')?.textContent.includes('Punkt zwei')))
await page.click('button[title^="Ausrücken"]')
check('in der Liste holt Ausrücken zurück', await page.evaluate(() => !document.querySelector('.ProseMirror ul ul')))
await page.waitForTimeout(800)
const other = await open()
const seen = await other.evaluate(() => [...document.querySelectorAll('.ProseMirror > p')].map(p => p.getAttribute('data-indent') || '0').join(','))
check('zweites Fenster sieht die Einrückung', seen.startsWith('1,2'), seen)
const html = await (await fetch(`${base}/p/${encodeURIComponent(P)}/export/html`, { headers: { Cookie: 'sessionID=' + S } })).text()
check('HTML-Export rückt ein', /<p style="padding-left:4em"><span [^>]*>Zweiter Absatz/.test(html), html.slice(0, 300))
check('Listenpunkt aus eingerücktem Absatz ohne Einrückung', /<li><p><span [^>]*>Punkt eins/.test(html), html.slice(0, 400))
const txt = await (await fetch(`${base}/p/${encodeURIComponent(P)}/export/txt`, { headers: { Cookie: 'sessionID=' + S } })).text()
check('Text-Export rückt mit Leerzeichen ein', txt.includes('        Zweiter Absatz'), JSON.stringify(txt.slice(0, 120)))
const pdf = await fetch(`${base}/p/${encodeURIComponent(P)}/export/pdf`, { headers: { Cookie: 'sessionID=' + S } })
const head = Buffer.from(await pdf.arrayBuffer()).subarray(0, 5).toString()
check('PDF-Export mit Einrückung gelingt', pdf.ok && head === '%PDF-', `${pdf.status} ${head}`)
check('keine Fehler auf der Seite', !errors.length, errors.join(' / '))
await browser.close()
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
