// Startseite: Prüfung des Pad-Namens vor dem Absenden, zuletzt geöffnete Pads (nur in diesem Browser).
(() => {
  const base = document.documentElement.dataset.base || ''

  // Pad-Name: dieselben Regeln wie auf dem Server (valid_free_pad_name), Fehler als Text, nicht nur Farbe
  const form = document.querySelector('form.join')
  const input = document.getElementById('padname')
  const err = document.getElementById('padname-error')
  function problem(v) {
    if (!v) return 'Bitte einen Namen für das Pad eingeben.'
    if ([...v].length > 50) return 'Der Name ist zu lang: höchstens 50 Zeichen.'
    if (/^[gr]\./.test(v)) return 'Namen dürfen nicht mit „g.“ oder „r.“ beginnen.'
    if (!/^[\p{L}\p{N} ._(),-]+$/u.test(v)) return 'Erlaubt sind Buchstaben, Ziffern, Leerzeichen und die Zeichen - _ . ( ) ,'
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

  // Zuletzt geöffnet (bis 0.2.x unter cooppad.recent)
  try { const o = localStorage.getItem('cooppad.recent'); if (o && !localStorage.getItem('nextpad.recent')) localStorage.setItem('nextpad.recent', o); localStorage.removeItem('cooppad.recent') } catch {}
  const KEY = 'nextpad.recent'
  let list = []
  try { list = JSON.parse(localStorage.getItem(KEY) || '[]') } catch { list = [] }
  if (!Array.isArray(list)) list = []
  const box = document.getElementById('recent')
  const ul = document.getElementById('recentlist')
  const rel = t => {
    const m = Math.round((Date.now() - t) / 60000)
    if (m < 1) return 'gerade eben'
    if (m < 60) return `vor ${m} Min.`
    const h = Math.round(m / 60)
    if (h < 24) return `vor ${h} Std.`
    const d = Math.round(h / 24)
    return d === 1 ? 'gestern' : `vor ${d} Tagen`
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
      if (title !== e.id) a.setAttribute('aria-label', `${title}, Pad ${e.id}, ${w.textContent}`)
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
