# NextPad-Editor (Browser)

TipTap 3 auf ProseMirror, Yjs über y-websocket gegen den Rust-Server. Gebaut mit esbuild zu einem einzigen Bundle (`dist/editor.js`, rund 540 kB), das in den Server eingebettet wird.

## Bauen

    bash build.sh

Das Skript kopiert die Quellen nach `~/.cache/nextpad-web` (die Projektablage ist `noexec`), installiert mit pnpm nach Lockfile, baut und legt `editor.js` und `editor.css` unter `../server/static/` ab. pnpm 11: `pnpm-workspace.yaml` erlaubt das Install-Skript von esbuild (`allowBuilds`).

## Was drin ist

StarterKit (ohne Undo-Redo, Link, Codeblock, Zitat, Linie), TextStyle mit Farbe und Schriftgröße, Textausrichtung, Collaboration (Feld `default`), CollaborationCaret, eigene Mark `author` (Autorenkennung je Zeichen; immer als gespeicherte Mark des eigenen Autors aktiv), Toolbar, Statuszeile mit Verbindung und Teilnehmenden, Zeitleiste (Snapshot-Liste des Servers), Export-Links. Konfiguration kommt aus `<script type="application/json" id="nextpad-config">` der Seite.

## Tests

    node test/synctest.mjs <ids-datei>   # zwei Browser, ein Pad (Playwright, Chromium)
    node test/diag.mjs <ids-datei>       # Diagnose: Marks in Y und PM, Tippen, Überschrift

`<ids-datei>` enthält `groupID|padID|authorID|sessionID|readOnlyID` aus einem API-Lauf (Aufrufe in `../docs/api-spezifikation.md`). Aus dem Bau-Verzeichnis starten, dort liegt Playwright.
