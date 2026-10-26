# NextPad

English version: [README.md](README.md)

Ein schlanker Pad-Server in [Rust](https://www.rust-lang.org), der die
[Etherpad](https://etherpad.org)-HTTP-API nachbildet, damit Moodle über das unveränderte
Plugin `mod_etherpadlite` weiterläuft. Darunter liegt ein moderner Unterbau: gemeinsames
Schreiben über [Yjs](https://yjs.dev) (CRDT), der Editor [TipTap](https://tiptap.dev) und
eine Zeitleiste aus Snapshots. Kein Chat.

> **Name.** Der Name für Nutzende ist **NextPad** (Adresse `/np`). Im Code, in Repos und
> Paketen heißt das Projekt weiter `cooppad`.

## Worum es geht

Das ZWEK betreibt für die Moodle-Plattformen der Hochschule einen Etherpad-Server. Moodle
bindet Pads über das Plugin `mod_etherpadlite` (Humboldt-Universität zu Berlin) als
Aktivität in Kurse ein. Etherpad samt Plugins ist in der Pflege teuer geworden: Die
Plugin-Landschaft bremst jedes Upgrade, und der Abhängigkeitsbaum zählt über tausend
npm-Pakete.

Die Anforderung an den Dienst ist dagegen klein: Überschriften, Schriftgröße, fett,
kursiv, unterstrichen, durchgestrichen, Textfarben, Autorenfarben, Zeitleiste, dazu
Markdown. Kein Chat. Diese Lücke zwischen Bedarf und Betriebsaufwand schließt NextPad:
ein eigener Server, der genau die API-Aufrufe beantwortet, die das Moodle-Plugin absetzt,
und dahinter einen modernen Editor betreibt.

Etherpad bleibt als Rückfall bestehen, bis der neue Server sich im Betrieb bewährt hat.

## Was drinsteckt

- Drop-in-Etherpad-API für `mod_etherpadlite`: 36 Vertragsprüfungen grün, Roundtrip
  identisch.
- Gemeinsames Schreiben über Yjs/yrs (CRDT), keine Operational Transforms.
- Zeitleiste aus Snapshots, wie Etherpads Timeslider: Schieberegler, Abspielen, Zeit und
  Autor je Schritt.
- Export als HTML, Markdown, Text und PDF (mit [Typst](https://typst.app) gesetzt).
- Zeichenbrett über Excalidraw auf demselben Yjs-Server.
- Anmeldung über Moodle per LTI 1.3 (Namen und Profilbilder).
- Freie Pads mit Besitz- und Aufbewahrungsregeln.
- Ein Rust-Binary, SQLite, rund 50 MB RAM unter Last.

## Stand

Pilot seit 2026-10 im Produktivbetrieb an einer deutschen Hochschule. Belastungstest: ein
Pad, 100 Personen, 400 Eingaben/s, Median um 50 ms, p99 unter 80 ms. Die Lasttests liegen
in `web/test/`.

## Dokumentation

| Datei | Inhalt |
|---|---|
| `docs/api-spezifikation.md` | Die nachzubildende Etherpad-API, aus dem Plugin-Quellcode abgeleitet |
| `docs/docker.md` | Betrieb als Container |
| `server/README.md` | Konfiguration (Umgebungsvariablen), Befehle |
| `web/README.md` | Editor-Bündel und Browsertests |
| `packaging/nextpad_ynh/` | YunoHost-App |

Der Code ist die maßgebliche Quelle.

## Lokal ausprobieren

```bash
mkdir -p ~/.cache/cooppad-data
head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 48 > ~/.cache/cooppad-data/api-keys.txt
cd server
CARGO_TARGET_DIR=~/.cache/cargo-target/cooppad cargo build
COOPPAD_DATA=~/.cache/cooppad-data \
COOPPAD_FRAME_ANCESTORS="'self' http://127.0.0.1:5090" \
~/.cache/cargo-target/cooppad/debug/cooppad
```

Dann Gruppe, Pad, Autor und Session über die API anlegen (Beispiele in
`docs/api-spezifikation.md`) und `http://127.0.0.1:5090/p/<padID>` mit gesetztem Cookie
`sessionID` öffnen.

## Projekthintergrund

Diese Software wurde an der Hochschule Düsseldorf – University of Applied Sciences (HSD)
durch ZWEK – Zentrum für Weiterbildung und Kompetenzentwicklung entwickelt, als Nachfolger
des Etherpad-Dienstes hinter den Moodle-Plattformen der HSD.

## Zitation

Die maschinenlesbaren Zitationsmetadaten liegen in [`CITATION.cff`](CITATION.cff).

## Lizenz

Veröffentlicht unter der [European Union Public Licence 1.2](LICENSE) (EUPL-1.2; deutscher
Rechtstext, englische Übersetzung in [`LICENSE.en`](LICENSE.en)).
