# Mitwirken

Beiträge sind willkommen: Fehlerberichte, Verbesserungsvorschläge und Pull Requests.
Dieses Dokument beschreibt den Entwicklungsablauf.

## Entwicklungsumgebung

```bash
git clone <repoadresse>
cd nextpad
cd server
CARGO_TARGET_DIR=~/.cache/cargo-target/nextpad cargo build
```

Der Editor in `web/` wird mit pnpm gebaut (siehe `web/README.md`); das Ergebnis
(`dist/editor.js`, `dist/editor.css`) wird in den Server eingebettet. Das Arbeitsverzeichnis
liegt auf einem `noexec`-Mount, daher kopiert das Bauskript die Quellen nach
`~/.cache/nextpad-web`.

## Tests und Prüfung

```bash
bash web/build.sh          # Editor- und Zeichenbrett-Bündel, vor dem ersten Rust-Bau nötig
cd server
cargo test                 # Unit-Tests (Aufbewahrung, Migration, IDs)
cargo build                # Programm für die Browsertests
cd ..
bash web/test/browsertests.sh     # Browser- und API-Tests gegen frisch gestartete Server
cd server && cargo build --release  # Freigabe-Binary mit LTO, stripped
```

`web/test/browsertests.sh` startet drei Server mit leerem Datenbestand (Hauptserver,
LTI mit nachgespielter Plattform, Addons ein und aus) und führt die Tests in `web/test/`
mit Playwright (Chromium) aus: API, Sync, Zeitleiste, Wiederherstellen, Editor, Gäste,
Startseite, Ideensammlung, Quiz, Buzzer, Zeichenbrett, LTI, Addons. Erstes Argument:
Pfad zum Programm (Vorgabe `server/target/debug/nextpad`), weitere Argumente wählen
einzelne Tests. Einmalig vorher: `pnpm exec playwright install chromium` im
Bau-Verzeichnis von `web/build.sh`. Jeder Test endet mit Code 1, wenn eine Prüfung
fehlschlägt. Last-, Barrierefreiheits- und Pilottests (`belastung*`, `a11y-*`, `ltipilot`,
`remotetest`) laufen weiter von Hand.

Die CI (`.github/workflows/ci.yml`) baut den Server, führt `cargo test`, clippy mit
`-D warnings`, `cargo audit` und dasselbe Testskript aus. Schlägt ein Browsertest fehl,
hängen Serverprotokolle und Testausgaben als Artefakt `browsertests` am Lauf.

## Konventionen

- Commits: deutsch, Präsens, Verb am Anfang, kein Punkt, kein Trailer
  („ergänzt Cargo.toml um die Lizenz“). Im Text darunter knappe Stichpunkte: was und warum.
- Keine Geheimnisse, keine Zugangsdaten, keine Pad-Inhalte im Repo. Was ignoriert wird,
  steht in `.gitignore`.
- `README.md` und `README.de.md` werden inhaltlich synchron gehalten.
- Neue Funktionalität kommt mit Nachweis (Testdatei, Messung) und einem Eintrag in
  `CHANGELOG.md`.

## Sicherheitsrelevante Meldungen

Siehe [`SECURITY.md`](SECURITY.md).
