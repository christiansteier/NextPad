// Gleichzeitiges Tippen: A und B tippen im selben Pad parallel; zählt angekommene Zeichen je Marker bei beiden.
// node test/typing2.mjs <ids-datei> [chars=60] [delay=0]
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
const [G, P] = readFileSync(process.argv[2], 'utf8').trim().split('|')
const N = +(process.argv[3] || 60), DELAY = +(process.argv[4] || 0)
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
async function session(m, name) {
  const a = await (await fetch(`${base}/api/1.2/createAuthorIfNotExistsFor`, { method: 'POST', body: new URLSearchParams({ apikey: key, authorMapper: m, name }) })).json()
  const s = await (await fetch(`${base}/api/1.2/createSession`, { method: 'POST', body: new URLSearchParams({ apikey: key, groupID: G, authorID: a.data.authorID, validUntil: String(Math.floor(Date.now() / 1000) + 3600) }) })).json()
  return s.data.sessionID
}
const browser = await chromium.launch()
async function open(sid) {
  const ctx = await browser.newContext(); await ctx.addCookies([{ name: 'sessionID', value: sid, domain: new URL(base).hostname, path: '/' }])
  const page = await ctx.newPage(); page.on('pageerror', e => console.log('pageerror', e.message.slice(0, 100)))
  await page.goto(`${base}/p/${P}`); await page.waitForSelector('.ProseMirror'); await page.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
  return page
}
const pa = await open(await session('c1', 'Gleich A')), pb = await open(await session('c2', 'Gleich B'))
// A tippt am Ende, B in einem neuen Absatz davor (eigener Absatz je Person)
await pa.evaluate(() => window.nextpad.editor.commands.focus('end')); await pa.keyboard.press('Enter')
await pb.evaluate(() => window.nextpad.editor.commands.focus('start')); await pb.keyboard.press('Enter'); await pb.keyboard.press('ArrowUp')
await pa.waitForTimeout(500); await pb.waitForTimeout(500)
const ma = 'A' + Date.now().toString(36), mb = 'B' + Date.now().toString(36)
const text = 'abcdefghijklmnopqrstuvwxyz'.repeat(5).slice(0, N)
await Promise.all([pa.keyboard.type(ma + text, { delay: DELAY }), pb.keyboard.type(mb + text, { delay: DELAY })])
await pa.waitForTimeout(2000)
const count = (page, m) => page.evaluate(m => { const t = document.querySelector('.ProseMirror').innerText; const i = t.indexOf(m); return i < 0 ? -1 : t.slice(i + m.length).split('\n')[0].length }, m)
console.log(`gleichzeitig, delay ${DELAY} ms, je ${N} Zeichen: A-Text bei A ${await count(pa, ma)} bei B ${await count(pb, ma)}; B-Text bei A ${await count(pa, mb)} bei B ${await count(pb, mb)}`)
await browser.close()
