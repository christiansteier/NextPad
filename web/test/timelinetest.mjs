// Zeitleiste: A tippt mehrere Schritte, B öffnet die Zeitleiste, prüft Schritte, Regler, Inhalt alter Stände, Autor.
// node test/timelinetest.mjs <ids-datei>
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const [G, P] = readFileSync(process.argv[2], 'utf8').trim().split('|')
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
async function session(m, name) {
  const a = await (await fetch(`${base}/api/1.2/createAuthorIfNotExistsFor`, { method: 'POST', body: new URLSearchParams({ apikey: key, authorMapper: m, name }) })).json()
  const s = await (await fetch(`${base}/api/1.2/createSession`, { method: 'POST', body: new URLSearchParams({ apikey: key, groupID: G, authorID: a.data.authorID, validUntil: String(Math.floor(Date.now() / 1000) + 3600) }) })).json()
  return s.data.sessionID
}
const browser = await chromium.launch(); const errors = []
async function open(sid) {
  const ctx = await browser.newContext(); await ctx.addCookies([{ name: 'sessionID', value: sid, domain: new URL(base).hostname, path: '/' }])
  const page = await ctx.newPage(); page.on('pageerror', e => errors.push(e.message.slice(0, 160)))
  await page.goto(`${base}/p/${P}`); await page.waitForSelector('.ProseMirror'); await page.waitForFunction(() => document.querySelector('#statusbar .ok') !== null); return page
}
const pa = await open(await session('tl1', 'Zeit Anna'))
await pa.evaluate(() => window.nextpad.editor.commands.focus('end')); await pa.waitForTimeout(150)
const marker = 'tl' + Date.now().toString(36)
await pa.keyboard.press('Enter'); await pa.keyboard.type(marker + ' erster Satz', { delay: 15 }); await pa.keyboard.press('Enter'); await pa.keyboard.type('# Später eine Überschrift', { delay: 15 })
await pa.waitForTimeout(800)
const hist = await (await fetch(`${base}/p/${encodeURIComponent(P)}/history`, { headers: { Cookie: 'sessionID=' + await session('tl1', 'Zeit Anna') } })).json()
check('Verlauf mit Schritten und Autor', hist.updates.length > 0 && !!hist.updates.at(-1)?.author, `updates ${hist.updates.length}, snapshots ${hist.snapshots.length}`)
const pb = await open(await session('tl2', 'Zeit Ben'))
await pb.click('button[title="Frühere Stände ansehen"]')
await pb.waitForSelector('#timeline .tl-slider')
const max = await pb.evaluate(() => +document.querySelector('#timeline .tl-slider').max)
const last = await pb.evaluate(() => document.querySelector('#timeline .snapshot').innerText)
check('letzter Stand enthält die Überschrift', last.includes('Später eine Überschrift'), `Schritte ${max + 1}`)
const info = await pb.evaluate(() => document.querySelector('#timeline .tl-info').textContent)
check('Info nennt die Autorin', info.includes('Zeit Anna'), info)
// Regler auf die Mitte der eigenen Eingabe: Marker da, Überschrift noch nicht
await pb.evaluate(m => { const s = document.querySelector('#timeline .tl-slider'); s.value = String(m); s.dispatchEvent(new Event('input')) }, Math.max(0, max - 12))
const mid = await pb.evaluate(() => document.querySelector('#timeline .snapshot').innerText)
check('mittlerer Stand: Marker da, Überschrift noch nicht', mid.includes(marker) && !mid.includes('Später eine Überschrift'))
await pb.evaluate(() => { const s = document.querySelector('#timeline .tl-slider'); s.value = '0'; s.dispatchEvent(new Event('input')) })
const first = await pb.evaluate(() => document.querySelector('#timeline .snapshot').innerText)
check('erster Stand ohne Marker', !first.includes(marker))
const pos0 = await pb.evaluate(() => document.querySelector('#timeline .tl-pos').textContent)
await pb.click('#timeline .tl-play'); await pb.waitForTimeout(1500)
const pos1 = await pb.evaluate(() => document.querySelector('#timeline .tl-pos').textContent)
check('Abspielen läuft weiter', pos1 !== pos0, `${pos0} -> ${pos1}`)
await browser.close(); check('keine Fehler im Browser', !errors.length, errors.join(' / '))
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
