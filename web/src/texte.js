// Texte in der Sprache der Seite: der Server legt sie in den Konfigurationsblock (texte, lang). Platzhalter {name}
const cfg = (() => { try { return JSON.parse(document.getElementById('nextpad-config')?.textContent || '{}') } catch { return {} } })()
const T = cfg.texte || {}
export const tx = (k, w = {}) => (T[k] || k).replace(/\{(\w+)\}/g, (_, n) => String(w[n] ?? ''))
// Datum und Uhrzeit wie die Seite (englisch britisch)
export const locale = cfg.lang === 'en' ? 'en-GB' : 'de-DE'
