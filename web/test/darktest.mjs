// Dunkler Modus im Pad: Umschalter, Merken im Browser, Geräteeinstellung, Kontraste (axe)
// node darktest.mjs <base> [screenshot-präfix]
import { chromium } from 'playwright'
import AxeBuilder from '@axe-core/playwright'
const base = process.argv[2] || 'http://127.0.0.1:5093', shot = process.argv[3]
const b = await chromium.launch(); let ok = 0, fail = 0
const check = (n, c, x = '') => { c ? ok++ : fail++; console.log((c ? 'ok   ' : 'FAIL ') + n + (c ? '' : ' ' + x)) }
const theme = p => p.evaluate(() => document.documentElement.dataset.theme)
let ctx = await b.newContext({ viewport: { width: 1280, height: 800 }, colorScheme: 'light' })
let p = await ctx.newPage(); await p.goto(base + '/open?padname=dunkeltest-' + Date.now().toString().slice(-5)); await p.waitForSelector('.ProseMirror')
await p.click('.ProseMirror'); await p.keyboard.type('# Überschrift\nText mit Autorenfarbe\n[ ] Aufgabe offen\n')
check('hell nach Gerät', await theme(p) === 'light')
await p.click('button[title="Dunkler Modus ein/aus"]')
check('Umschalter: dunkel', await theme(p) === 'dark' && await p.getAttribute('button[title="Dunkler Modus ein/aus"]', 'aria-pressed') === 'true')
const bg = await p.evaluate(() => getComputedStyle(document.getElementById('editor')).backgroundColor)
check('Editorfläche dunkel', bg === 'rgb(30, 30, 30)', bg)
if (shot) await p.screenshot({ path: shot + '-dunkel.png' })
const axe = await new AxeBuilder({ page: p }).withTags(['wcag2aa', 'wcag21aa']).include('#app').analyze()
const cc = axe.violations.filter(v => v.id === 'color-contrast')
check('Kontraste im dunklen Modus (axe)', cc.length === 0, cc.flatMap(v => v.nodes.map(n => n.target.join(' '))).slice(0, 5).join(' | '))
await p.reload(); await p.waitForSelector('.ProseMirror')
check('Wahl bleibt nach Neuladen', await theme(p) === 'dark')
await p.click('button[title="Dunkler Modus ein/aus"]'); check('zurück auf hell', await theme(p) === 'light')
await ctx.close()
ctx = await b.newContext({ colorScheme: 'dark' }); p = await ctx.newPage(); await p.goto(base + '/open?padname=dunkeltest-geraet'); await p.waitForSelector('.ProseMirror')
check('dunkel nach Gerät ohne eigene Wahl', await theme(p) === 'dark')
await b.close(); console.log(`${ok} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
