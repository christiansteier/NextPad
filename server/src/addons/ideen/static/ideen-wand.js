// Leinwand: die beliebtesten offenen Beiträge groß, live wie die Sammlung selbst. Keine Bedienung.
(() => {
  const cfg = JSON.parse(document.getElementById('nextpad-config').textContent)
  const T = cfg.texte || {}
  const tx = (k, w = {}) => (T[k] || k).replace(/\{(\w+)\}/g, (_, n) => String(w[n] ?? ''))
  const api = `${cfg.basePath || ''}/p/${encodeURIComponent(cfg.urlId)}/ideen`
  const $ = id => document.getElementById(id)
  const TOP = 6
  let known = null

  function render(data) {
    $('prompt').textContent = data.prompt || tx('titel')
    document.title = tx('wand_titel', { titel: data.prompt || tx('name') })
    const open = data.items.filter(i => !i.done && !i.hidden).sort((a, b) => b.votes - a.votes || a.id - b.id)
    const top = open.slice(0, TOP)
    $('wall-list').replaceChildren(...top.map(i => {
      const li = document.createElement('li')
      li.className = 'wall-item' + (known && !known.has(i.id) ? ' is-new' : '')
      const v = document.createElement('span'); v.className = 'wall-votes'; v.textContent = i.votes
      v.setAttribute('aria-label', tx(i.votes === 1 ? 'stimme' : 'stimmen', { n: i.votes }))
      const t = document.createElement('p'); t.className = 'wall-text'; t.textContent = i.text
      li.append(v, t)
      return li
    }))
    known = new Set(data.items.map(i => i.id))
    $('wall-empty').hidden = open.length > 0
    const rest = open.length - top.length
    $('wall-count').textContent = tx(open.length === 1 ? 'offen1' : 'offen', { n: open.length }) + (rest > 0 ? tx('beliebteste', { n: top.length }) : '')
  }

  let busy = false, again = false
  async function load() {
    if (busy) { again = true; return }
    busy = true
    try {
      const r = await fetch(api, { cache: 'no-store' })
      if (r.ok) render(await r.json())
      else if (r.status === 404) $('wall-count').textContent = tx('weg')
    } catch {}
    busy = false
    if (again) { again = false; load() }
  }

  let timer = null
  const es = new EventSource(`${api}/live`)
  es.addEventListener('change', () => { clearTimeout(timer); timer = setTimeout(load, 120) })
  es.addEventListener('open', () => load())
  load()
})()
