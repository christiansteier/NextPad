<h1><img src="docs/bilder/nextpad-logo.svg" alt="NextPad" width="252" height="48"></h1>

English version: [README.md](README.md)

Ein schlanker Pad-Server in [Rust](https://www.rust-lang.org), der die
[Etherpad](https://etherpad.org)-HTTP-API nachbildet, damit Moodle über das unveränderte
Plugin `mod_etherpadlite` weiterläuft. Darunter liegt ein moderner Unterbau: gemeinsames
Schreiben über [Yjs](https://yjs.dev) (CRDT), der Editor [TipTap](https://tiptap.dev) und
eine Zeitleiste aus Snapshots. Kein Chat.

## Worum es geht

Das ZWEK betreibt für die Moodle-Plattformen der Hochschule einen Etherpad-Server. Moodle
bindet Pads über das Plugin `mod_etherpadlite` (Humboldt-Universität zu Berlin) als
Aktivität in Kurse ein. Etherpad samt Plugins ist in der Pflege mit den aktuellen
Ressourcen kaum noch leistbar: Die Plugin-Landschaft bremst jedes Upgrade, und der
Abhängigkeitsbaum zählt über tausend npm-Pakete.

Die Anforderung an den Dienst ist dagegen klein: Überschriften, Schriftgröße, fett,
kursiv, unterstrichen, durchgestrichen, Textfarben, Autorenfarben, Zeitleiste, dazu
Markdown. Kein Chat. Diese Lücke zwischen Bedarf und Betriebsaufwand schließt NextPad:
ein eigener Server, der genau die API-Aufrufe beantwortet, die das Moodle-Plugin absetzt,
und dahinter einen modernen Editor betreibt.

Etherpad bleibt als Rückfall bestehen, bis der neue Server sich im Betrieb bewährt hat.

## Warum Rust

Ein Pad-Server hängt offen im Netz und verarbeitet Eingaben von allen, die ein Pad öffnen.
Deshalb zählt neben dem Betriebsaufwand vor allem, wie viel Angriffsfläche er bietet.

- **Speichersicherheit.** Rust schließt Pufferüberläufe, Zugriffe auf freigegebenen
  Speicher und Datenrennen schon beim Übersetzen aus. Diese Fehlerklassen stehen hinter
  einem großen Teil der Sicherheitslücken in Serversoftware. Der eigene Code enthält kein
  `unsafe`; das erzwingt der Compiler (`#![forbid(unsafe_code)]`).
- **Kleine Laufzeit.** Auf dem Server läuft ein einzelnes Programm, ohne Node.js, ohne
  Paketverwaltung und ohne Plugins, die zur Laufzeit fremden Code nachladen. Bei Etherpad
  laufen Plugins mit allen Rechten im Serverprozess. Das Editor-Bündel für den Browser
  wird zwar mit npm gebaut, dieser Code läuft aber nur im Browser, nicht auf dem Server.
- **Feste und geprüfte Abhängigkeiten.** `Cargo.lock` legt alle Crates auf genaue
  Versionen fest; ein großer Teil davon gehört zum PDF-Satz mit Typst. Die CI prüft sie bei
  jedem Push mit `cargo audit` gegen die RustSec-Datenbank. Gemeldete Lücken, die NextPad
  nachweislich nicht erreichen, stehen mit Begründung in `server/.cargo/audit.toml`; jede
  neue Meldung lässt die CI fehlschlagen.
- **Eingaben bleiben Daten.** Pad-Inhalte gehen nur als Zeichenketten in HTML, Markdown
  und den PDF-Satz, nie als Markup oder Code. Die Anmeldung über Moodle nimmt nur
  RS256-signierte Tokens an.
- **Wenig Betrieb.** Ein Update heißt: eine Datei tauschen. Unter Last braucht der Server
  rund 50 MB Arbeitsspeicher.

Rust schützt nicht vor Logikfehlern und nicht vor falscher Konfiguration. Reverse-Proxy,
API-Schlüssel und die Liste erlaubter Adressen bleiben Aufgabe des Betriebs.

## Was drinsteckt

- Drop-in-Etherpad-API für `mod_etherpadlite`: 36 Vertragsprüfungen grün, Roundtrip
  identisch.
- Gemeinsames Schreiben über Yjs/yrs (CRDT), keine Operational Transforms.
- Zeitleiste aus Snapshots, wie Etherpads Timeslider: Schieberegler, Abspielen, Zeit und
  Autor je Schritt.
- Export als HTML, Markdown, Text und PDF (mit [Typst](https://typst.app) gesetzt).
- Tabellen (auch als Markdown einfügbar), Aufgabenlisten, Bewertungen mit Daumen hoch und
  runter im Text, Inhaltsverzeichnis als Seitenleiste und als Baustein im Text.
- Zeichenbrett über Excalidraw auf demselben Yjs-Server.
- Ideensammlung nach dem Vorbild von [liveask](https://github.com/liveask/liveask): Ideen und Fragen ohne Namen eintragen, abstimmen, live für alle, mit Leinwand-Ansicht und QR-Code für den Beamer. Der Server zählt eine Stimme je Person, moderieren darf nur, wer die Sammlung angelegt hat. Im Moodle-Kurs läuft sie als Aktivität „Externes Tool“ (LTI 1.3) mit Zugang nur für Kursmitglieder, dort moderieren die Lehrenden.
- Anmeldung über Moodle per LTI 1.3 (Namen und Profilbilder).
- Freie Pads mit Besitz- und Aufbewahrungsregeln.
- Oberfläche, Hilfe, Exporte und Mails auf Deutsch und Englisch, von jeder Person umschaltbar.
- Ein Rust-Binary, SQLite, rund 50 MB RAM unter Last.

## Stand

Pilot seit 2026-10 im Produktivbetrieb an einer deutschen Hochschule. Belastungstest: ein
Pad, 100 Personen, 400 Eingaben/s, Median um 50 ms, p99 unter 80 ms. Die Lasttests liegen
in `web/test/`.

## Dokumentation

| Datei | Inhalt |
|---|---|
| `docs/api-spezifikation.md` | Die nachzubildende Etherpad-API, aus dem Plugin-Quellcode abgeleitet |
| `docs/docker.de.md` | Betrieb als Container |
| `server/README.de.md` | Konfiguration (Umgebungsvariablen), Befehle |
| `web/README.md` | Editor-Bündel und Browsertests |
| `packaging/nextpad_ynh/` | YunoHost-App |

Der Code ist die maßgebliche Quelle.

## Lokal ausprobieren

```bash
mkdir -p ~/.cache/nextpad-data
head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 48 > ~/.cache/nextpad-data/api-keys.txt
bash web/build.sh            # Editor- und Zeichenbrett-Bündel (braucht Node 22 und pnpm)
cd server
CARGO_TARGET_DIR=~/.cache/cargo-target/nextpad cargo build
NEXTPAD_DATA=~/.cache/nextpad-data \
NEXTPAD_FRAME_ANCESTORS="'self' http://127.0.0.1:5090" \
~/.cache/cargo-target/nextpad/debug/nextpad
```

Dann Gruppe, Pad, Autor und Session über die API anlegen (Beispiele in
`docs/api-spezifikation.md`) und `http://127.0.0.1:5090/p/<padID>` mit gesetztem Cookie
`sessionID` öffnen.

## Projekthintergrund

Idee, Konzept und Entwicklung: Christian-Maximilian Steier.

Diese Software wurde an der Hochschule Düsseldorf – University of Applied Sciences (HSD)
durch ZWEK – Zentrum für Weiterbildung und Kompetenzentwicklung entwickelt, als Nachfolger
des Etherpad-Dienstes hinter den Moodle-Plattformen der HSD.

## Zitation

Die maschinenlesbaren Zitationsmetadaten liegen in [`CITATION.cff`](CITATION.cff).

## Lizenz

Veröffentlicht unter der [European Union Public Licence 1.2](LICENSE) (EUPL-1.2; deutscher
Rechtstext, englische Übersetzung in [`LICENSE.en`](LICENSE.en)).
