// Lasttest ohne Browser: N Yjs-Clients über y-websocket (Node) gegen den Server.
// node test/lasttest.mjs <ids-datei> [clients=200] [pads=20] [sekunden=30] [tippabstand_ms=500]
// Misst: Latenz bis ein eingefügter Marker bei einem anderen Client ankommt, Konvergenz am Ende, CPU/RSS des Servers.
import { readFileSync } from 'node:fs'
import { execSync } from 'node:child_process'
import * as Y from 'yjs'
import { WebsocketProvider } from 'y-websocket'
import WebSocket from 'ws'
const [G] = readFileSync(process.argv[2], 'utf8').trim().split('|')
const N = +(process.argv[3] || 200), PADS = +(process.argv[4] || 20), SEC = +(process.argv[5] || 30), GAP = +(process.argv[6] || 500)
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const wsBase = base.replace(/^http/, 'ws') + '/ws'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
const post = async (fn, params) => { const fd = new FormData(); for (const [k, v] of Object.entries({ ...params, apikey: key })) fd.append(k, v); return (await fetch(`${base}/api/1.2/${fn}`, { method: 'POST', body: fd })).json() }
const pid = execSync('pgrep -x nextpad').toString().trim().split('\n')[0]
// CPU aus /proc (Delta utime+stime je Abtastung, in Prozent eines Kerns), RSS aus /proc/pid/status
import { readFileSync as rf } from 'node:fs'
const clk = 100
let lastTicks = null, lastT = null
const sample = () => {
  const st = rf(`/proc/${pid}/stat`, 'utf8').split(') ')[1].split(' ')
  const ticks = +st[11] + +st[12]
  const now = performance.now()
  let cpu = 0
  if (lastTicks !== null) cpu = (ticks - lastTicks) / clk / ((now - lastT) / 1000) * 100
  lastTicks = ticks; lastT = now
  const rss = +(rf(`/proc/${pid}/status`, 'utf8').match(/VmRSS:\s+(\d+)/)?.[1] || 0)
  return { cpu, rssMB: Math.round(rss / 1024) }
}
sample()

const pads = []
// LAST_PAD_ID: vorhandenes Pad nutzen (mehrere Testprozesse im selben Pad)
if (process.env.LAST_PAD_ID) pads.push(process.env.LAST_PAD_ID)
else for (let i = 0; i < PADS; i++) { const r = await post('createGroupPad', { groupID: G, padName: 'last' + Date.now() + '_' + i, text: 'Lasttest\n' }); pads.push(r.data.padID) }
const sendTimes = new Map() // marker → t0
const latencies = []
const clients = []
let connected = 0
for (let i = 0; i < N; i++) {
  const a = await post('createAuthorIfNotExistsFor', { authorMapper: 'last' + i, name: 'Last ' + i })
  const s = await post('createSession', { groupID: G, authorID: a.data.authorID, validUntil: String(Math.floor(Date.now() / 1000) + 3600) })
  const sid = s.data.sessionID
  class WS extends WebSocket { constructor(url) { super(url, { headers: { Cookie: 'sessionID=' + sid } }) } }
  const doc = new Y.Doc()
  const pad = pads[i % PADS]
  const prov = new WebsocketProvider(wsBase, pad, doc, { WebSocketPolyfill: WS, disableBc: true })
  prov.on('status', e => { if (e.status === 'connected') connected++ })
  const frag = doc.getXmlFragment('default')
  frag.observeDeep(events => {
    for (const ev of events) for (const d of ev.changes.delta) if (typeof d.insert === 'string') {
      for (const m of d.insert.matchAll(/\[(\d+):(\d+)\]/g)) { const t0 = sendTimes.get(m[0]); if (t0 !== undefined && +m[1] !== i) latencies.push(performance.now() - t0) }
    }
  })
  clients.push({ i, doc, prov, frag, pad })
}
await new Promise(r => setTimeout(r, 3000))
console.log(`verbunden: ${connected}/${N}, Pads: ${PADS}, Dauer ${SEC}s, Tippabstand ${GAP}ms`)
const samples = []
const t0 = performance.now(); let k = 0
const timer = setInterval(() => {
  for (const c of clients) {
    const text = c.frag.get(0)?.get(0)
    if (text) { const marker = `[${c.i}:${k}]`; sendTimes.set(marker, performance.now()); c.doc.transact(() => text.insert(text.length, marker + ' ')) }
    else if (c.frag.length === 0) { /* leeres Pad */ }
  }
  k++
  samples.push(sample())
}, GAP)
await new Promise(r => setTimeout(r, SEC * 1000))
clearInterval(timer)
await new Promise(r => setTimeout(r, 3000))
// Konvergenz je Pad
let diverged = 0
for (const pad of pads) { const texts = new Set(clients.filter(c => c.pad === pad).map(c => c.frag.toString())); if (texts.size > 1) diverged++ }
latencies.sort((a, b) => a - b)
const q = p => latencies[Math.floor(latencies.length * p)]?.toFixed(0)
console.log(`Marker gesendet: ${k * N}, Latenzen gemessen: ${latencies.length}, Median ${q(0.5)} ms, p95 ${q(0.95)} ms, p99 ${q(0.99)} ms, max ${latencies.at(-1)?.toFixed(0)} ms`)
console.log(`Server CPU % eines Kerns: Mittel ${(samples.reduce((a, s) => a + s.cpu, 0) / samples.length).toFixed(1)}, max ${Math.max(...samples.map(s => s.cpu)).toFixed(1)}; RSS MB max ${Math.max(...samples.map(s => s.rssMB))}`)
console.log(`Pads mit abweichenden Ständen zwischen Clients: ${diverged} von ${PADS}`)
for (const c of clients) c.prov.destroy()
process.exit(0)
