// Inhaltsverzeichnis: als Seitenleiste neben dem Text und als Baustein im Text.
// Beide lesen die Überschriften live aus dem Dokument; der Baustein speichert selbst keinen Inhalt.
import { Node, mergeAttributes } from '@tiptap/core'
import { tx } from './texte.js'

export function headings(doc) {
  const out = []
  doc.descendants((n, pos) => {
    if (n.type.name === 'heading') { const text = n.textContent.trim(); if (text) out.push({ level: n.attrs.level, text, pos }); return false }
    return n.type.name !== 'table'
  })
  return out
}

function jumpTo(editor, pos) {
  const dom = editor.view.nodeDOM(pos)
  if (dom instanceof Element) dom.scrollIntoView({ behavior: 'smooth', block: 'start' })
  if (editor.isEditable) editor.chain().setTextSelection(pos + 1).focus(undefined, { scrollIntoView: false }).run()
}

function fillList(list, editor, items) {
  list.replaceChildren()
  if (!items.length) { const li = document.createElement('li'); li.className = 'empty'; li.textContent = tx('keine_ueberschriften'); list.appendChild(li); return }
  const min = Math.min(...items.map(i => i.level))
  for (const it of items) {
    const li = document.createElement('li'); li.className = 'lvl' + (it.level - min + 1)
    const a = document.createElement('a'); a.href = '#'; a.textContent = it.text
    a.addEventListener('mousedown', e => e.preventDefault())
    a.addEventListener('click', e => { e.preventDefault(); jumpTo(editor, it.pos) })
    li.appendChild(a); list.appendChild(li)
  }
}

export const TableOfContents = Node.create({
  name: 'tableOfContents',
  group: 'block',
  atom: true,
  selectable: true,
  draggable: false,
  parseHTML() { return [{ tag: 'nav[data-type="toc"]' }] },
  renderHTML({ HTMLAttributes }) { return ['nav', mergeAttributes(HTMLAttributes, { 'data-type': 'toc', class: 'toc' }), tx('verzeichnis')] },
  renderMarkdown() { return '' },
  addCommands() {
    return { insertTableOfContents: () => ({ commands }) => commands.insertContent({ type: 'tableOfContents' }) }
  },
  addNodeView() {
    return ({ editor }) => {
      const dom = document.createElement('nav'); dom.className = 'toc'; dom.contentEditable = 'false'; dom.setAttribute('aria-label', tx('verzeichnis'))
      const h = document.createElement('div'); h.className = 'toc-title'; h.textContent = tx('verzeichnis')
      const list = document.createElement('ol'); dom.append(h, list)
      let last = ''
      const refresh = () => {
        const items = headings(editor.state.doc), key = JSON.stringify(items)
        if (key !== last) { last = key; fillList(list, editor, items) }
      }
      editor.on('update', refresh); refresh()
      return { dom, ignoreMutation: () => true, stopEvent: e => e.target instanceof Element && !!e.target.closest('a'), destroy() { editor.off('update', refresh) } }
    }
  },
})

// Seitenleiste im grauen Rand links; Zustand (offen/zu) merkt sich der Browser.
export function createTocSidebar(editor, host, { readonly, onChange }) {
  const KEY = 'nextpad.toc'
  const box = document.createElement('aside'); box.id = 'tocbar'; box.setAttribute('aria-label', tx('verzeichnis'))
  const head = document.createElement('div'); head.className = 'toc-head'
  const title = document.createElement('h2'); title.textContent = tx('inhalt')
  const close = document.createElement('button'); close.type = 'button'; close.className = 'toc-close'; close.title = tx('verzeichnis_schliessen'); close.setAttribute('aria-label', close.title); close.textContent = '×'
  head.append(title, close)
  const list = document.createElement('ol'); list.className = 'toc-list'
  box.append(head, list)
  if (!readonly) {
    const ins = document.createElement('button'); ins.type = 'button'; ins.className = 'toc-insert'; ins.textContent = tx('ins_pad')
    ins.title = tx('ins_pad_titel')
    ins.addEventListener('click', () => editor.chain().focus().insertTableOfContents().run())
    box.appendChild(ins)
  }
  host.prepend(box) // vor dem Editor, damit sie in den Rand floatet
  let last = ''
  const refresh = () => {
    if (box.hidden) return
    const items = headings(editor.state.doc), key = JSON.stringify(items)
    if (key !== last) { last = key; fillList(list, editor, items) }
  }
  const stored = () => { try { return localStorage.getItem(KEY) } catch { return null } }
  const sidebar = {
    get open() { return !box.hidden },
    set(open) {
      box.hidden = !open; document.body.classList.toggle('toc-open', open)
      try { localStorage.setItem(KEY, open ? '1' : '0') } catch {}
      if (open) { last = ''; refresh() }
      onChange?.(open)
    },
    toggle() { sidebar.set(box.hidden) },
  }
  close.addEventListener('click', () => { sidebar.set(false); editor.commands.focus() })
  editor.on('update', refresh)
  sidebar.set(stored() === '1')
  return sidebar
}
