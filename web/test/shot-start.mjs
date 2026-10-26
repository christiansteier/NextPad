// Startseite auf dem Telefon, hell und dunkel, mit „Zuletzt geöffnet“: node shot-start.mjs <url> <ausgabe-präfix>
import { chromium } from 'playwright'
const [url, out] = process.argv.slice(2)
const b = await chromium.launch()
for (const scheme of ['light', 'dark']) {
  const ctx = await b.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true, colorScheme: scheme })
  await ctx.addInitScript(() => { localStorage.setItem('nextpad.recent', JSON.stringify([{ id: 'gruppe-3', t: Date.now() - 600000, title: 'Gruppenarbeit Statistik' }, { id: 'Xy12ab34Cd56Ef78', t: Date.now() - 2 * 86400000, title: '' }])) })
  const p = await ctx.newPage(); await p.goto(url); await p.screenshot({ path: `${out}-${scheme}.png`, fullPage: true }); await ctx.close()
}
await b.close()
