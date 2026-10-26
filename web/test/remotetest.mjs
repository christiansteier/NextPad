// Browsertest gegen einen entfernten Pilot ohne API-Schlüssel: Sessions kommen aus der Kennungsdatei.
// node test/remotetest.mjs <ids-datei>   (groupID|padID|sessionA|sessionB|readOnlyID), NEXTPAD_URL = Basis mit Pfad
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
const [G, P, SA, SB, RO] = readFileSync(process.argv[2], 'utf8').trim().split('|')
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const host = new URL(base).hostname
const browser = await chromium.launch(); const errors = []
async function open(sid, id) {
  const ctx = await browser.newContext(); await ctx.addCookies([{ name: 'sessionID', value: sid, domain: host, path: '/' }])
  const page = await ctx.newPage()
  page.on('pageerror', e => errors.push('pageerror: ' + e.message)); page.on('console', m => { if (m.type() === 'error') errors.push('console: ' + m.text()) })
  const r = await page.goto(`${base}/p/${id}`); console.log('GET /p/… →', r.status(), 'CSP:', (r.headers()['content-security-policy'] || '').slice(0, 60))
  await page.waitForSelector('.ProseMirror'); await page.waitForFunction(() => document.querySelector('#statusbar .ok') !== null, null, { timeout: 15000 })
  return page
}
const pa = await open(SA, P), pb = await open(SB, P)
console.log('A sieht Vorbestand:', JSON.stringify((await pa.evaluate(() => document.querySelector('.ProseMirror').innerText)).slice(0, 50)))
await pa.evaluate(() => window.nextpad.editor.commands.focus('end')); await pa.waitForTimeout(300); await pa.keyboard.press('Enter'); await pa.keyboard.type('Gruß von Anna über nginx', { delay: 20 })
await pb.waitForFunction(() => document.querySelector('.ProseMirror').innerText.includes('Gruß von Anna über nginx'), null, { timeout: 15000 }); console.log('B sieht Annas Text: ja')
await pb.waitForTimeout(500); await pb.evaluate(() => window.nextpad.editor.commands.focus('end')); await pb.waitForTimeout(300); await pb.keyboard.press('Enter'); await pb.keyboard.type('## Antwort von Ben', { delay: 20 })
await pa.waitForFunction(() => [...document.querySelectorAll('.ProseMirror h2')].some(h => h.innerText.includes('Antwort von Ben')), null, { timeout: 15000 }); console.log('A sieht Bens Überschrift: ja')
console.log('Autoren-IDs im Pad:', new Set(await pa.evaluate(() => [...document.querySelectorAll('.ProseMirror span.author')].map(s => s.dataset.author))).size)
console.log('Teilnehmende laut Statuszeile:', await pa.evaluate(() => [...document.querySelectorAll('#statusbar .user')].map(u => u.textContent)))
const pr = await open(SA, RO); console.log('Lesemodus:', await pr.evaluate(() => document.querySelector('.ProseMirror').getAttribute('contenteditable')))
const noCookie = await (await browser.newContext()).newPage(); const r = await noCookie.goto(`${base}/p/${P}`); console.log('ohne Cookie →', r.status())
await browser.close(); console.log('Fehler im Browser:', errors.length ? errors : 'keine')
