// Eingehende Yjs-Änderungen bündeln: höchstens einmal je Bildschirmbild einspielen.
// Hintergrund: y-prosemirror tauscht bei jeder fremden Änderung den Dokumentinhalt aus. Schreiben viele Personen
// gleichzeitig (100 Studierende, 200 Änderungen/s), wäre das 200-mal je Sekunde; gebündelt sind es höchstens ~60,
// und wird das Einspielen teuer (großes Dokument), wächst der Abstand mit, bis höchstens 250 ms.
import * as Y from 'yjs'
import * as decoding from 'lib0/decoding'

const MESSAGE_SYNC = 0
const SYNC_UPDATE = 2

export function batchRemoteUpdates(provider) {
  const original = provider.messageHandlers[MESSAGE_SYNC]
  let queue = []
  let scheduled = false
  let lastCost = 0
  const flush = () => {
    scheduled = false
    if (!queue.length) return
    const updates = queue
    queue = []
    const t0 = performance.now()
    Y.applyUpdate(provider.doc, updates.length === 1 ? updates[0] : Y.mergeUpdates(updates), provider)
    lastCost = performance.now() - t0
  }
  const schedule = () => {
    if (scheduled) return
    scheduled = true
    if (document.hidden) { setTimeout(flush, 250); return } // Hintergrund-Tab: kein requestAnimationFrame
    if (lastCost > 8) setTimeout(() => requestAnimationFrame(flush), Math.min(250, lastCost * 4))
    else requestAnimationFrame(flush)
  }
  provider.messageHandlers[MESSAGE_SYNC] = (encoder, decoder, prov, emitSynced, messageType) => {
    if (decoding.peekVarUint(decoder) === SYNC_UPDATE) {
      decoding.readVarUint(decoder)
      queue.push(decoding.readVarUint8Array(decoder))
      schedule()
      return
    }
    flush() // Reihenfolge: Ausstehendes vor Sync-Schritten einspielen
    original(encoder, decoder, prov, emitSynced, messageType)
  }
  document.addEventListener('visibilitychange', () => { if (!document.hidden) flush() })
  return { flush, pending: () => queue.length }
}
