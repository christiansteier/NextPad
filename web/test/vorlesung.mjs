// Vorlesungs-Lasttest: W Schreibende (echte Y.Docs) und R Mitlesende (nur Empfang, ohne CRDT-Rechnung)
// in EINEM Pad. Mitlesende suchen die Marker direkt in den empfangenen Bytes; so misst der Test den Server,
// nicht den Testrechner. node test/vorlesung.mjs <gruppe> <padID> [schreibende=20] [mitlesende=300] [sekunden=30] [abstand_ms=2000]
import * as Y from 'yjs'
import { WebsocketProvider } from 'y-websocket'
import WebSocket from 'ws'
import { readFileSync } from 'node:fs'
import { execSync } from 'node:child_process'
const [G, PAD] = process.argv.slice(2)
const W = +(process.argv[4] || 20), R = +(process.argv[5] || 300), SEC = +(process.argv[6] || 30), GAP = +(process.argv[7] || 2000)
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5093'
const wsBase = base.replace(/^http/, 'ws') + '/ws'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE, 'utf8').trim()
const post = async (fn, p) => { const fd = new FormData(); for (const [k, v] of Object.entries({ ...p, apikey: key })) fd.append(k, v); return (await fetch(`${base}/api/1.2/${fn}`, { method: 'POST', body: fd })).json() }
async function session(i) {
  const a = await post('createAuthorIfNotExistsFor', { authorMapper: 'v' + i, name: 'V' + i })
  const s = await post('createSession', { groupID: G, authorID: a.data.authorID, validUntil: String(Math.floor(Date.now() / 1000) + 3600) })
  return s.data.sessionID
}
const pid = execSync('pgrep -x nextpad').toString().trim().split('\n')[0]
let lastTicks = null, lastT = null
const cpu = () => { const st = readFileSync(`/proc/${pid}/stat`, 'utf8').split(') ')[1].split(' '); const t = +st[11] + +st[12]; const n = performance.now(); let c = 0; if (lastTicks !== null) c = (t - lastTicks) / 100 / ((n - lastT) / 1000) * 100; lastTicks = t; lastT = n; return c }
cpu()
const sendTimes = new Map(); const lat = []; let readersUp = 0, disconnects = 0
// Mitlesende: rohe WebSockets, Marker im Byte-Strom
const enc = new TextEncoder()
for (let i = 0; i < R; i++) {
  const sid = await session('r' + i)
  const ws = new WebSocket(`${wsBase}/${encodeURIComponent(PAD)}`, { headers: { Cookie: 'sessionID=' + sid } })
  ws.on('open', () => readersUp++)
  ws.on('close', () => disconnects++)
  ws.on('message', buf => {
    const s = buf.toString('latin1')
    for (const m of s.matchAll(/\[w(\d+):(\d+)\]/g)) { const t0 = sendTimes.get(m[0]); if (t0 !== undefined) lat.push(performance.now() - t0) }
  })
}
// Schreibende
const writers = []
for (let i = 0; i < W; i++) {
  const sid = await session('w' + i)
  class WS extends WebSocket { constructor(u) { super(u, { headers: { Cookie: 'sessionID=' + sid } }) } }
  const doc = new Y.Doc(); const prov = new WebsocketProvider(wsBase, PAD, doc, { WebSocketPolyfill: WS, disableBc: true })
  writers.push({ i, doc, prov, frag: doc.getXmlFragment('default') })
}
await new Promise(r => setTimeout(r, 3000))
console.log(`Mitlesende verbunden: ${readersUp}/${R}, Schreibende: ${W}, Abstand ${GAP} ms, ${SEC} s`)
const cpus = []; let k = 0
const timer = setInterval(() => {
  for (const w of writers) { const text = w.frag.get(0)?.get(0); if (text) { const m = `[w${w.i}:${k}]`; sendTimes.set(m, performance.now()); w.doc.transact(() => text.insert(text.length, m + ' ')) } }
  k++; cpus.push(cpu())
}, GAP)
await new Promise(r => setTimeout(r, SEC * 1000)); clearInterval(timer); await new Promise(r => setTimeout(r, 3000))
lat.sort((a, b) => a - b); const q = p => lat[Math.floor(lat.length * p)]?.toFixed(0)
const expected = k * W * R
console.log(`Eingaben: ${k * W}, Zustellungen gemessen: ${lat.length} von ${expected} (${(100 * lat.length / expected).toFixed(1)} %), Median ${q(.5)} ms, p95 ${q(.95)} ms, p99 ${q(.99)} ms, max ${lat.at(-1)?.toFixed(0)} ms`)
console.log(`Server CPU % eines Kerns: Mittel ${(cpus.reduce((a, b) => a + b, 0) / cpus.length).toFixed(1)}, max ${Math.max(...cpus).toFixed(1)}; Verbindungsabbrüche: ${disconnects}`)
process.exit(0)
