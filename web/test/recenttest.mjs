// „Zuletzt geöffnet“: zwei Pads nacheinander öffnen, beide stehen danach auf der Startseite
// node test/recenttest.mjs <basis-url>
import { chromium } from 'playwright'
const base = process.argv[2] || 'http://127.0.0.1:5093'
let fails = 0
const check = (name, cond, info = '') => { console.log((cond ? 'ok   ' : 'FAIL ') + name + (info ? '  ' + info : '')); if (!cond) fails++ }
const b = await chromium.launch(); const p = await (await b.newContext({ locale: 'de-DE' })).newPage()
const stamp = Date.now().toString(36)
for (const name of [`zuletzt-a-${stamp}`, `zuletzt-b-${stamp}`]) {
  await p.goto(`${base}/open?padname=${name}`); await p.waitForSelector('.ProseMirror'); await p.waitForTimeout(400)
}
const list = await p.evaluate(() => JSON.parse(localStorage.getItem('nextpad.recent') || '[]').map(x => x.id || x.name))
check('beide Pads im Speicher', list.some(x => String(x).includes('zuletzt-a')) && list.some(x => String(x).includes('zuletzt-b')), JSON.stringify(list.slice(0, 3)))
await p.goto(base + '/'); await p.waitForTimeout(300)
const shown = await p.textContent('#recentlist').catch(() => '')
check('beide Pads auf der Startseite', shown.includes(`zuletzt-a-${stamp}`) && shown.includes(`zuletzt-b-${stamp}`))
await b.close()
console.log(`\n${fails ? fails + ' fehlgeschlagen' : 'alle bestanden'}`)
process.exit(fails ? 1 : 0)
