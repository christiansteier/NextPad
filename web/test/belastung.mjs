// Belastungstest über die öffentliche Adresse mit freien Pads (Gast-Cookies, wie Studierende über einen Link).
// Jede Person ist ein vollständiger Yjs-Client; gemessen wird die Zeit, bis eine Eingabe bei den anderen ankommt.
// node test/belastung.mjs <basis-url> <padname-präfix> <personen> <pads> <sekunden> <abstand_ms> [prozess-nr]
import * as Y from 'yjs'
import { WebsocketProvider } from 'y-websocket'
import WebSocket from 'ws'
const [base, prefix, N0, P0, S0, G0, proc = '0'] = process.argv.slice(2)
const N = +N0, PADS = +P0, SEC = +S0, GAP = +G0
const wsBase = base.replace(/^http/, 'ws') + '/ws'
const pads = Array.from({ length: PADS }, (_, i) => `${prefix}-${i}`)
// Pad anlegen (nur Prozess 0) bzw. öffnen; Gast-Cookie je Person über einen Seitenaufruf
async function guestCookie(pad) {
  const r = await fetch(`${base}/p/${encodeURIComponent(pad)}`, { redirect: 'manual' })
  const c = r.headers.getSetCookie().map(x => x.split(';')[0]).find(x => x.startsWith('nextpad_author='))
  return c
}
if (proc === '0') for (const p of pads) await fetch(`${base}/open?padname=${encodeURIComponent(p)}`, { redirect: 'manual' })
else await new Promise(r => setTimeout(r, 3000))
const sendTimes = new Map(); const lat = []; let connected = 0, closed = 0
const clients = []
for (let i = 0; i < N; i++) {
  const pad = pads[i % PADS]
  const cookie = await guestCookie(pad)
  class WS extends WebSocket { constructor(u) { super(u, { headers: { Cookie: cookie, Origin: new URL(base).origin } }) } }
  const doc = new Y.Doc()
  const prov = new WebsocketProvider(wsBase, encodeURIComponent(pad), doc, { WebSocketPolyfill: WS, disableBc: true })
  prov.on('status', e => { if (e.status === 'connected') connected++; if (e.status === 'disconnected') closed++ })
  const frag = doc.getXmlFragment('default')
  const id = `${proc}.${i}`
  frag.observeDeep(evs => { for (const ev of evs) for (const d of ev.changes.delta) if (typeof d.insert === 'string')
    for (const m of d.insert.matchAll(/\[(\d+\.\d+):(\d+)\]/g)) { const t0 = sendTimes.get(m[0]); if (t0 !== undefined && m[1] !== id) lat.push(performance.now() - t0) } })
  clients.push({ id, doc, prov, frag, pad })
}
await new Promise(r => setTimeout(r, 4000))
// leeres Pad: erster Client legt einen Absatz an
for (const c of clients) if (c.frag.length === 0) c.doc.transact(() => { const p = new Y.XmlElement('paragraph'); p.insert(0, [new Y.XmlText('Start ')]); c.frag.insert(0, [p]) })
await new Promise(r => setTimeout(r, 1500))
console.log(`[${proc}] verbunden: ${connected}/${N}, Pads ${PADS}, ${SEC}s, Abstand ${GAP}ms`)
let k = 0, sent = 0
const timer = setInterval(() => {
  for (const c of clients) { const t = c.frag.get(0)?.get(0); if (t) { const m = `[${c.id}:${k}]`; sendTimes.set(m, performance.now()); c.doc.transact(() => t.insert(t.length, m + ' ')); sent++ } }
  k++
}, GAP)
await new Promise(r => setTimeout(r, SEC * 1000)); clearInterval(timer); await new Promise(r => setTimeout(r, 4000))
let diverged = 0
for (const p of pads) { const s = new Set(clients.filter(c => c.pad === p).map(c => c.frag.toString())); if (s.size > 1) diverged++ }
lat.sort((a, b) => a - b); const q = x => lat[Math.floor(lat.length * x)]?.toFixed(0)
console.log(`[${proc}] gesendet ${sent}, Zustellungen ${lat.length}, Median ${q(.5)} ms, p95 ${q(.95)} ms, p99 ${q(.99)} ms, max ${lat.at(-1)?.toFixed(0)} ms, Abbrüche ${closed}, abweichende Pads ${diverged}`)
for (const c of clients) c.prov.destroy()
process.exit(0)
