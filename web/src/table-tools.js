// Tabellen: Einfügen über die Werkzeugleiste, Zeilen und Spalten über eine Auswahl, die nur in einer Tabelle erscheint.
// Markdown-Tabellen (| a | b |) kommen über das Einfügen aus der Zwischenablage oder den Markdown-Dialog.
import { Table, TableRow, TableHeader, TableCell } from '@tiptap/extension-table'

export const tableExtensions = [Table.configure({ resizable: false }), TableRow, TableHeader, TableCell]

const ACTIONS = [
  ['rowBefore', 'Zeile darüber', c => c.addRowBefore()],
  ['rowAfter', 'Zeile darunter', c => c.addRowAfter()],
  ['colBefore', 'Spalte links', c => c.addColumnBefore()],
  ['colAfter', 'Spalte rechts', c => c.addColumnAfter()],
  ['header', 'Kopfzeile ein/aus', c => c.toggleHeaderRow()],
  ['delRow', 'Zeile löschen', c => c.deleteRow()],
  ['delCol', 'Spalte löschen', c => c.deleteColumn()],
  ['delTable', 'Tabelle löschen', c => c.deleteTable()],
]

// ibtn und select kommen aus der Werkzeugleiste des Editors, damit Aussehen und Verhalten gleich bleiben.
export function addTableTools(editor, parent, { ibtn, select, icon }) {
  ibtn(parent, icon, 'Tabelle einfügen (3 × 3 mit Kopfzeile; Markdown: | A | B |)', () => editor.chain().focus().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(), () => editor.isActive('table'))
  const sel = select(parent, 'Tabelle bearbeiten', 'Tabelle', ACTIONS.map(([v, t]) => [v, t]), v => {
    const a = ACTIONS.find(x => x[0] === v); if (a) a[2](editor.chain().focus()).run()
  })
  // bleibt stehen, damit die Leiste nicht springt; außerhalb einer Tabelle nur ausgegraut
  const sync = () => { sel.disabled = !editor.isActive('table') }
  editor.on('selectionUpdate', sync); editor.on('transaction', sync); sync()
}
