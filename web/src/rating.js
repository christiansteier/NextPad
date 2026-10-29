// Bewertung mit Daumen hoch und runter als Baustein im Text.
// Der Knoten trägt nur seine Kennung; die Stimmen liegen in der Y.Map "ratings" unter "<kennung>:<autor>" → 1 | -1.
// Flache Schlüssel, damit gleichzeitige Stimmen verschiedener Personen sich nicht überschreiben.
import { Node, mergeAttributes } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Fragment, Slice } from '@tiptap/pm/model'
import { tx } from './texte.js'

export const RATINGS_MAP = 'ratings'

// Daumen nach Lucide (ISC-Lizenz), als Umriss in Textfarbe
const UP = '<path d="M7 10v12"/><path d="M15 5.88 14 10h5.83a2 2 0 0 1 1.92 2.56l-2.33 8A2 2 0 0 1 17.5 22H4a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2h2.76a2 2 0 0 0 1.79-1.11L12 2a3.13 3.13 0 0 1 3 3.88Z"/>'
const DOWN = '<path d="M17 14V2"/><path d="M9 18.12 10 14H4.17a2 2 0 0 1-1.92-2.56l2.33-8A2 2 0 0 1 6.5 2H20a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-2.76a2 2 0 0 0-1.79 1.11L12 22a3.13 3.13 0 0 1-3-3.88Z"/>'
export const thumbSvg = (dir, size = 16) => `<svg class="thumb" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${dir === 'up' ? UP : DOWN}</svg>`

const newId = () => Array.from(crypto.getRandomValues(new Uint8Array(8)), b => b.toString(16).padStart(2, '0')).join('')

export function tally(map, id) {
  let up = 0, down = 0
  const prefix = id + ':'
  map.forEach((v, k) => { if (k.startsWith(prefix)) { if (v === 1) up++; else if (v === -1) down++ } })
  return { up, down }
}

// Eingefügte Bewertungen bekommen neue Kennungen, sonst teilten Original und Kopie die Stimmen.
function renewIds(fragment, type) {
  const out = []
  fragment.forEach(n => {
    if (n.type === type) out.push(type.create({ ...n.attrs, id: newId() }, null, n.marks))
    else out.push(n.copy(renewIds(n.content, type)))
  })
  return Fragment.from(out)
}

export function createRating({ ydoc, me, readonly }) {
  const votes = ydoc.getMap(RATINGS_MAP)
  const views = new Set()
  votes.observe(() => { for (const v of views) v.refresh() })
  const vote = (id, dir) => {
    if (readonly || !me || !id) return
    const key = `${id}:${me}`
    if (votes.get(key) === dir) votes.delete(key); else votes.set(key, dir)
  }

  return Node.create({
    name: 'rating',
    group: 'inline',
    inline: true,
    atom: true,
    selectable: true,
    draggable: false,
    addAttributes() {
      return { id: { default: null, parseHTML: el => el.getAttribute('data-rating'), renderHTML: a => (a.id ? { 'data-rating': a.id } : {}) } }
    },
    parseHTML() { return [{ tag: 'span[data-type="rating"]' }] },
    // Ohne Ansicht (Zeitleiste, Zwischenablage): Stand als Text
    renderHTML({ node, HTMLAttributes }) {
      const t = tally(votes, node.attrs.id)
      return ['span', mergeAttributes(HTMLAttributes, { 'data-type': 'rating', class: 'rating' }), `👍 ${t.up} · 👎 ${t.down}`]
    },
    renderText({ node }) { const t = tally(votes, node.attrs.id); return `👍 ${t.up} · 👎 ${t.down}` },
    renderMarkdown(node) { const t = tally(votes, node.attrs.id); return `👍 ${t.up} · 👎 ${t.down}` },
    addCommands() {
      return { insertRating: () => ({ commands }) => commands.insertContent([{ type: 'rating', attrs: { id: newId() } }, { type: 'text', text: ' ' }]) }
    },
    addNodeView() {
      return ({ node }) => {
        let id = node.attrs.id
        const dom = document.createElement('span'); dom.className = 'rating'; dom.contentEditable = 'false'; dom.setAttribute('role', 'group'); dom.setAttribute('aria-label', 'Bewertung')
        dom.dataset.type = 'rating'; if (id) dom.dataset.rating = id
        const mk = (dir, label) => {
          const b = document.createElement('button'); b.type = 'button'; b.className = 'rate ' + dir; b.disabled = readonly
          b.innerHTML = `${thumbSvg(dir)}<span class="n">0</span>`
          b.addEventListener('mousedown', e => e.preventDefault()) // Cursor bleibt, wo er war
          b.addEventListener('click', e => { e.preventDefault(); e.stopPropagation(); vote(id, dir === 'up' ? 1 : -1) })
          b._label = label; dom.appendChild(b); return b
        }
        const up = mk('up', tx('daumen_hoch')), down = mk('down', tx('daumen_runter'))
        const view = {
          dom,
          refresh() {
            const t = tally(votes, id), mine = me ? votes.get(`${id}:${me}`) : undefined
            for (const [b, n, on] of [[up, t.up, mine === 1], [down, t.down, mine === -1]]) {
              b.querySelector('.n').textContent = String(n)
              b.setAttribute('aria-pressed', String(on)); b.classList.toggle('on', on)
              b.title = `${b._label}: ${n}${on ? tx('meine_stimme') : ''}`
              b.setAttribute('aria-label', b.title)
            }
          },
          update(n) { if (n.type.name !== 'rating') return false; id = n.attrs.id; dom.dataset.rating = id || ''; view.refresh(); return true },
          stopEvent: e => e.target instanceof Element && !!e.target.closest('button'),
          ignoreMutation: () => true,
          destroy() { views.delete(view) },
        }
        views.add(view); view.refresh()
        return view
      }
    },
    addProseMirrorPlugins() {
      const type = this.type
      return [new Plugin({
        key: new PluginKey('nextpad-rating-paste'),
        props: { transformPasted: slice => new Slice(renewIds(slice.content, type), slice.openStart, slice.openEnd) },
      })]
    },
  })
}
