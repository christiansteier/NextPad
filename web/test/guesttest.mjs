// Freie Pads: zwei Gäste ohne Moodle öffnen dasselbe Pad, tippen, ändern ihren Namen.
// node test/guesttest.mjs   (NEXTPAD_URL = Basis mit Pfad; Server mit NEXTPAD_ALLOW_FREE_PADS=true)
import { chromium } from 'playwright'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const browser = await chromium.launch(); const errors = []
async function guest() {
  const ctx = await browser.newContext(); const page = await ctx.newPage()
  page.on('pageerror', e => errors.push(e.message.slice(0, 150)))
  return page
}
const pa = await guest()
await pa.goto(`${base}/`); check('Startseite', (await pa.title()).includes('NextPad'), await pa.title())
await pa.goto(`${base}/new`); await pa.waitForSelector('.ProseMirror'); await pa.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
const padUrl = pa.url(); const gast = await pa.evaluate(() => document.querySelector('#statusbar input.myname')?.value)
check('neues Pad mit Gast-Name', padUrl.includes('/p/') && !!gast, `${padUrl.replace(base, '')} ${gast}`)
await pa.click('button[title="Teilnehmende"]'); await pa.fill('#statusbar input.myname', 'Gast Anna'); await pa.press('#statusbar input.myname', 'Enter'); await pa.waitForTimeout(300); await pa.click('.ProseMirror')
const pb = await guest(); await pb.goto(padUrl); await pb.waitForSelector('.ProseMirror'); await pb.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
await pa.evaluate(() => window.nextpad.editor.commands.focus('end')); await pa.waitForTimeout(200); await pa.keyboard.type('Hallo von Gast Anna', { delay: 15 })
await pb.waitForFunction(() => document.querySelector('.ProseMirror').innerText.includes('Hallo von Gast Anna'), null, { timeout: 10000 }); check('Gast B sieht Annas Text', true)
await pb.evaluate(() => window.nextpad.editor.commands.focus('end')); await pb.waitForTimeout(200); await pb.keyboard.press('Enter'); await pb.keyboard.type('Antwort von Gast B', { delay: 15 })
await pa.waitForFunction(() => document.querySelector('.ProseMirror').innerText.includes('Antwort von Gast B'), null, { timeout: 10000 }); check('Gast A sieht Bs Text', true)
const users = await pa.evaluate(() => [...document.querySelectorAll('#statusbar .user')].map(u => u.textContent))
check('Teilnehmende bei A: zwei, mit neuem Namen', users.length === 2 && users.some(u => u.includes('Gast Anna')), JSON.stringify(users))
const ids = new Set(await pa.evaluate(() => [...document.querySelectorAll('.ProseMirror span.author')].map(s => s.dataset.author)))
check('zwei verschiedene Autoren', ids.size === 2, String(ids.size))
await pa.reload(); await pa.waitForSelector('.ProseMirror'); await pa.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
const name = await pa.evaluate(() => document.querySelector('#statusbar input.myname')?.value)
check('Name nach Neuladen erhalten', name === 'Gast Anna', name)
// per Name öffnen
const pc = await guest(); await pc.goto(`${base}/`); await pc.fill('#padname', 'Gruppe Blau'); await pc.press('#padname', 'Enter'); await pc.waitForSelector('.ProseMirror'); check('per Name geöffnet', pc.url().endsWith('/p/Gruppe%20Blau'), pc.url().replace(base, ''))
await browser.close(); check('keine Fehler im Browser', !errors.length, errors.join(' / '))
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
