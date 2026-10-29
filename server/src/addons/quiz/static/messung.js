// Buzzer-Messung: Handys gleichen ihre Uhr ab und drücken auf ein Signal, die Leitung sieht die Auswertung.
// Rechnen tut der Server; hier wird nur gemessen und angezeigt.
(() => {
  const cfg = JSON.parse(document.getElementById('nextpad-config').textContent)
  const T = cfg.texte || {}
  const tx = (k, w = {}) => (T[k] || k).replace(/\{(\w+)\}/g, (_, n) => String(w[n] ?? ''))
  const base = cfg.basePath || ''
  const pad = `${base}/p/${encodeURIComponent(cfg.urlId)}`
  const api = pad + '/messung'
  const $ = id => document.getElementById(id)
  // hochauflösend und monoton; der Versatz zur Serveruhr wird ohnehin laufend neu bestimmt
  const now = () => performance.timeOrigin + performance.now()
  const ms = x => x == null ? '–' : `${Math.round(x)} ms`
  // Zeitgrenze je Anfrage: eine hängende Verbindung hält sonst den Abgleich minutenlang auf
  const frist = t => (typeof AbortSignal !== 'undefined' && AbortSignal.timeout) ? { signal: AbortSignal.timeout(t) } : {}

  async function post(path, body) {
    const r = await fetch(api + path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body || {}) })
    if (!r.ok) throw new Error((await r.text()) || tx('fehler', { status: r.status }))
    return r.json()
  }
  const stand = async () => (await fetch(api + '/stand', { cache: 'no-store' })).json()

  // ---- Handy ----
  function devId() {
    try { const d = sessionStorage.getItem('nextpad-messung'); if (d) return d } catch {}
    const a = new Uint8Array(12); crypto.getRandomValues(a)
    const d = [...a].map(b => b.toString(36).padStart(2, '0')).join('').slice(0, 20)
    try { sessionStorage.setItem('nextpad-messung', d) } catch {}
    return d
  }
  const dev = devId()
  const recent = [] // die letzten drei Abgleiche; der mit der kürzesten Laufzeit gilt
  let started = false, runde = 0, signalT = 0, abgleiche = 0, closeTimer = 0, wake = null
  const best = () => recent.reduce((a, b) => (!a || b.rtt < a.rtt ? b : a), null)

  async function serie() {
    const s = []
    for (let i = 0; i < 5; i++) {
      const t0 = now()
      const j = await (await fetch(api + '/zeit', { cache: 'no-store', ...frist(3000) })).json()
      const t1 = now()
      s.push({ rtt: t1 - t0, off: j.t - (t0 + t1) / 2 })
    }
    s.sort((a, b) => a.rtt - b.rtt)
    const b = { rtt: s[0].rtt, off: s[0].off, rttMed: s[2].rtt }
    recent.push(b); if (recent.length > 3) recent.shift()
    await post('/probe', { dev, rtt: b.rtt, rttMed: b.rttMed, off: b.off })
    abgleiche++
    $('m-status').textContent = tx('misst', { n: abgleiche, laufzeit: ms(b.rtt) })
  }
  async function loop() {
    while (started) {
      try { await serie() } catch (e) { $('m-status').textContent = tx('unterbrochen', { grund: e.message }) }
      await new Promise(r => setTimeout(r, 3000))
    }
  }
  // Bildschirm anlassen; erzwingen lässt es sich nicht. Lehnt der Browser ab oder kann es nicht, steht ein Hinweis da.
  async function wachHalten() {
    wake = null
    try { if (navigator.wakeLock) wake = await navigator.wakeLock.request('screen') } catch {}
    $('m-wach').hidden = !!wake
  }
  document.addEventListener('visibilitychange', () => { if (started && document.visibilityState === 'visible') wachHalten() })

  function zeigeBuzzer(an) {
    const b = $('m-buzzer')
    b.disabled = !an
    b.classList.toggle('is-jetzt', an)
    b.textContent = an ? tx('jetzt') : tx('warten')
  }
  async function signalPruefen(tr) {
    if (!started) return
    const st = await stand()
    if (st.runde <= runde) return
    runde = st.runde
    const b = best()
    if (!st.offen || !b) return
    signalT = tr
    $('m-reaktion').textContent = ''
    zeigeBuzzer(true)
    clearTimeout(closeTimer); closeTimer = setTimeout(() => zeigeBuzzer(false), 10000)
    const meldung = { dev, runde, t: tr, off: b.off, rtt: b.rtt }
    // einmal wiederholen; geht es dann nicht, sieht es die Person, statt dass die Runde still fehlt
    post('/empfang', meldung).catch(() => new Promise(r => setTimeout(r, 500)).then(() => post('/empfang', meldung)))
      .catch(e => { $('m-reaktion').textContent = tx('empfang_fehlt', { grund: e.message }) })
  }
  $('m-buzzer').addEventListener('click', () => {
    const tp = now()
    const b = best()
    zeigeBuzzer(false); clearTimeout(closeTimer)
    $('m-reaktion').textContent = tx('gedrueckt_nach', { zeit: ms(tp - signalT) })
    post('/druck', { dev, runde, t: tp, off: b.off, rtt: b.rtt }).catch(e => { $('m-reaktion').textContent = tx('nicht_angekommen', { grund: e.message }) })
  })
  $('m-start').addEventListener('click', async () => {
    if (started) return
    started = true
    $('m-start').hidden = true
    runde = (await stand()).runde // eine schon laufende Runde zählt nicht
    wachHalten()
    loop()
  })

  // ---- Leitung ----
  function wert(dt, dd) {
    const t = document.createElement('dt'); t.textContent = dt
    const d = document.createElement('dd'); d.textContent = dd
    return [t, d]
  }
  async function auswertung() {
    let a
    try { const r = await fetch(api + '/daten', { cache: 'no-store' }); if (!r.ok) return; a = await r.json() } catch { return }
    $('m-aktiv').textContent = a.aktiv
    $('m-urteil').textContent = a.urteil
    const v = a.vergleich
    $('m-werte').replaceChildren(
      ...wert(tx('w_geraete'), `${a.geraete} · ${a.abgleiche} · ${a.runden}`),
      ...wert(tx('w_laufzeit'), tx('w_laufzeit_wert', { median: ms(a.laufzeit.median), p95: ms(a.laufzeit.p95), ueber: a.laufzeit.ueber300 })),
      ...wert(tx('w_uhr'), tx('w_uhr_wert', { median: ms(a.uhr.median), p95: ms(a.uhr.p95) })),
      ...wert(tx('w_signal'), tx('w_signal_wert', { median: ms(a.signal.median), p95: ms(a.signal.p95), spanne: ms(a.signal.spanneMedian), max: ms(a.signal.spanneMax) })),
      ...wert(tx('w_netz'), tx('w_netz_wert', { median: ms(a.eingang.median), p95: ms(a.eingang.p95), spanne: ms(a.eingang.spanneMedian), max: ms(a.eingang.spanneMax) })),
      ...wert(tx('w_reaktion'), tx('w_reaktion_wert', { median: ms(a.reaktion.median) })),
      ...wert(tx('w_erster'), v.runden ? tx('w_erster_wert', { runden: v.runden, eingang: v.wechselEingang, reaktion: v.wechselReaktion, knapp: v.knapp }) : tx('w_erster_keine')),
    )
    $('m-liste').replaceChildren(...a.liste.map(g => {
      const tr = document.createElement('tr')
      for (const x of [tx('geraet', { n: g.nr }), g.art === 'andere' ? tx('andere') : g.art, g.abgleiche, ms(g.laufzeit), ms(g.uhr), g.druecke, g.zuletztS == null ? '–' : tx('vor', { n: g.zuletztS })]) {
        const td = document.createElement('td'); td.textContent = x; tr.append(td)
      }
      return tr
    }))
  }
  let sending = false
  async function signal() {
    if (sending) return
    sending = true
    try { await post('/signal') } catch (e) { alert(e.message) } finally { setTimeout(() => { sending = false }, 1000) }
  }
  $('m-signal').addEventListener('click', signal)
  $('m-loeschen').addEventListener('click', async () => {
    if (!confirm(tx('loeschen_frage'))) return
    try { await post('/loeschen') } catch (e) { alert(e.message) }
    auswertung()
  })

  ;(async () => {
    const st = await stand()
    if (st.lead) {
      $('m-lead').hidden = false
      document.addEventListener('keydown', e => {
        if (e.key !== ' ' || e.target.closest?.('input, textarea, button, a, summary')) return
        e.preventDefault(); signal()
      })
      auswertung(); setInterval(auswertung, 2000)
    } else {
      $('m-handy').hidden = false
      const es = new EventSource(`${pad}/quiz/live`)
      es.addEventListener('change', () => { signalPruefen(now()).catch(() => {}) })
    }
  })()
})()
