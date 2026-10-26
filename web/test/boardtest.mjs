// Zeichenbrett: zwei Browser, A zeichnet ein Rechteck, B sieht es; Zeiger, CSP-Verstöße, fremde Anfragen.
// node test/boardtest.mjs <basis-url>
import { chromium } from 'playwright'
const base = process.argv[2] || 'http://127.0.0.1:5093'
const b = await chromium.launch()
const errs = [], foreign = []
async function ctxPage() {
  const ctx = await b.newContext({ viewport: { width: 1280, height: 800 } })
  const p = await ctx.newPage()
  p.on('console', m => { if (m.type() === 'error') errs.push(m.text().slice(0, 160)) })
  p.on('request', r => { const u = new URL(r.url()); if (u.origin !== new URL(base).origin && !u.protocol.startsWith('data') && !u.protocol.startsWith('blob')) foreign.push(u.href.slice(0, 100)) })
  return p
}
const a = await ctxPage()
await a.goto(`${base}/new?typ=zeichenbrett`)
await a.waitForSelector('.excalidraw', { timeout: 20000 })
const url = a.url()
const pb = await ctxPage()
await pb.goto(url); await pb.waitForSelector('.excalidraw', { timeout: 20000 })
await a.waitForTimeout(1500)
// Rechteck zeichnen: Taste r, ziehen
await a.mouse.click(640, 400); await a.keyboard.press('r')
await a.mouse.move(400, 300); await a.mouse.down(); await a.mouse.move(600, 450, { steps: 8 }); await a.mouse.up()
await a.keyboard.press('Escape')
await pb.waitForTimeout(2000)
const countA = await a.evaluate(() => [...window.nextpad.yElements.values()].filter(e => !e.isDeleted).length)
const countB = await pb.evaluate(() => [...window.nextpad.yElements.values()].filter(e => !e.isDeleted).length)
const typeB = await pb.evaluate(() => [...window.nextpad.yElements.values()].map(e => e.type).join(','))
await pb.mouse.move(700, 500); await pb.mouse.move(720, 520); await a.waitForTimeout(800)
const cursors = await a.evaluate(() => window.nextpad.provider.awareness.getStates().size)
await a.reload(); await a.waitForSelector('.excalidraw'); await a.waitForTimeout(2000)
const afterReload = await a.evaluate(() => window.nextpad.api.getSceneElements().length)
const sceneB = await pb.evaluate(() => window.nextpad.api.getSceneElements().length)
await a.screenshot({ path: process.env.SHOT || '/tmp/board.png' })
console.log(JSON.stringify({ url: url.replace(base, ''), countA, countB, sceneB, typeB, awarenessClients: cursors, afterReload, consoleErrors: errs, foreignRequests: foreign }, null, 1))
await b.close()
process.exit(countA >= 1 && countB === countA && sceneB === countA && afterReload === countA && !foreign.length ? 0 : 1)
