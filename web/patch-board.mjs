// Nach dem Bau: Excalidraw hängt jeder Schrift eine Zweitquelle bei esm.sh an. ORT liefert alles selbst aus,
// deshalb zeigt die Zweitquelle auf denselben eigenen Pfad (keine Anfragen nach außen, keine CSP-Meldungen).
import { readdirSync, readFileSync, writeFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
const re = /"ASSETS_FALLBACK_URL",`https:\/\/esm\.sh\/.*?\/dist\/prod\/`/g
let n = 0
function walk(d) {
  for (const f of readdirSync(d)) {
    const p = join(d, f)
    if (statSync(p).isDirectory()) walk(p)
    else if (p.endsWith('.js')) {
      const s = readFileSync(p, 'utf8')
      if (re.test(s)) { re.lastIndex = 0; writeFileSync(p, s.replace(re, '"ASSETS_FALLBACK_URL",`${window.EXCALIDRAW_ASSET_PATH||"/"}`')); n++ }
      re.lastIndex = 0
    }
  }
}
walk('dist/board')
if (n === 0) { console.error('patch-board: Zweitquelle nicht gefunden (Excalidraw-Version geändert?)'); process.exit(1) }
console.log(`patch-board: ${n} Datei(en) angepasst`)
