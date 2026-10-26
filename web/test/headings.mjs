// Überschriften 1–4 im Editor: Bildschirmfoto und berechnete Schrift/Größe je Stufe.
import { chromium } from 'playwright'
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5091'; const out = process.argv[2]
const b = await chromium.launch(); const p = await (await b.newContext({ viewport: { width: 1200, height: 700 } })).newPage()
await p.goto(`${base}/open?padname=${encodeURIComponent('Ueberschriften Probe ' + Date.now())}`); await p.waitForSelector('.ProseMirror'); await p.waitForTimeout(600)
await p.evaluate(() => window.nextpad.editor.commands.setContent('<h1>Überschrift 1</h1><p>Fließtext darunter.</p><h2>Überschrift 2</h2><p>Fließtext.</p><h3>Überschrift 3</h3><p>Fließtext.</p><h4>Überschrift 4</h4><p>Fließtext.</p>'))
await p.waitForTimeout(800)
for (const t of ['h1', 'h2', 'h3', 'h4', 'p']) console.log(t, await p.evaluate(t => { const e = document.querySelector('.ProseMirror ' + t); const c = getComputedStyle(e); return `${c.fontFamily.split(',')[0]} ${c.fontWeight} ${c.fontSize}` }, t))
console.log('HSD Sans geladen:', await p.evaluate(async () => { await document.fonts.ready; return document.fonts.check('40px "HSD Sans"') }))
await p.screenshot({ path: out + '/cp-headings.png' }); await b.close()
