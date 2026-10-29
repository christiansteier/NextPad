// Startseite: Prüfung des Pad-Namens vor dem Absenden, zuletzt geöffnete Pads (nur in diesem Browser).
(() => {
  const base = document.documentElement.dataset.base || ''
  // Texte in der Sprache der Seite (Server, Block #start-texte); Platzhalter {name}
  let T = {}
  try { T = JSON.parse(document.getElementById('start-texte')?.textContent || '{}') } catch { T = {} }
  const tx = (k, w = {}) => (T[k] || k).replace(/\{(\w+)\}/g, (_, n) => String(w[n] ?? ''))

  // Pad-Name: dieselben Regeln wie auf dem Server (valid_free_pad_name), Fehler als Text, nicht nur Farbe
  const form = document.querySelector('form.join')
  const input = document.getElementById('padname')
  const err = document.getElementById('padname-error')
  function problem(v) {
    if (!v) return '' // leer: der Server legt ein Pad mit Zufallsname an
    if ([...v].length > 50) return tx('zu_lang')
    if (/^[gr]\./.test(v)) return tx('praefix')
    if (!/^[\p{L}\p{N} ._(),-]+$/u.test(v)) return tx('zeichen')
    return ''
  }
  function show(msg) {
    if (!err || !input) return
    err.textContent = msg; err.hidden = !msg
    if (msg) input.setAttribute('aria-invalid', 'true'); else input.removeAttribute('aria-invalid')
  }
  if (form && input) {
    form.addEventListener('submit', e => {
      input.value = input.value.trim()
      const msg = problem(input.value)
      if (msg) { e.preventDefault(); show(msg); input.focus() }
    })
    input.addEventListener('input', () => { if (input.getAttribute('aria-invalid')) show(problem(input.value.trim())) })
  }

  // Zuletzt geöffnet
  const KEY = 'nextpad.recent'
  let list = []
  try { list = JSON.parse(localStorage.getItem(KEY) || '[]') } catch { list = [] }
  if (!Array.isArray(list)) list = []
  const box = document.getElementById('recent')
  const ul = document.getElementById('recentlist')
  const rel = t => {
    const m = Math.round((Date.now() - t) / 60000)
    if (m < 1) return tx('gerade')
    if (m < 60) return tx('min', { n: m })
    const h = Math.round(m / 60)
    if (h < 24) return tx('std', { n: h })
    const d = Math.round(h / 24)
    return d === 1 ? tx('gestern') : tx('tage', { n: d })
  }
  const entries = list.filter(e => e && typeof e.id === 'string' && e.id.length <= 60).slice(0, 6)
  if (box && ul && entries.length) {
    for (const e of entries) {
      const li = document.createElement('li')
      const a = document.createElement('a')
      a.href = `${base}/p/${encodeURIComponent(e.id)}`
      const title = (typeof e.title === 'string' && e.title.trim()) ? e.title.trim() : e.id
      const t = document.createElement('span'); t.className = 't'; t.textContent = title
      const w = document.createElement('span'); w.className = 'w'; w.textContent = typeof e.t === 'number' ? rel(e.t) : ''
      if (title !== e.id) a.setAttribute('aria-label', tx('label', { titel: title, id: e.id, wann: w.textContent }))
      a.append(t, w); li.append(a); ul.append(li)
    }
    box.hidden = false
    document.getElementById('recentclear')?.addEventListener('click', () => {
      try { localStorage.removeItem(KEY) } catch {}
      box.hidden = true
      input?.focus()
    })
  }
})()
