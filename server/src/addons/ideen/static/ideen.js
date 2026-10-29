// Ideensammlung: Beiträge eintragen, abstimmen, moderieren. Der Server prüft alles, hier wird nur angezeigt.
(() => {
  const cfg = JSON.parse(document.getElementById('nextpad-config').textContent)
  const T = cfg.texte || {}
  const tx = (k, w = {}) => (T[k] || k).replace(/\{(\w+)\}/g, (_, n) => String(w[n] ?? ''))
  // aus Moodle gestartet: zurück in den Kurs, auch aus der eingebetteten Ansicht heraus
  if (cfg.course) { const a = document.createElement('a'); a.href = cfg.course; a.target = '_top'; a.textContent = tx('zum_kurs'); document.querySelector('.site-header nav')?.prepend(a) }
  const base = cfg.basePath || ''
  const pad = encodeURIComponent(cfg.urlId)
  const api = `${base}/p/${pad}/ideen`
  const $ = id => document.getElementById(id)
  let data = null
  let sort = 'top'
  try { if (localStorage.getItem('nextpad.ideen.sort') === 'new') sort = 'new' } catch {}

  async function post(url, body) {
    const r = await fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })
    if (!r.ok) throw new Error((await r.text()) || tx('fehler', { status: r.status }))
    return r.json()
  }

  function icon(d, size = 18) {
    const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('width', size); s.setAttribute('height', size); s.setAttribute('aria-hidden', 'true'); s.setAttribute('focusable', 'false')
    const p = document.createElementNS('http://www.w3.org/2000/svg', 'path'); p.setAttribute('d', d); s.append(p)
    return s
  }
  const stimmen = n => tx(n === 1 ? 'stimme' : 'stimmen', { n })
  const ICON = {
    check: 'M5 12.5l4.5 4.5L19 7.5',
    undo: 'M9 14L4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3',
    hide: 'M3 3l18 18M10.6 10.6a2 2 0 0 0 2.8 2.8M9.9 5.1A10 10 0 0 1 12 5c5 0 9 5 10 7a17 17 0 0 1-3.2 4M6.6 6.6C4.2 8 2.7 10.4 2 12c1 2 5 7 10 7 1.7 0 3.3-.5 4.6-1.3',
    show: 'M2 12c1-2 5-7 10-7s9 5 10 7c-1 2-5 7-10 7S3 14 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
  }
  // Beiträge, die schon angezeigt wurden: neue bekommen kurz eine Markierung
  let known = null

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
      b.setAttribute('aria-label', `${on ? tx('zuruecknehmen') : tx('zustimmen')}, ${stimmen(i.votes)}`)
      const n = document.createElement('span'); n.textContent = i.votes
      b.append(icon('M12 5l7 9H5z'), n)
      li.append(b)
    } else {
      const n = document.createElement('span'); n.className = 'votes'; n.textContent = stimmen(i.votes)
      meta.append(n)
    }
    if (i.done) { const d = document.createElement('span'); d.className = 'done-mark'; d.append(icon(ICON.check, 16), tx('erledigt')); meta.append(d) }
    if (mine.has(i.id)) { const m = document.createElement('span'); m.className = 'tag'; m.textContent = tx('dein'); meta.append(m) }
    const body = document.createElement('div'); body.className = 'idea-body'
    body.append(text, meta)
    li.append(body)
    if (data.owner) {
      const tools = document.createElement('div'); tools.className = 'mod-tools'
      const mk = (key, label, d, body) => { const b = button('icon-btn', '', `${i.id}:${key}`, () => post(`${api}/${i.id}/moderation`, body)); b.append(icon(d, 20)); b.setAttribute('aria-label', label); b.title = label; return b }
      if (!i.hidden) tools.append(i.done ? mk('done', tx('wieder_oeffnen'), ICON.undo, { done: false }) : mk('done', tx('als_erledigt'), ICON.check, { done: true }))
      tools.append(i.hidden ? mk('hide', tx('einblenden'), ICON.show, { hidden: false }) : mk('hide', tx('ausblenden'), ICON.hide, { hidden: true }))
      li.append(tools)
    }
    if (known && !known.has(i.id)) li.classList.add('is-new')
    return li
  }

  function render() {
    const focusKey = document.activeElement?.dataset?.key
    const voted = new Set(data.voted), mine = new Set(data.mine)
    $('prompt').textContent = data.prompt || tx('titel')
    document.title = `${data.prompt || tx('name')} · NextPad`
    $('prompt-edit').hidden = !data.owner
    if (!data.owner) { $('prompt-form').hidden = true; $('prompt-edit').setAttribute('aria-expanded', 'false') }
    if (data.owner && document.activeElement !== $('prompt-input')) $('prompt-input').value = data.prompt
    $('add-form').hidden = !data.canWrite
    $('readonly-hint').hidden = data.canWrite
    $('owner-tools').hidden = !data.canDelete

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
    $('empty').textContent = items.length > hidden.length ? tx('keine_offenen') : cfg.free ? tx('leer_teilen') : tx('leer')
    const c = tx(open.length === 1 ? 'offen1' : 'offen', { n: open.length })
    if ($('ideas-count').textContent !== c) $('ideas-count').textContent = c
    for (const b of document.querySelectorAll('.sort .tab')) b.setAttribute('aria-pressed', b.dataset.sort === sort ? 'true' : 'false')
    known = new Set(data.items.map(i => i.id))
    if (focusKey) document.querySelector(`[data-key="${CSS.escape(focusKey)}"]`)?.focus()
  }

  let loading = null, again = false
  async function load() {
    if (loading) { again = true; return loading }
    loading = (async () => {
      try {
        const r = await fetch(api, { cache: 'no-store' })
        if (r.status === 404) { $('ideas-count').textContent = tx('weg'); return }
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
    if (!v) { err.textContent = tx('eingeben'); err.hidden = false; text.setAttribute('aria-invalid', 'true'); text.focus(); return }
    const b = e.submitter; if (b) b.disabled = true
    // nur leeren, wenn inzwischen nichts Neues eingetippt wurde
    try { await post(api, { text: v }); if (text.value.trim() === v) { text.value = ''; $('add-count').textContent = '0' } err.hidden = true }
    catch (x) { err.textContent = x.message; err.hidden = false }
    if (b) b.disabled = false
    text.focus()
    await load()
  })
  text.addEventListener('keydown', e => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) $('add-form').requestSubmit() })

  // Frage ändern: Stift neben der Überschrift klappt das Feld auf, Speichern oder Escape klappt es zu
  function editPrompt(open) {
    $('prompt-form').hidden = !open
    $('prompt-edit').setAttribute('aria-expanded', open ? 'true' : 'false')
    if (open) { $('prompt-input').value = data?.prompt || ''; $('prompt-input').focus() } else $('prompt-edit').focus()
  }
  $('prompt-edit').addEventListener('click', () => editPrompt($('prompt-form').hidden))
  $('prompt-input').addEventListener('keydown', e => { if (e.key === 'Escape') editPrompt(false) })
  $('prompt-form').addEventListener('submit', async e => {
    e.preventDefault()
    try { await post(`${api}/frage`, { prompt: $('prompt-input').value }); editPrompt(false) } catch (x) { alert(x.message) }
    await load()
  })

  for (const b of document.querySelectorAll('.sort .tab')) b.addEventListener('click', () => {
    sort = b.dataset.sort
    try { localStorage.setItem('nextpad.ideen.sort', sort) } catch {}
    if (data) render()
  })

  // Teilen: Link zum Mitmachen in die Zwischenablage
  const link = `${location.origin}${base}/p/${encodeURIComponent(cfg.urlId)}`
  if (cfg.readonly) { $('view-kicker').textContent = tx('leseansicht'); $('share-copy').closest('.share-row').hidden = true }
  // Kursaktivität: Zugang nur über Moodle, Links weiterzugeben hilft nicht
  if (!cfg.free) {
    for (const r of document.querySelectorAll('.share-row')) r.hidden = true
    $('share-h').textContent = tx('zeigen_sichern')
    const p = document.createElement('p'); p.className = 'hint'; p.textContent = tx('nur_moodle')
    $('share-h').after(p)
  }
  $('share-copy').addEventListener('click', async () => {
    const b = $('share-copy')
    let ok = true
    try { await navigator.clipboard.writeText(link) } catch { ok = false }
    b.textContent = ok ? tx('kopiert') : link
    setTimeout(() => { b.textContent = tx('link_kopieren') }, ok ? 2000 : 8000)
  })
  $('delete-all').addEventListener('click', async () => {
    if (!confirm(tx('loeschen_frage'))) return
    const r = await fetch(`${base}/p/${pad}/delete`, { method: 'POST' })
    if (r.ok) {
      try { const l = JSON.parse(localStorage.getItem('nextpad.recent') || '[]').filter(x => x && x.id !== cfg.urlId); localStorage.setItem('nextpad.recent', JSON.stringify(l)) } catch {}
      location.href = `${base}/`
    }
  })

  // Zuletzt geöffnet auf der Startseite
  function remember() {
    if (cfg.readonly || !cfg.free) return
    try {
      let l = JSON.parse(localStorage.getItem('nextpad.recent') || '[]'); if (!Array.isArray(l)) l = []
      l = l.filter(e => e && e.id !== cfg.urlId)
      l.unshift({ id: cfg.urlId, t: Date.now(), title: data?.prompt || tx('name') })
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
