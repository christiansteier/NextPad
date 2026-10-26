// Belastungstest mit schlanken Clients: jede Person schreibt echte Yjs-Änderungen, spielt fremde aber nicht ein,
// sondern sucht nur die Markierungen in den empfangenen Bytes. So misst der Test den Server, nicht den Testrechner.
// node test/belastung-leicht.mjs <basis-url> <padname-präfix> <personen> <pads> <sekunden> <abstand_ms> [prozess-nr]
import * as Y from 'yjs'
import * as encoding from 'lib0/encoding'
import * as decoding from 'lib0/decoding'
import * as syncProtocol from 'y-protocols/sync'
import WebSocket from 'ws'
const [base, prefix, N0, P0, S0, G0, proc = '0'] = process.argv.slice(2)
const N = +N0, PADS = +P0, SEC = +S0, GAP = +G0
const wsBase = base.replace(/^http/, 'ws') + '/ws'
const pads = Array.from({ length: PADS }, (_, i) => `${prefix}-${i}`)
const sendTimes = new Map(); const lat = []; let open = 0, closed = 0, ready = 0
async function guestCookie(pad) {
  const r = await fetch(`${base}/p/${encodeURIComponent(pad)}`, { redirect: 'manual' })
  return r.headers.getSetCookie().map(x => x.split(';')[0]).find(x => x.startsWith('nextpad_author='))
}
const clients = []
for (let i = 0; i < N; i++) {
  const pad = pads[i % PADS]; const cookie = await guestCookie(pad)
  if (!cookie) { console.log(`[${proc}] kein Gast-Cookie für ${pad}`); continue }
  const ws = new WebSocket(`${wsBase}/${encodeURIComponent(pad)}`, { headers: { Cookie: cookie, Origin: new URL(base).origin } })
  ws.binaryType = 'arraybuffer'
  const doc = new Y.Doc(); const c = { id: `${proc}.${i}`, ws, doc, ready: false }
  doc.on('update', (u, origin) => { if (origin === 'lokal' && ws.readyState === 1) { const e = encoding.createEncoder(); encoding.writeVarUint(e, 0); syncProtocol.writeUpdate(e, u); ws.send(encoding.toUint8Array(e)) } })
  ws.on('open', () => { open++; const e = encoding.createEncoder(); encoding.writeVarUint(e, 0); syncProtocol.writeSyncStep1(e, doc); ws.send(encoding.toUint8Array(e)) })
  ws.on('close', () => closed++)
  ws.on('message', data => {
    const buf = new Uint8Array(data)
    if (!c.ready) {
      const d = decoding.createDecoder(buf)
      if (decoding.readVarUint(d) === 0) {
        const e = encoding.createEncoder()
        const t = syncProtocol.readSyncMessage(d, e, doc, 'server')
        if (t === syncProtocol.messageYjsSyncStep2) { c.ready = true; ready++ }
      }
      return
    }
    const s = Buffer.from(buf).toString('latin1')
    for (const m of s.matchAll(/\[(\d+\.\d+):(\d+)\]/g)) { const t0 = sendTimes.get(m[0]); if (t0 !== undefined && m[1] !== c.id) lat.push(performance.now() - t0) }
  })
  clients.push(c)
}
await new Promise(r => setTimeout(r, 4000))
console.log(`[${proc}] verbunden ${open}/${N}, abgeglichen ${ready}, Pads ${PADS}, ${SEC}s, Abstand ${GAP}ms`)
let k = 0, sent = 0
const timer = setInterval(() => {
  for (const c of clients) {
    if (!c.ready) continue
    const frag = c.doc.getXmlFragment('default')
    // EIGENER_ABSATZ=1: jede Person schreibt in einen eigenen Absatz (realistisch), sonst alle in den ersten
    if (process.env.EIGENER_ABSATZ === '1' && !c.para) { c.doc.transact(() => { const p = new Y.XmlElement('paragraph'); const x = new Y.XmlText(); p.insert(0, [x]); frag.insert(frag.length, [p]); c.para = x }, 'lokal') }
    const t = process.env.EIGENER_ABSATZ === '1' ? c.para : frag.get(0)?.get(0)
    if (!(t instanceof Y.XmlText)) continue
    const m = `[${c.id}:${k}]`; sendTimes.set(m, performance.now()); c.doc.transact(() => t.insert(t.length, m + ' '), 'lokal'); sent++
  }
  k++
}, GAP)
await new Promise(r => setTimeout(r, SEC * 1000)); clearInterval(timer); await new Promise(r => setTimeout(r, 4000))
lat.sort((a, b) => a - b); const q = x => lat[Math.floor(lat.length * x)]?.toFixed(0)
console.log(`[${proc}] gesendet ${sent}, Zustellungen ${lat.length}, Median ${q(.5)} ms, p95 ${q(.95)} ms, p99 ${q(.99)} ms, max ${lat.at(-1)?.toFixed(0)} ms, Abbrüche ${closed}`)
for (const c of clients) c.ws.close()
process.exit(0)
