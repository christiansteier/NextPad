// Freie Pads: zwei Gäste ohne Moodle öffnen dasselbe Pad, tippen, ändern ihren Namen.
// node test/guesttest.mjs   (NEXTPAD_URL = Basis mit Pfad; Server mit NEXTPAD_ALLOW_FREE_PADS=true)
import { chromium } from 'playwright'
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const browser = await chromium.launch(); const errors = []
async function guest() {
  const ctx = await browser.newContext(); const page = await ctx.newPage()
  page.on('pageerror', e => errors.push(e.message.slice(0, 150)))
  return page
}
const pa = await guest()
await pa.goto(`${base}/`); console.log('Startseite:', await pa.title())
await pa.click('a[href$="/new"]'); await pa.waitForSelector('.ProseMirror'); await pa.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
const padUrl = pa.url(); console.log('Neues Pad:', padUrl.replace(base, ''), '| Gast-Name:', await pa.evaluate(() => document.querySelector('#statusbar input.myname')?.value))
await pa.click('button[title="Teilnehmende"]'); await pa.fill('#statusbar input.myname', 'Gast Anna'); await pa.press('#statusbar input.myname', 'Enter'); await pa.waitForTimeout(300); await pa.click('.ProseMirror')
const pb = await guest(); await pb.goto(padUrl); await pb.waitForSelector('.ProseMirror'); await pb.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
await pa.evaluate(() => window.nextpad.editor.commands.focus('end')); await pa.waitForTimeout(200); await pa.keyboard.type('Hallo von Gast Anna', { delay: 15 })
await pb.waitForFunction(() => document.querySelector('.ProseMirror').innerText.includes('Hallo von Gast Anna'), null, { timeout: 10000 }); console.log('Gast B sieht Annas Text: ja')
await pb.evaluate(() => window.nextpad.editor.commands.focus('end')); await pb.waitForTimeout(200); await pb.keyboard.press('Enter'); await pb.keyboard.type('Antwort von Gast B', { delay: 15 })
await pa.waitForFunction(() => document.querySelector('.ProseMirror').innerText.includes('Antwort von Gast B'), null, { timeout: 10000 }); console.log('Gast A sieht Bs Text: ja')
console.log('Teilnehmende bei A:', await pa.evaluate(() => [...document.querySelectorAll('#statusbar .user')].map(u => u.textContent)))
console.log('verschiedene Autoren-IDs:', new Set(await pa.evaluate(() => [...document.querySelectorAll('.ProseMirror span.author')].map(s => s.dataset.author))).size)
await pa.reload(); await pa.waitForSelector('.ProseMirror'); await pa.waitForFunction(() => document.querySelector('#statusbar .ok') !== null)
console.log('Name nach Neuladen erhalten:', await pa.evaluate(() => document.querySelector('#statusbar input.myname')?.value))
// per Name öffnen
const pc = await guest(); await pc.goto(`${base}/`); await pc.fill('#padname', 'Gruppe Blau'); await pc.press('#padname', 'Enter'); await pc.waitForSelector('.ProseMirror'); console.log('per Name geöffnet:', pc.url().replace(base, ''))
await browser.close(); console.log('Fehler:', errors.length ? errors : 'keine')
