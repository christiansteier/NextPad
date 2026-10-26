// Profilbilder: Moodle-Person (Schlüssel mit Moodle-Adresse) mit Bild, Moodle-Gast als „Gast“.
// node test/avatartest.mjs <schlüsseldatei mit zwei Zeilen: key1, key2 moodle-url> <ausgabeverzeichnis>
import { chromium } from 'playwright'
import { readFileSync } from 'node:fs'
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5092'
const [k1, k2line] = readFileSync(process.argv[2], 'utf8').trim().split('\n'); const k2 = k2line.split(/\s+/)[0]
const post = async (key, fn, p) => (await (await fetch(`${base}/api/1.2/${fn}`, { method: 'POST', body: new URLSearchParams({ ...p, apikey: key }) })).json()).data
const G = (await post(k2, 'createGroup', {})).groupID; const P = (await post(k2, 'createGroupPad', { groupID: G, padName: 'mdl' })).padID
const vu = String(Math.floor(Date.now() / 1000) + 3600)
const aM = (await post(k2, 'createAuthorIfNotExistsFor', { authorMapper: '999999', name: 'Maria Moodle' })).authorID
const aG = (await post(k2, 'createAuthor', { name: 'Guest-12345' })).authorID
const sM = (await post(k2, 'createSession', { groupID: G, authorID: aM, validUntil: vu })).sessionID
const sG = (await post(k2, 'createSession', { groupID: G, authorID: aG, validUntil: vu })).sessionID
const b = await chromium.launch(); const errs = []
async function open(sid) { const c = await b.newContext({ viewport: { width: 1200, height: 600 } }); await c.addCookies([{ name: 'sessionID', value: sid, domain: '127.0.0.1', path: '/' }]); const p = await c.newPage(); p.on('pageerror', e => errs.push(e.message)); p.on('console', m => { if (m.type() === 'error') errs.push(m.text().slice(0, 120)) }); await p.goto(`${base}/p/${P}`); await p.waitForFunction(() => document.querySelector('#statusbar .ok')); return p }
const pm = await open(sM), pg = await open(sG)
console.log('Moodle-Person avatar:', await pm.evaluate(() => JSON.parse(document.getElementById('nextpad-config').textContent).author.avatar))
console.log('Gast avatar:', await pg.evaluate(() => JSON.parse(document.getElementById('nextpad-config').textContent).author.avatar), '| guest-Flag:', await pg.evaluate(() => JSON.parse(document.getElementById('nextpad-config').textContent).author.guest))
await pm.evaluate(() => window.nextpad.editor.commands.focus('end')); await pm.waitForTimeout(200); await pm.keyboard.type('Text von Maria', { delay: 20 })
await pg.evaluate(() => window.nextpad.editor.commands.focus('end')); await pg.waitForTimeout(200); await pg.keyboard.type(' und vom Gast', { delay: 20 })
await pg.waitForTimeout(800); await pm.waitForTimeout(500)
console.log('Cursor bei Gast zeigt Maria mit Bild:', await pg.evaluate(() => [...document.querySelectorAll('.collaboration-carets__label')].map(l => l.textContent + (l.querySelector('img') ? ' [Bild]' : ''))))
console.log('Cursor bei Maria zeigt Gast:', await pm.evaluate(() => [...document.querySelectorAll('.collaboration-carets__label')].map(l => l.textContent + (l.querySelector('img') ? ' [Bild]' : ''))))
await pm.click('button[title="Teilnehmende"]'); await pm.waitForTimeout(800)
console.log('Teilnehmendenliste:', await pm.evaluate(() => [...document.querySelectorAll('.popup .user')].map(u => u.textContent + (u.querySelector('img') ? ' [Bild]' : ''))))
await pm.screenshot({ path: process.argv[3] + '/cp-avatar.png' }); await pg.screenshot({ path: process.argv[3] + '/cp-avatar-gast.png' })
await b.close(); console.log('Fehler:', errs.length ? errs : 'keine')
