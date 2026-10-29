// Tabellen: Einfügen über die Werkzeugleiste, Zeilen und Spalten über eine Auswahl, die nur in einer Tabelle erscheint.
// Markdown-Tabellen (| a | b |) kommen über das Einfügen aus der Zwischenablage oder den Markdown-Dialog.
import { Table, TableRow, TableHeader, TableCell } from '@tiptap/extension-table'
import { tx } from './texte.js'

export const tableExtensions = [Table.configure({ resizable: false }), TableRow, TableHeader, TableCell]

const ACTIONS = [
  ['rowBefore', tx('zeile_darueber'), c => c.addRowBefore()],
  ['rowAfter', tx('zeile_darunter'), c => c.addRowAfter()],
  ['colBefore', tx('spalte_links'), c => c.addColumnBefore()],
  ['colAfter', tx('spalte_rechts'), c => c.addColumnAfter()],
  ['header', tx('kopfzeile'), c => c.toggleHeaderRow()],
  ['delRow', tx('zeile_loeschen'), c => c.deleteRow()],
  ['delCol', tx('spalte_loeschen'), c => c.deleteColumn()],
  ['delTable', tx('tabelle_loeschen'), c => c.deleteTable()],
]

// ibtn und select kommen aus der Werkzeugleiste des Editors, damit Aussehen und Verhalten gleich bleiben.
export function addTableTools(editor, parent, { ibtn, select, icon }) {
  ibtn(parent, icon, tx('tabelle_einfuegen'), () => editor.chain().focus().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(), () => editor.isActive('table'))
  const sel = select(parent, tx('tabelle_bearbeiten'), tx('tabelle'), ACTIONS.map(([v, t]) => [v, t]), v => {
    const a = ACTIONS.find(x => x[0] === v); if (a) a[2](editor.chain().focus()).run()
  })
  // bleibt stehen, damit die Leiste nicht springt; außerhalb einer Tabelle nur ausgegraut
  const sync = () => { sel.disabled = !editor.isActive('table') }
  editor.on('selectionUpdate', sync); editor.on('transaction', sync); sync()
}
