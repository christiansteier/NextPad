// Lesbarkeit auf durchscheinenden Flächen über dem Foto: axe kann Text auf Bildern nicht bewerten.
// Für jedes Textstück: Hintergrund ohne Text fotografieren, Kontrast gegen den ungünstigsten Bildpunkt (1. Perzentil).
// node test/transparenz.mjs <basis-url> [pfad]
import { chromium } from 'playwright'
const [base = 'http://127.0.0.1:5093', path = '/'] = process.argv.slice(2)
const b = await chromium.launch()
const lum = ([r, g, bl]) => { const f = c => { c /= 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4 }; return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(bl) }
const cr = (a, c) => (Math.max(a, c) + 0.05) / (Math.min(a, c) + 0.05)
let fails = 0
for (const scheme of ['light', 'dark']) for (const w of [320, 768, 1024, 1440, 1920]) {
  const ctx = await b.newContext({ viewport: { width: w, height: 1000 }, colorScheme: scheme, deviceScaleFactor: 1 })
  const p = await ctx.newPage(); await p.goto(base + path); await p.waitForTimeout(300)
  // Textstücke: Elemente mit eigenem Text, sichtbar
  const items = await p.evaluate(() => {
    const out = []
    for (const el of document.querySelectorAll('main *')) {
      const own = [...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim())
      if (!own) continue
      const r = el.getBoundingClientRect(); if (r.width < 2 || r.height < 2) continue
      const cs = getComputedStyle(el); if (cs.visibility === 'hidden' || cs.display === 'none') continue
      const m = cs.color.match(/[\d.]+/g).map(Number)
      const size = parseFloat(cs.fontSize), bold = Number(cs.fontWeight) >= 700
      // nur innerhalb des Rahmens messen, der Rahmen gehört nicht zum Hintergrund des Textes
      const bl = parseFloat(cs.borderLeftWidth), bt = parseFloat(cs.borderTopWidth), br = parseFloat(cs.borderRightWidth), bb = parseFloat(cs.borderBottomWidth)
      out.push({ sel: el.tagName.toLowerCase() + (el.id ? '#' + el.id : el.className ? '.' + String(el.className).split(' ')[0] : ''), text: el.textContent.trim().slice(0, 30), x: r.x + scrollX + bl + 1, y: r.y + scrollY + bt + 1, w: r.width - bl - br - 2, h: r.height - bt - bb - 2, rgb: m.slice(0, 3), large: size >= 24 || (bold && size >= 18.66) })
    }
    return out
  })
  await p.addStyleTag({ content: '*, *::before, *::after { color: transparent !important; text-shadow: none !important; } svg { visibility: hidden !important; } input::placeholder { color: transparent !important; }' })
  const shot = (await p.screenshot({ fullPage: true })).toString('base64')
  const a = await ctx.newPage()
  const res = await a.evaluate(async ({ shot, items }) => {
    const img = new Image(); img.src = 'data:image/png;base64,' + shot; await img.decode()
    const c = document.createElement('canvas'); c.width = img.width; c.height = img.height
    const g = c.getContext('2d'); g.drawImage(img, 0, 0)
    return items.map(it => {
      const d = g.getImageData(Math.round(it.x), Math.round(it.y), Math.max(1, Math.round(it.w)), Math.max(1, Math.round(it.h))).data
      const px = []; for (let i = 0; i < d.length; i += 4) px.push([d[i], d[i + 1], d[i + 2]])
      return { ...it, px }
    })
  }, { shot, items })
  for (const it of res) {
    const lt = lum(it.rgb), ls = it.px.map(lum).map(l => cr(l, lt)).sort((x, y) => x - y)
    const worst = ls[Math.floor(ls.length * 0.01)], need = it.large ? 3 : 4.5
    if (worst < need) { fails++; console.log(`FAIL ${scheme}-${w} ${it.sel} „${it.text}“ ${worst.toFixed(2)}:1 < ${need}`) }
    it.worst = worst
  }
  const min = res.reduce((m, it) => it.worst < m.worst ? it : m, { worst: 99 })
  console.log(`${scheme}-${w}: ${res.length} Textstücke, schwächstes ${min.worst.toFixed(2)}:1 (${min.sel} „${min.text}“)`)
  await ctx.close()
}
await b.close()
console.log(fails ? `\n${fails} unter der Grenze` : '\nalle Textstücke erreichen WCAG AA')
process.exit(fails ? 1 : 0)
