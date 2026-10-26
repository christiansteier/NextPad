// Prüfung der Startseite: axe (WCAG 2.2 A/AA), Querscrollen, Textabstände (1.4.12), Tab-Reihenfolge, Fokus,
// Zielgrößen; Bildschirmfotos je Prüfbreite, hell und dunkel.
// node test/a11y-start.mjs <url> <ausgabeverzeichnis>
import { chromium } from 'playwright'
import AxeBuilder from '@axe-core/playwright'
import { mkdirSync, writeFileSync } from 'node:fs'
const [url, out] = process.argv.slice(2)
mkdirSync(out, { recursive: true })
const widths = [320, 375, 768, 1024, 1440]
const b = await chromium.launch()
const report = {}
for (const scheme of ['light', 'dark']) {
  for (const w of widths) {
    const ctx = await b.newContext({ viewport: { width: w, height: 900 }, colorScheme: scheme, reducedMotion: 'reduce' })
    await ctx.addInitScript(() => { try { localStorage.setItem('nextpad.recent', JSON.stringify([{ id: 'gruppe-3', t: Date.now() - 600000, title: 'Gruppenarbeit Statistik' }])) } catch {} })
    const p = await ctx.newPage()
    await p.goto(url)
    await p.waitForTimeout(300)
    const key = `${scheme}-${w}`
    await p.screenshot({ path: `${out}/${key}.png`, fullPage: true })
    const axe = await new AxeBuilder({ page: p }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze()
    const overflow = await p.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
    // 1.4.12: Zeilenhöhe 1,5, Absatz 2em, Buchstaben 0,12em, Wörter 0,16em
    await p.addStyleTag({ content: '*{line-height:1.5 !important;letter-spacing:.12em !important;word-spacing:.16em !important} p{margin-bottom:2em !important}' })
    const spacingOverflow = await p.evaluate(() => {
      const bad = []
      for (const el of document.querySelectorAll('a,button,label,input,h1,h2,p,span')) {
        if (el.scrollWidth > el.clientWidth + 2 && getComputedStyle(el).overflow !== 'visible') bad.push((el.textContent || el.getAttribute('placeholder') || '').trim().slice(0, 30))
      }
      return { bad: bad.slice(0, 8), pageOverflow: document.documentElement.scrollWidth - window.innerWidth }
    })
    const targets = await p.evaluate(() => [...document.querySelectorAll('a,button,input')].filter(e => e.offsetParent).map(e => { const r = e.getBoundingClientRect(); return { t: (e.textContent || e.getAttribute('aria-label') || e.type).trim().slice(0, 24), w: Math.round(r.width), h: Math.round(r.height) } }).filter(x => x.w < 24 || x.h < 24))
    report[key] = { axe: axe.violations.map(v => ({ id: v.id, impact: v.impact, n: v.nodes.length, help: v.help, targets: v.nodes.slice(0, 3).map(n => n.target.join(' ')) })), overflow, spacingOverflow, smallTargets: targets }
    if (w === 1440 && scheme === 'light') {
      await p.reload(); await p.waitForTimeout(200)
      const order = []
      for (let i = 0; i < 25; i++) {
        await p.keyboard.press('Tab')
        const f = await p.evaluate(() => { const e = document.activeElement; if (!e || e === document.body) return null; const cs = getComputedStyle(e); return { el: e.tagName.toLowerCase() + (e.id ? '#' + e.id : ''), name: (e.getAttribute('aria-label') || e.textContent || e.getAttribute('placeholder') || '').trim().replace(/\s+/g, ' ').slice(0, 40), outline: cs.outlineStyle !== 'none' && cs.outlineWidth !== '0px' ? cs.outlineWidth + ' ' + cs.outlineColor : (cs.boxShadow !== 'none' ? 'box-shadow' : 'KEIN FOKUS') } })
        if (!f) break
        if (order.length && order[0].el === f.el && order[0].name === f.name) break
        order.push(f)
      }
      report.tabOrder = order
      report.headings = await p.evaluate(() => [...document.querySelectorAll('h1,h2,h3')].map(h => h.tagName + ' ' + h.textContent.trim().slice(0, 40)))
      report.landmarks = await p.evaluate(() => [...document.querySelectorAll('header,nav,main,footer,section[aria-label],section[aria-labelledby]')].map(e => e.tagName.toLowerCase() + (e.getAttribute('aria-label') ? `[${e.getAttribute('aria-label')}]` : '')))
    }
    await ctx.close()
  }
}
await b.close()
writeFileSync(`${out}/bericht.json`, JSON.stringify(report, null, 1))
const sum = Object.entries(report).filter(([k]) => k.includes('-')).map(([k, v]) => `${k}: axe ${v.axe.map(a => a.id + '×' + a.n).join(',') || 'ok'} | quer ${v.overflow}px | 1.4.12 ${v.spacingOverflow.bad.length ? v.spacingOverflow.bad.join('/') : 'ok'} (${v.spacingOverflow.pageOverflow}px) | <24px ${v.smallTargets.length}`)
console.log(sum.join('\n'))
console.log('Tab:', report.tabOrder?.map(t => `${t.el}:${t.name}[${t.outline}]`).join(' → '))
console.log('Überschriften:', report.headings?.join(' | '))
console.log('Bereiche:', report.landmarks?.join(', '))
