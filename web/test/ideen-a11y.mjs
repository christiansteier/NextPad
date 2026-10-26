// Ideensammlung: axe (WCAG 2.2 AA) hell und dunkel, Querscrollen, Bildschirmfotos
// node test/ideen-a11y.mjs <basis-url> <ausgabe>
import { chromium } from 'playwright'
import AxeBuilder from '@axe-core/playwright'
const [base = 'http://127.0.0.1:5093', out = '.'] = process.argv.slice(2)
const b = await chromium.launch()
const a = await (await b.newContext()).newPage()
await a.goto(base + '/new?typ=ideen'); await a.waitForSelector('#add-form:not([hidden])')
const url = a.url()
for (const t of ['Brauchen wir mehr Übungszeit vor der Klausur?', 'Lernvideos zu den schwierigen Kapiteln', 'Kurze Quizfragen am Anfang jeder Sitzung, damit man merkt, wo es hakt']) { await a.fill('#add-text', t); await a.click('#add-form button[type=submit]'); await a.waitForTimeout(150) }
await a.locator('.vote').first().click(); await a.waitForTimeout(300)
await a.locator('button[aria-label="Als erledigt markieren"]').last().click(); await a.waitForTimeout(300)
let fails = 0
for (const scheme of ['light', 'dark']) for (const w of [320, 768, 1440]) {
  const ctx = await b.newContext({ viewport: { width: w, height: 900 }, colorScheme: scheme })
  await ctx.addCookies(await a.context().cookies())
  const p = await ctx.newPage(); await p.goto(url); await p.waitForFunction(() => document.querySelectorAll('#open-list li').length > 0)
  const axe = await new AxeBuilder({ page: p }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze()
  const over = await p.evaluate(() => document.documentElement.scrollWidth - innerWidth)
  const v = axe.violations.map(x => `${x.id}×${x.nodes.length} ${x.nodes[0].target}`)
  if (v.length || over > 0) fails++
  console.log(`${scheme}-${w}: axe ${v.join(', ') || 'ok'} | quer ${over}px`)
  await p.screenshot({ path: `${out}/ideen-${scheme}-${w}.png`, fullPage: true })
  await p.goto(url + '/leinwand'); await p.waitForSelector('.wall-item')
  const axw = await new AxeBuilder({ page: p }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze()
  const vw = axw.violations.map(x => `${x.id}×${x.nodes.length} ${x.nodes[0].target}`)
  if (vw.length) fails++
  console.log(`  leinwand: axe ${vw.join(', ') || 'ok'}`)
  await p.screenshot({ path: `${out}/wand-${scheme}-${w}.png` }); await ctx.close()
}
await b.close(); process.exit(fails ? 1 : 0)
