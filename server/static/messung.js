// Buzzer-Messung: Handys gleichen ihre Uhr ab und drücken auf ein Signal, die Leitung sieht die Auswertung.
// Rechnen tut der Server; hier wird nur gemessen und angezeigt.
(() => {
  const cfg = JSON.parse(document.getElementById('nextpad-config').textContent)
  const base = cfg.basePath || ''
  const pad = `${base}/p/${encodeURIComponent(cfg.urlId)}`
  const api = pad + '/messung'
  const $ = id => document.getElementById(id)
  // hochauflösend und monoton; der Versatz zur Serveruhr wird ohnehin laufend neu bestimmt
  const now = () => performance.timeOrigin + performance.now()
  const ms = x => x == null ? '–' : `${Math.round(x)} ms`

  async function post(path, body) {
    const r = await fetch(api + path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body || {}) })
    if (!r.ok) throw new Error((await r.text()) || `Fehler ${r.status}`)
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
      const j = await (await fetch(api + '/zeit', { cache: 'no-store' })).json()
      const t1 = now()
      s.push({ rtt: t1 - t0, off: j.t - (t0 + t1) / 2 })
    }
    s.sort((a, b) => a.rtt - b.rtt)
    const b = { rtt: s[0].rtt, off: s[0].off, rttMed: s[2].rtt }
    recent.push(b); if (recent.length > 3) recent.shift()
    await post('/probe', { dev, rtt: b.rtt, rttMed: b.rttMed, off: b.off })
    abgleiche++
    $('m-status').textContent = `Misst mit: ${abgleiche} Abgleiche, Laufzeit zuletzt ${ms(b.rtt)}. Bitte offen lassen.`
  }
  async function loop() {
    while (started) {
      try { await serie() } catch (e) { $('m-status').textContent = `Unterbrochen (${e.message}), versuche es gleich wieder …` }
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
    b.textContent = an ? 'Jetzt drücken!' : 'Warten auf das Signal'
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
    post('/empfang', { dev, runde, t: tr, off: b.off, rtt: b.rtt }).catch(() => {})
  }
  $('m-buzzer').addEventListener('click', () => {
    const tp = now()
    const b = best()
    zeigeBuzzer(false); clearTimeout(closeTimer)
    $('m-reaktion').textContent = `Gedrückt nach ${ms(tp - signalT)}.`
    post('/druck', { dev, runde, t: tp, off: b.off, rtt: b.rtt }).catch(e => { $('m-reaktion').textContent = `Nicht angekommen: ${e.message}` })
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
      ...wert('Geräte, Abgleiche, Runden', `${a.geraete} · ${a.abgleiche} · ${a.runden}`),
      ...wert('Laufzeit hin und zurück', `Median ${ms(a.laufzeit.median)}, 95 % unter ${ms(a.laufzeit.p95)}, über 300 ms: ${a.laufzeit.ueber300} %`),
      ...wert('Uhr nach Abgleich (Abweichung)', `Median ${ms(a.uhr.median)}, 95 % unter ${ms(a.uhr.p95)}`),
      ...wert('Signal erreicht die Handys', `nach ${ms(a.signal.median)} (95 %: ${ms(a.signal.p95)}); zwischen erstem und letztem Handy je Runde ${ms(a.signal.spanneMedian)}, höchstens ${ms(a.signal.spanneMax)}`),
      ...wert('Netz verschiebt den Druck', `um ${ms(a.eingang.median)} (95 %: ${ms(a.eingang.p95)}); Unterschied zwischen Handys je Runde ${ms(a.eingang.spanneMedian)}, höchstens ${ms(a.eingang.spanneMax)}`),
      ...wert('Reaktionszeit', `Median ${ms(a.reaktion.median)}`),
      ...wert('Wer war zuerst?', v.runden ? `${v.runden} Runden mit mindestens zwei Drücken: Erster nach Eingang beim Server falsch in ${v.wechselEingang}, Erster nach Reaktionszeit anders in ${v.wechselReaktion}, knapper als 50 ms in ${v.knapp}` : 'noch keine Runde mit zwei Drücken'),
    )
    $('m-liste').replaceChildren(...a.liste.map(g => {
      const tr = document.createElement('tr')
      for (const x of [`Gerät ${g.nr}`, g.art, g.abgleiche, ms(g.laufzeit), ms(g.uhr), g.druecke, g.zuletztS == null ? '–' : `vor ${g.zuletztS} s`]) {
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
    if (!confirm('Alle Messdaten dieses Quiz löschen? Vorher die CSV sichern, wenn sie gebraucht wird.')) return
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
