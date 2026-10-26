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
cargo build --release      # Freigabe-Binary mit LTO, stripped
```

Die Browsertests in `web/test/` laufen mit Playwright (Chromium) und prüfen Sync, API,
Zeitleiste, LTI, Last und Barrierefreiheit. Die CI (`.github/workflows/ci.yml`) baut den
Server und führt `cargo test` und clippy aus; die Browsertests laufen bisher von Hand.

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
