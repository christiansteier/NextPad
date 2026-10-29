// Quiz-Raster: Die Leitung steuert am Beamer, alle anderen sehen live mit. Der Server prüft alles und gibt Antworten
// erst nach dem Aufdecken an Mitsehende heraus; hier wird nur angezeigt.
(() => {
  const cfg = JSON.parse(document.getElementById('nextpad-config').textContent)
  const T = cfg.texte || {}
  const tx = (k, w = {}) => (T[k] || k).replace(/\{(\w+)\}/g, (_, n) => String(w[n] ?? ''))
  // aus Moodle gestartet: zurück in den Kurs, auch aus der eingebetteten Ansicht heraus
  if (cfg.course) { const a = document.createElement('a'); a.href = cfg.course; a.target = '_top'; a.textContent = tx('zum_kurs'); document.querySelector('.site-header nav')?.prepend(a) }
  const base = cfg.basePath || ''
  const api = `${base}/p/${encodeURIComponent(cfg.urlId)}/quiz`
  const $ = id => document.getElementById(id)
  let data = null
  let busy = false
  let lastScores = {}
  const hasQr = !!$('qr-box').textContent.trim()
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
  // Zeitgrenze je Abgleich: eine hängende Verbindung hält sonst den alten Versatz minutenlang fest
  const frist = t => (typeof AbortSignal !== 'undefined' && AbortSignal.timeout) ? { signal: AbortSignal.timeout(t) } : {}
  async function serie() {
    const s = []
    for (let i = 0; i < 5; i++) {
      const t0 = now()
      const j = await (await fetch(`${base}/p/${encodeURIComponent(cfg.urlId)}/messung/zeit`, { cache: 'no-store', ...frist(3000) })).json()
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
    $('buzz-info').textContent = tx('gedrueckt')
    renderBuzz()
    post('/buzzer', { dev, team: myTeam, field: q.id, t, off: b.off, rtt: b.rtt })
      .then(r => { if (!r.neu) $('buzz-info').textContent = tx('schon') })
      .catch(e => { $('buzz-info').textContent = tx('nicht_angekommen', { grund: e.message }) })
  }
  // pointerdown statt click: der Druck zählt beim Antippen, nicht beim Loslassen; click bleibt für die Tastatur
  $('buzz-btn').addEventListener('pointerdown', e => { e.preventDefault(); press() })
  $('buzz-btn').addEventListener('click', press)

  function renderBuzzWahl() {
    const w = $('buzz-wahl')
    // am Ende des Spiels gibt es nichts mehr zu buzzern
    if (data.lead || !data.teams.length || finished()) { w.hidden = true; return }
    if (myTeam && !data.teams.some(t => t.id === myTeam)) { myTeam = 0; store.set(teamKey, '') }
    w.hidden = false; w.innerHTML = ''
    const l = document.createElement('p'); l.className = 'quiz-buzz-label'
    l.textContent = myTeam ? tx('buzzer_fuer') : tx('team_waehlen')
    w.append(l)
    data.teams.forEach(t => {
      if (myTeam && t.id !== myTeam) return
      const b = document.createElement('button'); b.type = 'button'; b.className = 'btn btn-secondary'; b.textContent = t.name
      b.setAttribute('aria-pressed', t.id === myTeam ? 'true' : 'false')
      b.addEventListener('click', () => chooseTeam(t.id)); w.append(b)
    })
    if (!myTeam) $('buzz-wach').hidden = true
    if (myTeam) {
      // wechseln nur vor dem Spiel; läuft es (Frage offen oder schon gespielt), bleibt das Team
      const laeuft = data.phase !== 'vorbereitung'
      if (!laeuft) {
        const c = document.createElement('button'); c.type = 'button'; c.className = 'btn-link'; c.textContent = tx('team_wechseln')
        c.addEventListener('click', () => chooseTeam(0)); w.append(c)
      }
      // ohne offene Frage kein Buzzer: sagen, worauf das Handy wartet
      if (!data.current) w.append(Object.assign(document.createElement('p'), { className: 'hint quiz-buzz-warten', textContent: data.phase === 'vorbereitung' ? tx('bereit_gleich') : tx('bereit_frage') }))
    }
  }

  // Buzzer-Ton, erzeugt statt geladen; Browser erlauben Ton erst nach einer Bedienung, darum entsteht er beim ersten Klick
  let audio = null, tonFrage = null
  let tonAn = (() => { try { return localStorage.getItem('nextpad-quiz-ton') !== 'aus' } catch { return true } })()
  document.addEventListener('pointerdown', () => {
    if (!data?.lead || audio) return
    try { audio = new (window.AudioContext || window.webkitAudioContext)() } catch {}
  }, { capture: true })
  function buzzerTon() {
    if (!tonAn || !audio) return
    try {
      if (audio.state === 'suspended') audio.resume()
      const t = audio.currentTime, g = audio.createGain(), f = audio.createBiquadFilter()
      f.type = 'lowpass'; f.frequency.value = 1400
      g.gain.setValueAtTime(0, t); g.gain.linearRampToValueAtTime(0.25, t + 0.02); g.gain.setValueAtTime(0.25, t + 0.5); g.gain.linearRampToValueAtTime(0, t + 0.62)
      for (const hz of [150, 157]) { const o = audio.createOscillator(); o.type = 'sawtooth'; o.frequency.value = hz; o.connect(f); o.start(t); o.stop(t + 0.65) }
      f.connect(g); g.connect(audio.destination)
    } catch {}
  }
  function zeigeTon() { $('q-ton').textContent = tonAn ? tx('ton_an') : tx('ton_aus'); $('q-ton').setAttribute('aria-pressed', tonAn ? 'true' : 'false') }

  const sek = ms => { const z = (ms / 1000).toFixed(2); return `${cfg.lang === 'en' ? z : z.replace('.', ',')} s` }
  // Countdown: Restzeit kommt vom Server, gezählt wird hier; nach Ablauf schließt der Buzzer
  let fristBis = null, tickTimer = 0
  const zeitUm = () => fristBis != null && performance.now() >= fristBis
  function fristSetzen() {
    const q = data.current
    fristBis = q && q.restMs != null ? performance.now() + q.restMs : null
    clearInterval(tickTimer)
    if (fristBis != null && !zeitUm()) tickTimer = setInterval(tick, 200)
    tick()
  }
  function tick() {
    const el = $('q-countdown'), q = data?.current
    if (!q || fristBis == null || q.revealed) { el.hidden = true; clearInterval(tickTimer); return }
    const rest = Math.max(0, Math.ceil((fristBis - performance.now()) / 1000))
    // ist schon gedrückt, zeigt der Beamer das Team statt der Uhr
    el.hidden = !!data.buzz?.reihe?.length
    el.textContent = rest > 0 ? String(rest) : tx('zeit_um')
    el.classList.toggle('is-knapp', rest > 0 && rest <= 3)
    el.classList.toggle('is-um', rest === 0)
    if (rest === 0) { clearInterval(tickTimer); renderBuzz() }
  }

  function renderBuzz() {
    const q = data.current
    if ((q?.id ?? null) !== lastQ) { pressed.clear(); lastQ = q?.id ?? null; $('buzz-info').textContent = '' }
    const btnB = $('buzz-btn')
    const can = !data.lead && myTeam && q && !q.revealed && !pressed.has(q.id) && !zeitUm()
    btnB.hidden = !can
    if (!data.lead && myTeam && q && !q.revealed && !pressed.has(q.id) && zeitUm()) $('buzz-info').textContent = tx('zeit_ist_um')
    if (can) {
      const ready = !!bestSync()
      btnB.disabled = !ready
      btnB.textContent = ready ? tx('buzzer') : tx('verbinde')
      btnB.setAttribute('aria-label', tx('buzzer_team', { team: data.teams.find(t => t.id === myTeam)?.name || '' }))
    }
    const z = data.buzz
    // Wer zuerst gedrückt hat, groß; erst wenn die Reihenfolge feststeht (Sammelfenster vorbei)
    // nach einer falschen Antwort ist das nächste Team in der Reihenfolge dran
    const erster = $('buzz-erster'), dran = z?.reihe?.find(r => !r.falsch), nachFalsch = !!z?.reihe?.some(r => r.falsch)
    const zeigen = !!(q && !q.revealed && z && z.fertig && dran && (nachFalsch || !dran.spaet))
    if (zeigen) {
      const gleich = nachFalsch ? [] : z.reihe.filter((r, i) => i > 0 && r.gleichauf && !r.spaet).map(r => r.name)
      const text = gleich.length ? tx('gleichauf_namen', { namen: [dran.name, ...gleich].join(tx('und')) }) : dran.name
      const label = nachFalsch ? tx('jetzt_dran') : gleich.length ? tx('gleichzeitig') : `${tx('zuerst')}${dran.reaktion != null ? ` · ${sek(dran.reaktion)}` : ''}`
      if (erster.dataset.text !== label + text) {
        erster.dataset.text = label + text
        erster.innerHTML = ''
        erster.append(Object.assign(document.createElement('small'), { textContent: label }), Object.assign(document.createElement('strong'), { textContent: text }))
      }
    } else { erster.dataset.text = ''; erster.innerHTML = '' }
    erster.hidden = !zeigen
    // Ton bei der Leitung (Beamer), sobald der erste Druck einer Frage ankommt
    const n = q && z ? z.reihe.length : 0
    if (data.lead && q && n > 0 && tonFrage !== q.id) { tonFrage = q.id; buzzerTon() }
    const list = $('buzz-reihe'); list.innerHTML = ''
    list.hidden = !(q && z && z.reihe.length)
    $('buzz-kopf').hidden = list.hidden
    if (list.hidden) return
    $('buzz-kopf').textContent = z.fertig ? tx('buzzer') : tx('reihenfolge')
    z.reihe.forEach((r, i) => {
      const li = document.createElement('li')
      if (r === dran && z.fertig && (nachFalsch || !r.spaet)) li.className = 'is-first'
      const zus = [r.abstand ? `+${Math.round(r.abstand)} ms` : '', r.gleichauf ? tx('gleichauf') : '', r.spaet ? tx('zu_spaet') : '', r.unsicher ? tx('unsicher') : '', r.falsch ? tx('falsch') : ''].filter(Boolean).join(', ')
      li.append(Object.assign(document.createElement('strong'), { textContent: r.name }))
      if (zus) li.append(Object.assign(document.createElement('span'), { textContent: ` ${zus}` }))
      list.append(li)
    })
    // während des Sammelfensters noch einmal nachsehen, dann steht die Reihenfolge
    clearTimeout(buzzTimer)
    if (!z.fertig) buzzTimer = setTimeout(load, 600)
  }

  // Ergebnis: beendet über „Spiel beenden“ oder alle Felder gespielt
  const finished = () => data.phase === 'ende' || (data.board.length > 0 && !data.current && data.board.every(f => f.played))

  async function post(path, body) {
    const r = await fetch(api + path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body || {}) })
    if (!r.ok) throw new Error((await r.text()) || tx('fehler', { status: r.status }))
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
    b.classList.toggle('is-gesperrt', data.phase === 'vorbereitung')
    const cols = cats.map(c => data.board.filter(f => f.cat === c).sort((x, y) => x.points - y.points))
    const rows = Math.max(...cols.map(c => c.length))
    b.style.gridTemplateColumns = `repeat(${cats.length}, minmax(0, 1fr))`
    b.style.gridTemplateRows = `auto repeat(${rows}, 1fr)`
    cats.forEach(c => { const h = document.createElement('div'); h.className = 'quiz-cat'; h.textContent = c; b.append(h) })
    for (let r = 0; r < rows; r++) {
      cols.forEach(col => {
        const f = col[r]
        if (!f) { b.append(document.createElement('div')); return }
        const label = tx(f.played ? 'feld_gespielt' : 'feld', { kategorie: f.cat, punkte: f.points })
        if (data.lead) {
          // gespielte Felder gesperrt: ein Fehlklick öffnet keine alte Frage, zurück geht es mit „Rückgängig“
          const cell = btn(String(f.points), 'quiz-cell' + (f.played ? ' is-played' : ''), () => post('/feld', { field: f.id }))
          cell.disabled = !!f.played || data.phase !== 'laeuft' // vor dem Start ist das Raster gesperrt
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
    $('q-meta').textContent = tx('meta', { kategorie: q.cat, punkte: q.points })
    $('q-text').textContent = q.question
    const a = $('q-answer')
    // Auch bei der Leitung erst nach dem Aufdecken: ihre Ansicht läuft am Beamer
    a.hidden = !q.revealed
    a.textContent = q.revealed ? (q.answer || '') : ''
    const box = $('q-actions'); box.innerHTML = ''
    if (!data.lead) return
    if (!q.revealed) box.append(btn(tx('aufdecken'), 'btn btn-primary', () => post('/feld', { field: q.id, revealed: true })))
    const grid = document.createElement('div'); grid.className = 'quiz-teams'
    data.teams.forEach(t => {
      const g = document.createElement('div'); g.className = 'quiz-team'
      const n = document.createElement('span'); n.className = 'quiz-team-name'; n.textContent = t.name
      // markiert, wer gerade dran ist: zuerst gedrückt oder nach einer falschen Antwort das nächste Team
      const z = data.buzz, dran = z?.reihe?.find(r => !r.falsch), nachFalsch = !!z?.reihe?.some(r => r.falsch)
      if (z?.fertig && dran && (nachFalsch || !dran.spaet) && dran.team === t.id) { g.classList.add('is-first'); n.textContent = nachFalsch ? tx('dran', { team: t.name }) : `1. ${t.name}` }
      g.append(n, btn(`+${q.points}`, 'btn btn-secondary plus', () => post('/wertung', { field: q.id, team: t.id, plus: true })),
        btn(`−${q.points}`, 'btn btn-secondary minus', () => post('/wertung', { field: q.id, team: t.id, plus: false })))
      g.querySelector('.plus').setAttribute('aria-label', tx('plus', { team: t.name, n: q.points }))
      g.querySelector('.minus').setAttribute('aria-label', tx('minus', { team: t.name, n: q.points }))
      grid.append(g)
    })
    box.append(grid,
      btn(tx('niemand'), 'btn btn-secondary', () => post('/wertung', { field: q.id })),
      btn(tx('zum_raster'), 'btn-link', () => post('/feld', { field: null })))
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
    if (data.board.length) { const o = document.createElement('p'); o.className = 'hint'; o.textContent = tx('offen', { offen: open, alle: data.board.length }); s.append(o) }
  }

  // Abschluss nach der letzten Frage: Platzierung und die Fragen, über die es sich zu sprechen lohnt
  function renderEnde() {
    const e = $('ende'); e.innerHTML = ''
    e.hidden = !finished()
    if (e.hidden) return
    const el = (tag, cls, text) => { const x = document.createElement(tag); if (cls) x.className = cls; if (text !== undefined) x.textContent = text; return x }
    const h = el('h2', '', tx('ergebnis')); h.id = 'ende-h'
    const ranked = [...data.teams].sort((a, b) => b.score - a.score)
    const top = ranked.filter(t => t.score === ranked[0]?.score)
    const sieg = el('p', 'quiz-sieg', !ranked.length ? '' : top.length > 1 ? tx('gleichstand', { namen: top.map(t => t.name).join(tx('und')) }) : tx('gewinnt', { team: top[0].name }))
    const ol = el('ol', 'quiz-rang')
    ranked.forEach(t => { const li = el('li'); li.append(el('span', '', t.name), el('strong', '', String(t.score))); ol.append(li) })
    const schwer = data.board.filter(f => f.schwer)
    const h3 = el('h3', '', tx('nachbesprechen'))
    const hint = el('p', 'hint', schwer.length ? tx('schwer') : tx('alle_geloest'))
    const ul = el('ul', 'quiz-nach')
    schwer.forEach(f => { const li = el('li'); li.append(el('span', 'quiz-meta-klein', `${f.cat} · ${f.points}`), el('span', '', f.question || '')); ul.append(li) })
    e.append(h, sieg, ol)
    const st = data.statistik
    if (st) {
      const sl = el('ul', 'quiz-stat')
      st.teams.forEach(t => sl.append(el('li', '', tx('stat_team', { team: t.name, zeit: sek(t.mittel), n: t.anzahl }))))
      e.append(el('h3', '', tx('buzzer')), el('p', '', tx('schnellste', { team: st.schnellste.name, zeit: sek(st.schnellste.ms) })), sl)
    }
    e.append(h3, hint, ul)
    if (data.lead) e.append(el('p', 'hint', tx('neu_hinweis')))
  }

  let messungTimer = 0
  function render() {
    document.title = `${data.title || tx('name')} · NextPad`
    $('quiz-title').textContent = data.title || tx('name')
    $('quiz-kicker').textContent = data.lead ? tx('leitung') : tx('mitsehen')
    $('lead-tools').hidden = !data.lead
    // Spielablauf: Vorbereitung (Teams wählen) → läuft → Ergebnis
    const ph = $('q-phase')
    // nur für die Leitung; die Handys sagen es bei der Teamwahl
    ph.hidden = !data.lead || data.phase !== 'vorbereitung' || !data.board.length
    ph.textContent = tx('vorbereitung')
    // Mitmessen nur, solange die Leitung die Messseite offen hat und keine Frage läuft
    $('viewer-tools').hidden = !data.messung || !!data.current
    clearTimeout(messungTimer)
    if (data.messung) messungTimer = setTimeout(load, 15000) // schließt die Leitung die Messseite, verschwindet der Knopf
    if (data.lead) {
      $('q-undo').disabled = !data.canUndo
      $('q-start').hidden = data.phase !== 'vorbereitung' || !data.board.length
      $('q-stop').hidden = data.phase !== 'laeuft' || !!data.current || finished()
      $('q-neu').hidden = !finished()
      // während einer Frage nur Aufdecken, Wertung und Vollbild: der Rest lenkt ab
      for (const id of ['q-undo', 'q-qr', 'q-edit', 'q-messung']) $(id).hidden = !!data.current
      // ohne Code (Moodle-Aktivität, die noch nie aus Moodle gestartet wurde) kein QR-Knopf
      if (!hasQr) $('q-qr').hidden = true
      if (data.current) $('qr-box').hidden = true
      const setup = $('setup')
      // Formular nur füllen, wenn es zu ist: sonst überschreibt ein Live-Update die Eingabe
      if (setup.hidden || !setup.dataset.filled) {
        $('in-title').value = data.setup.title; $('in-teams').value = data.setup.teams; $('in-table').value = data.setup.table; $('in-sek').value = data.setup.sekunden
        setup.dataset.filled = '1'
      }
      if (!data.board.length && setup.hidden) toggleSetup(true)
    }
    const empty = $('empty')
    empty.hidden = data.board.length > 0 || data.lead
    empty.textContent = tx('richtet_ein')
    // Team schon gewählt (Seite neu geladen, zurück von der Messung, neu aus Moodle gestartet): Abgleich hier starten,
    // sonst wird der Buzzer nie bereit
    if (!data.lead && myTeam && !syncing) { syncLoop(); wachHalten() }
    fristSetzen()
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
    if (open) { s.dataset.filled = ''; if (data) { $('in-title').value = data.setup.title; $('in-teams').value = data.setup.teams; $('in-table').value = data.setup.table; $('in-sek').value = data.setup.sekunden; s.dataset.filled = '1' } $('in-table').focus() }
  }
  $('q-edit').addEventListener('click', () => toggleSetup($('setup').hidden))
  $('q-save').addEventListener('click', () => {
    const err = $('setup-error')
    err.hidden = true
    if (data.canUndo && !confirm(tx('neu_beginnen'))) return
    act(async () => {
      try { await post('/fragen', { title: $('in-title').value, teams: $('in-teams').value, table: $('in-table').value, sekunden: Number($('in-sek').value || 0) }); toggleSetup(false) }
      catch (e) { err.textContent = e.message; err.hidden = false; throw new Error(tx('nicht_gespeichert', { grund: e.message })) }
    })
  })
  $('q-undo').addEventListener('click', () => act(() => post('/rueckgaengig')))
  $('q-ton').addEventListener('click', () => { tonAn = !tonAn; try { localStorage.setItem('nextpad-quiz-ton', tonAn ? 'an' : 'aus') } catch {}; zeigeTon(); if (tonAn) buzzerTon() })
  zeigeTon()
  // Menü schließt nach einer Auswahl und bei einem Klick daneben
  $('q-menu').addEventListener('click', e => { if (e.target.closest('.quiz-menu-list button, .quiz-menu-list a')) $('q-menu').open = false })
  document.addEventListener('click', e => { if (!e.target.closest('#q-menu')) $('q-menu').open = false })
  $('q-start').addEventListener('click', () => act(() => post('/spiel', { aktion: 'start' })))
  $('q-stop').addEventListener('click', () => { if (confirm(tx('beenden_frage'))) act(() => post('/spiel', { aktion: 'ende' })) })
  $('q-neu').addEventListener('click', () => { if (confirm(tx('neues_frage'))) act(() => post('/spiel', { aktion: 'neu' })) })
  $('q-qr').addEventListener('click', () => { const b = $('qr-box'); b.hidden = !b.hidden; $('q-qr').setAttribute('aria-expanded', b.hidden ? 'false' : 'true') })
  $('q-full').addEventListener('click', () => { document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen?.() })

  // Tasten für die Leitung: Leertaste deckt auf, Esc zurück zum Raster, Z nimmt die letzte Wertung zurück
  document.addEventListener('keydown', e => {
    if (!data?.lead || e.target.closest?.('input, textarea') || e.ctrlKey || e.metaKey || e.altKey) return
    const q = data.current
    // Leertaste auf einer Schaltfläche löst diese aus, nicht das Aufdecken
    if (e.key === ' ' && q && !q.revealed && !e.target.closest?.('button, a, summary')) { e.preventDefault(); act(() => post('/feld', { field: q.id, revealed: true })) }
    else if (e.key === 'Escape' && q) act(() => post('/feld', { field: null }))
    else if ((e.key === 'z' || e.key === 'Z') && data.canUndo) act(() => post('/rueckgaengig'))
  })

  load()
  // Live: der Server meldet nur „geändert“, den Stand holt jede Seite mit ihren eigenen Rechten
  const es = new EventSource(`${api}/live`)
  es.addEventListener('change', () => load())
  es.addEventListener('open', () => load())
})()
