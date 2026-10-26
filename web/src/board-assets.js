// Klassisches Skript, läuft vor dem Modul board.js (Excalidraw liest den Pfad schon beim Laden): Schriften und Sprachdateien kommen von NextPad selbst, nicht von esm.sh.
const cfg = JSON.parse(document.getElementById('nextpad-config').textContent)
window.EXCALIDRAW_ASSET_PATH = `${location.origin}${cfg.basePath}/static/board/`
