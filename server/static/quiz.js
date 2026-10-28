// Quiz-Raster: Die Leitung steuert am Beamer, alle anderen sehen live mit. Der Server prüft alles und gibt Antworten
// erst nach dem Aufdecken an Mitsehende heraus; hier wird nur angezeigt.
(() => {
  const cfg = JSON.parse(document.getElementById('nextpad-config').textContent)
  const base = cfg.basePath || ''
  const api = `${base}/p/${encodeURIComponent(cfg.urlId)}/quiz`
  const $ = id => document.getElementById(id)
  let data = null
  let busy = false
  let lastScores = {}
  const changedAt = {}
  // ---- Buzzer (Mitsehende): Team wählen, Uhr laufend mit dem Server abgleichen, drücken ----
  // Gewertet wird die Druckzeit in Serverzeit, nicht der Eingang; die Reihenfolge rechnet der Server.
  const now = () => performance.timeOrigin + performance.now()
  const store = { get: k => { try { return sessionStorage.getItem(k) } catch { return null } }, set: (k, v) => { try { sessionStorage.setItem(k, v) } catch {} } }
  const teamKey = `nextpad-buzzer-${cfg.urlId}`
  const dev = store.get('nextpad-geraet') || (() => {
    const a = new Uint8Array(12); crypto.getRandomValues(a)
    const d = [...a].map(b => b.toString(36).padStart(2, '0')).join('').slice(0, 20)
    store.set('nextpad-geraet', d); return d
  })()
  let myTeam = Number(store.get(teamKey)) || 0
  const recent = [] // die letzten drei Abgleiche; der mit der kürzesten Laufzeit gilt
  let syncing = false, lastQ = null, buzzTimer = 0
  const pressed = new Set()
  const bestSync = () => recent.reduce((a, b) => (!a || b.rtt < a.rtt ? b : a), null)
  async function serie() {
    const s = []
    for (let i = 0; i < 5; i++) {
      const t0 = now()
      const j = await (await fetch(`${base}/p/${encodeURIComponent(cfg.urlId)}/messung/zeit`, { cache: 'no-store' })).json()
      const t1 = now()
      s.push({ rtt: t1 - t0, off: j.t - (t0 + t1) / 2 })
    }
    s.sort((a, b) => a.rtt - b.rtt)
    recent.push(s[0]); if (recent.length > 3) recent.shift()
  }
  async function syncLoop() {
    if (syncing) return
    syncing = true
    while (myTeam) {
      const first = !recent.length
      try { await serie() } catch {}
      if (first && data) render()
      await new Promise(r => setTimeout(r, 3000))
    }
    syncing = false
  }
  // Bildschirm anlassen; erzwingen lässt es sich nicht. Lehnt der Browser ab oder kann es nicht, steht ein Hinweis da.
  async function wachHalten() {
    let ok = false
    try { if (navigator.wakeLock) { await navigator.wakeLock.request('screen'); ok = true } } catch {}
    $('buzz-wach').hidden = ok
  }
  document.addEventListener('visibilitychange', () => { if (myTeam && document.visibilityState === 'visible') wachHalten() })
  function chooseTeam(id) {
    myTeam = id; store.set(teamKey, String(id))
    if (id) { wachHalten(); syncLoop() }
    render()
  }
  function press() {
    const q = data?.current
    if (!q || q.revealed || !myTeam || pressed.has(q.id)) return
    const t = now()
    const b = bestSync()
    if (!b) return
    pressed.add(q.id)
    $('buzz-info').textContent = 'Gedrückt.'
    renderBuzz()
    post('/buzzer', { dev, team: myTeam, field: q.id, t, off: b.off, rtt: b.rtt })
      .then(r => { if (!r.neu) $('buzz-info').textContent = 'Schon gedrückt.' })
      .catch(e => { $('buzz-info').textContent = `Nicht angekommen: ${e.message}` })
  }
  // pointerdown statt click: der Druck zählt beim Antippen, nicht beim Loslassen; click bleibt für die Tastatur
  $('buzz-btn').addEventListener('pointerdown', e => { e.preventDefault(); press() })
  $('buzz-btn').addEventListener('click', press)

  function renderBuzzWahl() {
    const w = $('buzz-wahl')
    if (data.lead || !data.teams.length) { w.hidden = true; return }
    if (myTeam && !data.teams.some(t => t.id === myTeam)) { myTeam = 0; store.set(teamKey, '') }
    w.hidden = false; w.innerHTML = ''
    const l = document.createElement('p'); l.className = 'quiz-buzz-label'
    l.textContent = myTeam ? 'Buzzer für' : 'Für den Buzzer dein Team wählen:'
    w.append(l)
    data.teams.forEach(t => {
      if (myTeam && t.id !== myTeam) return
      const b = document.createElement('button'); b.type = 'button'; b.className = 'btn btn-secondary'; b.textContent = t.name
      b.setAttribute('aria-pressed', t.id === myTeam ? 'true' : 'false')
      b.addEventListener('click', () => chooseTeam(t.id)); w.append(b)
    })
    if (!myTeam) $('buzz-wach').hidden = true
    if (myTeam) {
      const c = document.createElement('button'); c.type = 'button'; c.className = 'btn-link'; c.textContent = 'Team wechseln'
      c.addEventListener('click', () => chooseTeam(0)); w.append(c)
    }
  }

  function renderBuzz() {
    const q = data.current
    if ((q?.id ?? null) !== lastQ) { pressed.clear(); lastQ = q?.id ?? null; $('buzz-info').textContent = '' }
    const btnB = $('buzz-btn')
    const can = !data.lead && myTeam && q && !q.revealed && !pressed.has(q.id)
    btnB.hidden = !can
    if (can) {
      const ready = !!bestSync()
      btnB.disabled = !ready
      btnB.textContent = ready ? 'Buzzer' : 'Verbinde …'
      btnB.setAttribute('aria-label', `Buzzer für ${data.teams.find(t => t.id === myTeam)?.name || ''}`)
    }
    const list = $('buzz-reihe'); list.innerHTML = ''
    const z = data.buzz
    list.hidden = !(q && z && z.reihe.length)
    $('buzz-kopf').hidden = list.hidden
    if (list.hidden) return
    $('buzz-kopf').textContent = z.fertig ? 'Buzzer' : 'Buzzer: Reihenfolge wird ermittelt …'
    z.reihe.forEach((r, i) => {
      const li = document.createElement('li')
      if (i === 0 && z.fertig && !r.spaet) li.className = 'is-first'
      const zus = [r.abstand ? `+${Math.round(r.abstand)} ms` : '', r.gleichauf ? 'gleichauf' : '', r.spaet ? 'zu spät' : '', r.unsicher ? 'Zeit unsicher' : ''].filter(Boolean).join(', ')
      li.append(Object.assign(document.createElement('strong'), { textContent: r.name }))
      if (zus) li.append(Object.assign(document.createElement('span'), { textContent: ` ${zus}` }))
      list.append(li)
    })
    // während des Sammelfensters noch einmal nachsehen, dann steht die Reihenfolge
    clearTimeout(buzzTimer)
    if (!z.fertig) buzzTimer = setTimeout(load, 600)
  }

  const finished = () => data.board.length > 0 && !data.current && data.board.every(f => f.played)

  async function post(path, body) {
    const r = await fetch(api + path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body || {}) })
    if (!r.ok) throw new Error((await r.text()) || `Fehler ${r.status}`)
    return r.json()
  }
  // Schaltflächen sperren, bis der Server geantwortet hat: kein doppeltes Werten
  async function act(fn) {
    if (busy) return
    busy = true
    try { await fn() } catch (e) { alert(e.message) } finally { busy = false; await load() }
  }
  function btn(label, cls, fn) {
    const b = document.createElement('button'); b.type = 'button'; b.className = cls; b.textContent = label
    b.addEventListener('click', () => act(fn)); return b
  }

  function renderBoard() {
    const b = $('board'); b.innerHTML = ''
    const cats = [...new Set(data.board.map(f => f.cat))]
    if (!cats.length) { b.hidden = true; return }
    b.hidden = !!data.current || finished()
    const cols = cats.map(c => data.board.filter(f => f.cat === c).sort((x, y) => x.points - y.points))
    const rows = Math.max(...cols.map(c => c.length))
    b.style.gridTemplateColumns = `repeat(${cats.length}, minmax(0, 1fr))`
    b.style.gridTemplateRows = `auto repeat(${rows}, 1fr)`
    cats.forEach(c => { const h = document.createElement('div'); h.className = 'quiz-cat'; h.textContent = c; b.append(h) })
    for (let r = 0; r < rows; r++) {
      cols.forEach(col => {
        const f = col[r]
        if (!f) { b.append(document.createElement('div')); return }
        const label = `${f.cat}, ${f.points} Punkte${f.played ? ', gespielt' : ''}`
        if (data.lead) {
          // gespielte Felder gesperrt: ein Fehlklick öffnet keine alte Frage, zurück geht es mit „Rückgängig“
          const cell = btn(String(f.points), 'quiz-cell' + (f.played ? ' is-played' : ''), () => post('/feld', { field: f.id }))
          cell.disabled = !!f.played
          cell.setAttribute('aria-label', label); b.append(cell)
        } else {
          const cell = document.createElement('div'); cell.className = 'quiz-cell' + (f.played ? ' is-played' : '')
          cell.textContent = f.points; cell.setAttribute('aria-label', label); cell.setAttribute('role', 'img'); b.append(cell)
        }
      })
    }
  }

  function renderQuestion() {
    const q = data.current
    $('question').hidden = !q
    if (!q) return
    $('q-meta').textContent = `${q.cat} · ${q.points} Punkte`
    $('q-text').textContent = q.question
    const a = $('q-answer')
    // Auch bei der Leitung erst nach dem Aufdecken: ihre Ansicht läuft am Beamer
    a.hidden = !q.revealed
    a.textContent = q.revealed ? (q.answer || '') : ''
    const box = $('q-actions'); box.innerHTML = ''
    if (!data.lead) return
    if (!q.revealed) box.append(btn('Antwort aufdecken', 'btn btn-primary', () => post('/feld', { field: q.id, revealed: true })))
    const grid = document.createElement('div'); grid.className = 'quiz-teams'
    data.teams.forEach(t => {
      const g = document.createElement('div'); g.className = 'quiz-team'
      const n = document.createElement('span'); n.className = 'quiz-team-name'; n.textContent = t.name
      const first = data.buzz?.fertig && data.buzz.reihe[0] && !data.buzz.reihe[0].spaet && data.buzz.reihe[0].team === t.id
      if (first) { g.classList.add('is-first'); n.textContent = `1. ${t.name}` }
      g.append(n, btn(`+${q.points}`, 'btn btn-secondary plus', () => post('/wertung', { field: q.id, team: t.id, plus: true })),
        btn(`−${q.points}`, 'btn btn-secondary minus', () => post('/wertung', { field: q.id, team: t.id, plus: false })))
      g.querySelector('.plus').setAttribute('aria-label', `${t.name} plus ${q.points}`)
      g.querySelector('.minus').setAttribute('aria-label', `${t.name} minus ${q.points}`)
      grid.append(g)
    })
    box.append(grid,
      btn('Niemand, Feld schließen', 'btn btn-secondary', () => post('/wertung', { field: q.id })),
      btn('Zurück zum Raster', 'btn-link', () => post('/feld', { field: null })))
  }

  function renderScores() {
    const s = $('scores'); s.innerHTML = ''
    const max = Math.max(0, ...data.teams.map(t => t.score))
    data.teams.forEach(t => {
      const d = document.createElement('div'); d.className = 'quiz-score' + (t.score === max && max > 0 ? ' is-lead' : '')
      const n = document.createElement('span'); n.textContent = t.name
      const p = document.createElement('strong'); p.textContent = t.score
      // kurz hervorheben, was sich gerade geändert hat; die Seite zeichnet oft zweimal kurz hintereinander,
      // deshalb läuft die Animation beim Neuzeichnen an der alten Stelle weiter
      if (lastScores[t.id] !== undefined && lastScores[t.id] !== t.score) changedAt[t.id] = Date.now()
      const since = Date.now() - (changedAt[t.id] || 0)
      if (since < 1600) { d.classList.add('is-changed'); d.style.animationDelay = `-${since}ms` }
      d.append(n, p); s.append(d)
    })
    lastScores = Object.fromEntries(data.teams.map(t => [t.id, t.score]))
    const open = data.board.filter(f => !f.played).length
    if (data.board.length) { const o = document.createElement('p'); o.className = 'hint'; o.textContent = `${open} von ${data.board.length} Fragen offen`; s.append(o) }
  }

  // Abschluss nach der letzten Frage: Platzierung und die Fragen, über die es sich zu sprechen lohnt
  function renderEnde() {
    const e = $('ende'); e.innerHTML = ''
    e.hidden = !finished()
    if (e.hidden) return
    const el = (tag, cls, text) => { const x = document.createElement(tag); if (cls) x.className = cls; if (text !== undefined) x.textContent = text; return x }
    const h = el('h2', '', 'Ergebnis'); h.id = 'ende-h'
    const ranked = [...data.teams].sort((a, b) => b.score - a.score)
    const top = ranked.filter(t => t.score === ranked[0]?.score)
    const sieg = el('p', 'quiz-sieg', !ranked.length ? '' : top.length > 1 ? `Gleichstand: ${top.map(t => t.name).join(' und ')}` : `${top[0].name} gewinnt`)
    const ol = el('ol', 'quiz-rang')
    ranked.forEach(t => { const li = el('li'); li.append(el('span', '', t.name), el('strong', '', String(t.score))); ol.append(li) })
    const schwer = data.board.filter(f => f.schwer)
    const h3 = el('h3', '', 'Zum Nachbesprechen')
    const hint = el('p', 'hint', schwer.length ? 'Diese Fragen hat niemand oder erst nach einem Fehlversuch gelöst:' : 'Alle Fragen wurden auf Anhieb gelöst.')
    const ul = el('ul', 'quiz-nach')
    schwer.forEach(f => { const li = el('li'); li.append(el('span', 'quiz-meta-klein', `${f.cat} · ${f.points}`), el('span', '', f.question || '')); ul.append(li) })
    e.append(h, sieg, ol, h3, hint, ul)
    if (data.lead) e.append(el('p', 'hint', '„Rückgängig“ holt die letzte Frage zurück, „Fragen bearbeiten“ beginnt ein neues Spiel.'))
  }

  function render() {
    document.title = `${data.title || 'Quiz-Raster'} · NextPad`
    $('quiz-title').textContent = data.title || 'Quiz-Raster'
    $('quiz-kicker').textContent = data.lead ? 'Quiz-Raster · Leitung' : 'Quiz-Raster · Mitsehen'
    $('lead-tools').hidden = !data.lead
    if (data.lead) {
      $('q-undo').disabled = !data.canUndo
      // während einer Frage nur Aufdecken, Wertung und Vollbild: der Rest lenkt ab
      for (const id of ['q-undo', 'q-qr', 'q-edit', 'q-messung']) $(id).hidden = !!data.current
      if (data.current) $('qr-box').hidden = true
      const setup = $('setup')
      // Formular nur füllen, wenn es zu ist: sonst überschreibt ein Live-Update die Eingabe
      if (setup.hidden || !setup.dataset.filled) {
        $('in-title').value = data.setup.title; $('in-teams').value = data.setup.teams; $('in-table').value = data.setup.table
        setup.dataset.filled = '1'
      }
      if (!data.board.length && setup.hidden) toggleSetup(true)
    }
    const empty = $('empty')
    empty.hidden = data.board.length > 0 || data.lead
    empty.textContent = 'Die Leitung richtet das Quiz gerade ein.'
    renderBuzzWahl(); renderBoard(); renderQuestion(); renderBuzz(); renderEnde(); renderScores()
  }

  async function load() {
    try {
      const r = await fetch(api, { cache: 'no-store' })
      if (!r.ok) throw new Error(await r.text())
      data = await r.json(); render()
    } catch (e) { console.warn('Quiz laden', e) }
  }

  function toggleSetup(open) {
    const s = $('setup'); s.hidden = !open
    $('q-edit').setAttribute('aria-expanded', open ? 'true' : 'false')
    if (open) { s.dataset.filled = ''; if (data) { $('in-title').value = data.setup.title; $('in-teams').value = data.setup.teams; $('in-table').value = data.setup.table; s.dataset.filled = '1' } $('in-table').focus() }
  }
  $('q-edit').addEventListener('click', () => toggleSetup($('setup').hidden))
  $('q-save').addEventListener('click', () => {
    const err = $('setup-error')
    err.hidden = true
    if (data.canUndo && !confirm('Neu beginnen? Alle Punkte gehen verloren.')) return
    act(async () => {
      try { await post('/fragen', { title: $('in-title').value, teams: $('in-teams').value, table: $('in-table').value }); toggleSetup(false) }
      catch (e) { err.textContent = e.message; err.hidden = false; throw new Error('Nicht gespeichert: ' + e.message) }
    })
  })
  $('q-undo').addEventListener('click', () => act(() => post('/rueckgaengig')))
  $('q-qr').addEventListener('click', () => { const b = $('qr-box'); b.hidden = !b.hidden; $('q-qr').setAttribute('aria-expanded', b.hidden ? 'false' : 'true') })
  $('q-full').addEventListener('click', () => { document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen?.() })
  if (!$('qr-box').textContent.trim()) $('q-qr').hidden = true

  // Tasten für die Leitung: Leertaste deckt auf, Esc zurück zum Raster, Z nimmt die letzte Wertung zurück
  document.addEventListener('keydown', e => {
    if (!data?.lead || e.target.closest?.('input, textarea') || e.ctrlKey || e.metaKey || e.altKey) return
    const q = data.current
    // Leertaste auf einer Schaltfläche löst diese aus, nicht das Aufdecken
    if (e.key === ' ' && q && !q.revealed && !e.target.closest?.('button, a')) { e.preventDefault(); act(() => post('/feld', { field: q.id, revealed: true })) }
    else if (e.key === 'Escape' && q) act(() => post('/feld', { field: null }))
    else if ((e.key === 'z' || e.key === 'Z') && data.canUndo) act(() => post('/rueckgaengig'))
  })

  load()
  // Live: der Server meldet nur „geändert“, den Stand holt jede Seite mit ihren eigenen Rechten
  const es = new EventSource(`${api}/live`)
  es.addEventListener('change', () => load())
  es.addEventListener('open', () => load())
})()
