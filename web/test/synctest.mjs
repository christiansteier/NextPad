// Zwei Browser, ein Pad (150 ms Pause zwischen Fokus und Tippen: direkt nach einem Fremd-Update greift die DOM-Auswahl erst einen Tick später): tippen in A, lesen in B; Autoren-Mark, Markdown-Eingabe, Neuladen, Lesemodus.
// Aufruf aus dem Bau-Verzeichnis (siehe build.sh): node test/synctest.mjs <ids-datei>
// ids-datei: groupID|padID|authorID|sessionID|readOnlyID (aus einem API-Lauf)
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const [G, P, AU, SID, RO] = readFileSync(process.argv[2], 'utf8').trim().split('|')
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
async function session(authorMapper, name) {
  const fd = new URLSearchParams({ apikey: key, authorMapper, name })
  const a = await (await fetch(`${base}/api/1.2/createAuthorIfNotExistsFor`, { method: 'POST', body: fd })).json()
  const fs = new URLSearchParams({ apikey: key, groupID: G, authorID: a.data.authorID, validUntil: String(Math.floor(Date.now() / 1000) + 3600) })
  const s = await (await fetch(`${base}/api/1.2/createSession`, { method: 'POST', body: fs })).json()
  return { author: a.data.authorID, session: s.data.sessionID }
}
const browser = await chromium.launch()
const errors = []
async function open(sid, id = P) {
  const ctx = await browser.newContext()
  await ctx.addCookies([{ name: 'sessionID', value: sid, domain: new URL(base).hostname, path: '/' }])
  const page = await ctx.newPage()
  page.on('pageerror', e => errors.push('pageerror: ' + e.message))
  page.on('console', m => { if (m.type() === 'error') errors.push('console: ' + m.text()) })
  await page.goto(`${base}/p/${id}`)
  await page.waitForSelector('.ProseMirror')
  await page.waitForFunction(() => document.querySelector('#statusbar .ok') !== null, null, { timeout: 10000 })
  return page
}
const a = await session('1001', 'Anna Alpha'), b = await session('1002', 'Ben Beta')
const pa = await open(a.session), pb = await open(b.session)
const before = await pb.evaluate(() => document.querySelector('.ProseMirror').innerText)
console.log('B sieht Vorbestand:', JSON.stringify(before.slice(0, 60)))
await pa.evaluate(() => window.nextpad.editor.commands.focus('end')); await pa.waitForTimeout(150); await pa.keyboard.press('Enter'); await pa.keyboard.type('Hallo von Anna ')
await pa.keyboard.press('Control+b'); await pa.keyboard.type('fett'); await pa.keyboard.press('Control+b')
await pb.waitForFunction(() => document.querySelector('.ProseMirror').innerText.includes('Hallo von Anna fett'), null, { timeout: 10000 })
check('B sieht Annas Text', true)
await pb.evaluate(() => window.nextpad.editor.commands.focus('end')); await pb.waitForTimeout(150); await pb.keyboard.press('Enter'); await pb.keyboard.type('# Antwort von Ben')
try {
  await pa.waitForFunction(() => [...document.querySelectorAll('.ProseMirror h1')].some(h => h.innerText.includes('Antwort von Ben')), null, { timeout: 10000 })
  check('A sieht Bens Markdown-Überschrift', true)
} catch (e) {
  check('A sieht Bens Markdown-Überschrift', false)
  console.log('  B-HTML Ende:', await pb.evaluate(() => window.nextpad.editor.getHTML().replace(/ class="author"[^>]*?(?=>)/g, '').slice(-260)))
  console.log('  Fehler bisher:', errors)
  console.log('  A-HTML Ende:', await pa.evaluate(() => window.nextpad.editor.getHTML().replace(/ class="author"[^>]*?(?=>)/g, '').slice(-260)))
}
const marks = await pa.evaluate(() => [...document.querySelectorAll('.ProseMirror span.author')].map(s => s.dataset.author))
check('Autoren-Marks von zwei Personen', new Set(marks).size === 2, JSON.stringify(Object.fromEntries([...new Set(marks)].map(id => [id, marks.filter(m => m === id).length]))))
check('fett angekommen', await pa.evaluate(() => !!document.querySelector('.ProseMirror strong')))
const users = await pa.evaluate(() => [...document.querySelectorAll('#statusbar .user')].map(u => u.textContent))
check('beide in der Teilnehmerliste', users.some(u => u.includes('Anna Alpha')) && users.some(u => u.includes('Ben Beta')), JSON.stringify(users))
await pa.reload(); await pa.waitForSelector('.ProseMirror'); await pa.waitForFunction(() => document.querySelector('.ProseMirror').innerText.includes('Antwort von Ben'), null, { timeout: 10000 })
check('nach Neuladen vorhanden', true)
const pr = await open(a.session, RO)
const editable = await pr.evaluate(() => document.querySelector('.ProseMirror').getAttribute('contenteditable'))
check('Lesemodus nicht bearbeitbar', editable === 'false', editable)
const txt = await (await fetch(`${base}/api/1.2/getText?apikey=${key}&padID=${encodeURIComponent(P)}`)).json()
check('getText enthält beide Beiträge', txt.data.text.includes('Hallo von Anna fett') && txt.data.text.includes('Antwort von Ben'), JSON.stringify(txt.data.text))
const html = await (await fetch(`${base}/api/1.2/getHTML?apikey=${key}&padID=${encodeURIComponent(P)}`)).json()
check('getHTML mit Autor und Überschrift', html.data.html.includes('data-author') && /<h1[ >][^]*Antwort von Ben/.test(html.data.html), JSON.stringify(html.data.html.slice(-400)))
await browser.close()
check('keine Fehler im Browser', !errors.length, errors.join(' / '))
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
