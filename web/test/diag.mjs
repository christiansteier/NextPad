// Diagnose für den Editor: Fehler, Autoren-Marks in Y und PM, Tippen, Markdown-Überschrift.
// node test/diag.mjs <ids-datei>
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
const [G, P] = readFileSync(process.argv[2], 'utf8').trim().split('|')
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
const mapper = String(9000 + Math.floor(Math.random() * 1000))
const a = await (await fetch(`${base}/api/1.2/createAuthorIfNotExistsFor`, { method: 'POST', body: new URLSearchParams({ apikey: key, authorMapper: mapper, name: 'Diag ' + mapper }) })).json()
const s = await (await fetch(`${base}/api/1.2/createSession`, { method: 'POST', body: new URLSearchParams({ apikey: key, groupID: G, authorID: a.data.authorID, validUntil: String(Math.floor(Date.now() / 1000) + 3600) }) })).json()
const browser = await chromium.launch(); const ctx = await browser.newContext()
await ctx.addCookies([{ name: 'sessionID', value: s.data.sessionID, domain: new URL(base).hostname, path: '/' }])
const page = await ctx.newPage()
page.on('pageerror', e => console.log('pageerror:', e.message.slice(0, 120), '@', (e.stack || '').split('\n')[1]?.trim().slice(0, 100)))
page.on('console', m => { if (m.type() === 'error') console.log('console.error:', m.text().slice(0, 200)) })
await page.goto(`${base}/p/${P}`); await page.waitForSelector('.ProseMirror'); await page.waitForTimeout(1200)
const ids = () => page.evaluate(() => JSON.stringify({
  y: [...new Set([...window.nextpad.ydoc.getXmlFragment('default').toString().matchAll(/author id="([^"]+)"/g)].map(m => m[1]))],
  pm: (() => { const r = new Set(); window.nextpad.editor.state.doc.descendants(n => n.marks.forEach(m => { if (m.type.name === 'author') r.add(m.attrs.id) })); return [...r] })(),
}))
console.log('ich bin', a.data.authorID); console.log('nach Laden:', await ids())
await page.evaluate(() => window.nextpad.editor.commands.focus('end')); await page.keyboard.type('Q'); await page.waitForTimeout(400)
console.log('nach Tippen Q:', await ids())
console.log('Y Ende:', await page.evaluate(() => window.nextpad.ydoc.getXmlFragment('default').toString().slice(-160)))
await page.keyboard.press('Enter'); await page.keyboard.type('# Kopf'); await page.waitForTimeout(400)
console.log('nach "# Kopf":', await page.evaluate(() => window.nextpad.editor.getHTML().replace(/ class="author"[^>]*?(?=>)/g, '').slice(-200)))
console.log('Y Ende:', await page.evaluate(() => window.nextpad.ydoc.getXmlFragment('default').toString().slice(-160)))
const html = await (await fetch(`${base}/api/1.2/getHTML?apikey=${key}&padID=${encodeURIComponent(P)}`)).json()
console.log('Server Ende:', html.data.html.slice(-200))
await browser.close()
