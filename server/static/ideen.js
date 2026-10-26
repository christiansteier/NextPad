// Ideensammlung: Beiträge eintragen, abstimmen, moderieren. Der Server prüft alles, hier wird nur angezeigt.
(() => {
  const cfg = JSON.parse(document.getElementById('nextpad-config').textContent)
  const base = cfg.basePath || ''
  const pad = encodeURIComponent(cfg.urlId)
  const api = `${base}/p/${pad}/ideen`
  const $ = id => document.getElementById(id)
  let data = null
  let sort = 'top'
  try { if (localStorage.getItem('nextpad.ideen.sort') === 'new') sort = 'new' } catch {}

  async function post(url, body) {
    const r = await fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })
    if (!r.ok) throw new Error((await r.text()) || `Fehler ${r.status}`)
    return r.json()
  }

  function icon(d) {
    const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('width', '18'); s.setAttribute('height', '18'); s.setAttribute('aria-hidden', 'true'); s.setAttribute('focusable', 'false')
    const p = document.createElementNS('http://www.w3.org/2000/svg', 'path'); p.setAttribute('d', d); s.append(p)
    return s
  }
  const stimmen = n => `${n} ${n === 1 ? 'Stimme' : 'Stimmen'}`

  function button(cls, text, key, onClick) {
    const b = document.createElement('button')
    b.type = 'button'; b.className = cls; b.dataset.key = key
    if (text) b.textContent = text
    b.addEventListener('click', async () => {
      b.disabled = true
      try { await onClick() } catch (e) { alert(e.message) }
      await load()
    })
    return b
  }

  function item(i, voted, mine) {
    const li = document.createElement('li')
    li.className = 'idea' + (i.done ? ' is-done' : '') + (i.hidden ? ' is-hidden' : '')
    const text = document.createElement('p'); text.className = 'idea-text'; text.textContent = i.text
    const meta = document.createElement('div'); meta.className = 'idea-meta'
    if (!i.done && !i.hidden && data.canWrite) {
      const on = voted.has(i.id)
      const b = button('vote', '', `${i.id}:vote`, () => post(`${api}/${i.id}/stimme`, { on: !on }))
      b.setAttribute('aria-pressed', on ? 'true' : 'false')
      b.setAttribute('aria-label', `${on ? 'Stimme zurücknehmen' : 'Zustimmen'}, ${stimmen(i.votes)}`)
      const n = document.createElement('span'); n.textContent = i.votes
      b.append(icon('M12 5l7 9H5z'), n)
      li.append(b)
    } else {
      const n = document.createElement('span'); n.className = 'votes'; n.textContent = stimmen(i.votes)
      meta.append(n)
    }
    if (mine.has(i.id)) { const m = document.createElement('span'); m.className = 'tag'; m.textContent = 'Dein Beitrag'; meta.append(m) }
    if (data.owner) {
      meta.append(button('btn-link', i.done ? 'Wieder öffnen' : 'Erledigt', `${i.id}:done`, () => post(`${api}/${i.id}/moderation`, { done: !i.done })))
      meta.append(button('btn-link', i.hidden ? 'Einblenden' : 'Ausblenden', `${i.id}:hide`, () => post(`${api}/${i.id}/moderation`, { hidden: !i.hidden })))
    }
    const body = document.createElement('div'); body.className = 'idea-body'
    body.append(text, meta)
    li.append(body)
    return li
  }

  function render() {
    const focusKey = document.activeElement?.dataset?.key
    const voted = new Set(data.voted), mine = new Set(data.mine)
    $('prompt').textContent = data.prompt || 'Ideen und Fragen'
    document.title = `${data.prompt || 'Ideensammlung'} · NextPad`
    const pf = $('prompt-form')
    pf.hidden = !data.owner
    if (data.owner && document.activeElement !== $('prompt-input')) $('prompt-input').value = data.prompt
    $('add-form').hidden = !data.canWrite
    $('readonly-hint').hidden = data.canWrite
    $('owner-tools').hidden = !data.owner

    const items = [...data.items]
    if (sort === 'top') items.sort((a, b) => b.votes - a.votes || a.id - b.id)
    else items.sort((a, b) => b.id - a.id)
    const open = items.filter(i => !i.done && !i.hidden), done = items.filter(i => i.done && !i.hidden), hidden = items.filter(i => i.hidden)
    $('open-list').replaceChildren(...open.map(i => item(i, voted, mine)))
    $('done-list').replaceChildren(...done.map(i => item(i, voted, mine)))
    $('hidden-list').replaceChildren(...hidden.map(i => item(i, voted, mine)))
    $('done-n').textContent = done.length; $('done-box').hidden = !done.length
    $('hidden-n').textContent = hidden.length; $('hidden-box').hidden = !hidden.length
    $('empty').hidden = open.length > 0
    const c = `${open.length} ${open.length === 1 ? 'offener Beitrag' : 'offene Beiträge'}`
    if ($('ideas-count').textContent !== c) $('ideas-count').textContent = c
    for (const b of document.querySelectorAll('.sort .chip')) b.setAttribute('aria-pressed', b.dataset.sort === sort ? 'true' : 'false')
    if (focusKey) document.querySelector(`[data-key="${CSS.escape(focusKey)}"]`)?.focus()
  }

  let loading = null, again = false
  async function load() {
    if (loading) { again = true; return loading }
    loading = (async () => {
      try {
        const r = await fetch(api, { cache: 'no-store' })
        if (r.status === 404) { $('ideas-count').textContent = 'Diese Sammlung gibt es nicht mehr.'; return }
        if (r.ok) { data = await r.json(); render() }
      } catch {}
    })()
    await loading; loading = null
    if (again) { again = false; await load() }
  }

  // Eintragen
  const text = $('add-text'), err = $('add-error')
  text.addEventListener('input', () => { $('add-count').textContent = [...text.value].length; if (!err.hidden && text.value.trim()) { err.hidden = true; text.removeAttribute('aria-invalid') } })
  $('add-form').addEventListener('submit', async e => {
    e.preventDefault()
    const v = text.value.trim()
    if (!v) { err.textContent = 'Bitte eine Idee oder Frage eingeben.'; err.hidden = false; text.setAttribute('aria-invalid', 'true'); text.focus(); return }
    const b = e.submitter; if (b) b.disabled = true
    // nur leeren, wenn inzwischen nichts Neues eingetippt wurde
    try { await post(api, { text: v }); if (text.value.trim() === v) { text.value = ''; $('add-count').textContent = '0' } err.hidden = true }
    catch (x) { err.textContent = x.message; err.hidden = false }
    if (b) b.disabled = false
    text.focus()
    await load()
  })
  text.addEventListener('keydown', e => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) $('add-form').requestSubmit() })

  $('prompt-form').addEventListener('submit', async e => {
    e.preventDefault()
    try { await post(`${api}/frage`, { prompt: $('prompt-input').value }) } catch (x) { alert(x.message) }
    await load()
  })

  for (const b of document.querySelectorAll('.sort .chip')) b.addEventListener('click', () => {
    sort = b.dataset.sort
    try { localStorage.setItem('nextpad.ideen.sort', sort) } catch {}
    if (data) render()
  })

  // Teilen
  const link = id => `${location.origin}${base}/p/${encodeURIComponent(id)}`
  $('share-link').value = link(cfg.urlId)
  if (cfg.readonlyId) $('share-ro').value = link(cfg.readonlyId)
  else $('share-ro').closest('.share-row').hidden = true
  if (cfg.readonly) $('share-link').closest('.share-row').hidden = true
  for (const b of document.querySelectorAll('[data-copy]')) b.addEventListener('click', async () => {
    const f = $(b.dataset.copy)
    try { await navigator.clipboard.writeText(f.value) } catch { f.select(); document.execCommand('copy') }
    b.textContent = 'Kopiert'; setTimeout(() => { b.textContent = 'Kopieren' }, 2000)
  })
  $('delete-all').addEventListener('click', async () => {
    if (!confirm('Die Sammlung samt allen Beiträgen und Stimmen löschen? Das lässt sich nicht rückgängig machen.')) return
    const r = await fetch(`${base}/p/${pad}/delete`, { method: 'POST' })
    if (r.ok) {
      try { const l = JSON.parse(localStorage.getItem('nextpad.recent') || '[]').filter(x => x && x.id !== cfg.urlId); localStorage.setItem('nextpad.recent', JSON.stringify(l)) } catch {}
      location.href = `${base}/`
    }
  })

  // Zuletzt geöffnet auf der Startseite
  function remember() {
    if (cfg.readonly) return
    try {
      let l = JSON.parse(localStorage.getItem('nextpad.recent') || '[]'); if (!Array.isArray(l)) l = []
      l = l.filter(e => e && e.id !== cfg.urlId)
      l.unshift({ id: cfg.urlId, t: Date.now(), title: data?.prompt || 'Ideensammlung' })
      localStorage.setItem('nextpad.recent', JSON.stringify(l.slice(0, 10)))
    } catch {}
  }

  // Live: der Server meldet nur „geändert“, den Stand holt jede Seite mit ihren Rechten selbst
  let timer = null
  const es = new EventSource(`${api}/live`)
  es.addEventListener('change', () => { clearTimeout(timer); timer = setTimeout(load, 120) })
  es.addEventListener('open', () => load())
  load().then(remember)
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') remember() })
})()
