// Bildschirmfotos für den Designvergleich: Startseite, Pad, Export-Menü, Teilnehmende, Zeitleiste.
// node test/shot2.mjs <ausgabeverzeichnis>   (NEXTPAD_URL, Server mit freien Pads)
import { chromium } from 'playwright'
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5091'; const out = process.argv[2]
const b = await chromium.launch(); const p = await (await b.newContext({ viewport: { width: 1200, height: 800 } })).newPage()
p.on('pageerror', e => console.log('pageerror', e.message.slice(0, 120)))
await p.goto(`${base}/`); await p.waitForTimeout(800); await p.screenshot({ path: out + '/cp-index.png' })
await p.fill('#padname', 'Design Probe'); await p.press('#padname', 'Enter'); await p.waitForSelector('.ProseMirror'); await p.waitForTimeout(800)
await p.evaluate(() => window.nextpad.editor.commands.setContent('<h1>So sieht das für alle aus!</h1><p>Ein Absatz mit <strong>fett</strong>, <em>kursiv</em> und <u>unterstrichen</u>.</p><ul><li>Punkt eins</li><li>Punkt zwei</li></ul><p>Etherpad on Github: https://github.com/ether/etherpad-lite</p>'))
await p.waitForTimeout(500); await p.screenshot({ path: out + '/cp-pad.png' })
await p.click('button[title="Exportieren"]'); await p.waitForTimeout(300); await p.screenshot({ path: out + '/cp-pad-export.png' })
await p.click('button[title="Teilnehmende"]'); await p.waitForTimeout(300); await p.screenshot({ path: out + '/cp-pad-users.png' })
await p.click('button[title="Frühere Stände ansehen"]'); await p.waitForTimeout(800); await p.screenshot({ path: out + '/cp-pad-timeline.png' })
await b.close(); console.log('fertig')
