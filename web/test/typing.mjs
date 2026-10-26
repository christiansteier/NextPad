// Tippverlust-Probe: B tippt N Zeichen ohne Pause, während A verbunden ist. Zählt, wie viele ankommen.
// node test/typing.mjs <ids-datei> [chars=40] [delay=0] [suffix='']   (suffix z. B. '?nocaret=1')
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
const [G, P] = readFileSync(process.argv[2], 'utf8').trim().split('|')
const N = +(process.argv[3] || 40), DELAY = +(process.argv[4] || 0), SUFFIX = process.argv[5] || ''
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
  await page.goto(`${base}/p/${P}${SUFFIX}`); await page.waitForSelector('.ProseMirror'); await page.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
  return page
}
const pa = await open(await session('t1', 'Tipp A')), pb = await open(await session('t2', 'Tipp B'))
if (process.env.PRE) { // A tippt zuerst in dasselbe Pad, B wartet auf Ankunft
  await pa.evaluate(() => window.nextpad.editor.commands.focus('end')); await pa.keyboard.press('Enter'); await pa.keyboard.type('Vorlauf von A ')
  if (process.env.PRE !== 'plain') { await pa.keyboard.press('Control+b'); await pa.keyboard.type('fett'); await pa.keyboard.press('Control+b') } else { await pa.keyboard.type('fett') }
  await pb.waitForFunction(() => document.querySelector('.ProseMirror').innerText.includes('Vorlauf von A fett'))
  if (process.env.PRE === 'wait') await pb.waitForTimeout(800)
}
await pb.evaluate(() => window.nextpad.editor.commands.focus('end')); await pb.waitForTimeout(+(process.env.GAP ?? 150)); // Pause nach Fokus, sonst gehen die ersten Tastendrücke nach einem Fremd-Update verloren (Testartefakt, GAP=0 zum Nachstellen) await pb.keyboard.press('Enter')
const marker = 'x' + Date.now().toString(36)
await pb.keyboard.type(marker + 'abcdefghijklmnopqrstuvwxyz'.repeat(3).slice(0, N), { delay: DELAY })
await pb.waitForTimeout(1500)
const countB = await pb.evaluate(m => { const t = document.querySelector('.ProseMirror').innerText; const i = t.indexOf(m); return i < 0 ? -1 : t.slice(i + m.length).split('\n')[0].length }, marker)
const countA = await pa.evaluate(m => { const t = document.querySelector('.ProseMirror').innerText; const i = t.indexOf(m); return i < 0 ? -1 : t.slice(i + m.length).split('\n')[0].length }, marker)
console.log(`delay ${DELAY} ms${SUFFIX}: getippt ${N}, bei B angekommen ${countB}, bei A angekommen ${countA}`)
if (countB !== N) console.log('  B-Dokument Ende:', JSON.stringify((await pb.evaluate(() => document.querySelector('.ProseMirror').innerText)).slice(-140)))
await browser.close()
