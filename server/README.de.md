# NextPad-Server

English version: [README.md](README.md)

Rust (Axum, yrs). Bildet die Etherpad-API für mod_etherpadlite nach und betreibt dahinter Yjs-Dokumente mit WebSocket-Sync.

## Bauen und starten

    cd server
    CARGO_TARGET_DIR=~/.cache/cargo-target/nextpad cargo build --release

Das Editor-Bundle liegt gebaut unter `static/` und wird zur Übersetzungszeit eingebettet (`include_str!`). Neu bauen: `bash ../web/build.sh`, danach `cargo build`.

Konfiguration über Umgebungsvariablen:

| Variable | Standard | Bedeutung |
|---|---|---|
| `NEXTPAD_BIND` | `127.0.0.1:5090` | Adresse und Port |
| `NEXTPAD_DATA` | `./data` | Datenverzeichnis (SQLite `nextpad.sqlite`) |
| `NEXTPAD_API_KEYS_FILE` | `<data>/api-keys.txt` | ein Schlüssel je Zeile, mindestens 32 Zeichen, optional danach die Basis-URL der zugehörigen Moodle (`<schlüssel> https://moodle.example.org`): dann zeigt der Editor die Moodle-Profilbilder (`user/pix.php/<id>/f1.jpg`, vom Browser mit der Moodle-Sitzung geladen) und trennt gleiche Nutzer-IDs verschiedener Moodles; `#` leitet Kommentare ein |
| `NEXTPAD_API_ALLOWED` | `127.0.0.1,::1` | erlaubte Quelladressen für `/api/*`, Adressen oder Netze (CIDR), kommagetrennt, `*` für alle |
| `NEXTPAD_TRUST_PROXY` | `false` | `true`: Quelladresse aus `X-Real-IP` bzw. letztem `X-Forwarded-For`-Eintrag, aber nur von Adressen in `NEXTPAD_TRUSTED_PROXIES` |
| `NEXTPAD_TRUSTED_PROXIES` | `127.0.0.1,::1` | Reverse-Proxys, deren Kopfzeilen gelten (CIDR erlaubt) |
| `NEXTPAD_ALLOWED_ORIGINS` | leer | zusätzliche erlaubte Origins für WebSocket-Verbindungen; der eigene Host ist immer erlaubt |
| `NEXTPAD_FRAME_ANCESTORS` | `'self'` | Wert für `Content-Security-Policy: frame-ancestors` |
| `NEXTPAD_BASE_PATH` | leer | Pfadpräfix hinter dem Reverse-Proxy, z. B. `/np` (nur für Links in der Seite; der Proxy schneidet den Präfix ab) |
| `NEXTPAD_API_VERSION` | `1.2.15` | Antwort von `GET /api` |
| `NEXTPAD_ALLOW_FREE_PADS` | `false` | freie Pads: Startseite mit „Neues Pad“ und Pad per Name, Gäste schreiben ohne Moodle (Cookie-Identität), `createPad` über die API |
| `NEXTPAD_ADDONS` | alle | eingeschaltete Addons, kommagetrennt aus `ideen`, `quiz`, `zeichenbrett`; leer = keins, unbekannte Kennung = Startfehler. Ausgeschaltet: keine Kachel, keine Routen, keine Hilfe, kein Moodle-Typ; bestehende Pads zeigen einen Hinweis, ihre Daten bleiben und gehen beim Löschen des Pads mit |
| `NEXTPAD_FREE_PADS_PER_HOUR` | `20` | höchstens so viele neue freie Pads je Quelladresse und Stunde |
| `NEXTPAD_AUTHOR_COOKIE` | `nextpad_author` | Name des Gast-Cookies |
| `NEXTPAD_THEME` | `colibris` | Erscheinungsbild: `colibris` (wie Etherpad) oder `hsd` (Corporate Design der Hochschule Düsseldorf) |
| `NEXTPAD_THEME_DIR` | leer | Vorgabe-Verzeichnis mit `logo.png`, `background.jpg` und `font.ttf` für das Design; Marke und Hausschrift liegen bewusst nicht im Repo |
| `NEXTPAD_BACKGROUND_CREDIT` | leer | Bildnachweis für `background.jpg` im Design-Verzeichnis (Hintergrund der HSD-Startseite), z. B. `Foto: Jula2812, CC BY-SA 4.0, Wikimedia Commons`; bei Bildern unter CC-Lizenz Pflicht |
| `NEXTPAD_BACKGROUND_CREDIT_URL` | leer | Link des Bildnachweises (Dateiseite auf Commons) |
| `NEXTPAD_HISTORY_DAYS` | `30` | so lange zeigt die Zeitleiste jeden Einzelschritt; ältere Abschnitte nur als Tagesstände (`nextpad compact`) |
| `NEXTPAD_SITE_NAME` | leer | Betreiberangabe im Fuß der Startseite (z. B. `ZWEK der Hochschule Düsseldorf`) |
| `NEXTPAD_ADMINS` | leer | Mailadressen aus Moodle (mit Komma getrennt), die das Erscheinungsbild einstellen dürfen; die Moodle-Administration darf es immer |
| `NEXTPAD_COOKIE_NAME` | `sessionID` | Name des Session-Cookies, das Moodle setzt |
| `NEXTPAD_PUBLIC_URL` | leer | öffentliche Adresse mit Basis-Pfad, z. B. `https://host/np`; Pflicht für LTI (Rücksprungadresse) und für Links in Mails |
| `NEXTPAD_LTI_FILE` | `<data>/lti-platforms.json` | Moodle-Instanzen für die Anmeldung per LTI 1.3, fehlt die Datei: keine Anmeldung |
| `NEXTPAD_LOGIN_COOKIE` | `nextpad_login` | Name des Anmelde-Cookies (14 Tage) |
| `NEXTPAD_FOOTER_LINKS` | leer | Links im Fuß der Startseite, `Beschriftung|https://…;Beschriftung|https://…` |
| `NEXTPAD_RETENTION` | `off` | Aufbewahrung freier Pads: `off`, `dry-run` (nur protokollieren), `on` |
| `NEXTPAD_RETENTION_DAYS` | `365` | Frist ohne Aktivität in Tagen (mindestens 30) |
| `NEXTPAD_NOTICE_DAYS` | `14` | Ankündigung per Mail so viele Tage vorher |
| `NEXTPAD_SMTP_URL` / `NEXTPAD_SMTP_URL_FILE` | leer | Mailausgang im lettre-Format, z. B. `smtp://127.0.0.1:25` oder `smtps://user:pass@host:465`; mit Zugangsdaten besser als Datei (systemd `LoadCredential`) |
| `NEXTPAD_MAIL_FROM` | leer | Absender der Ankündigungen |
| `RUST_LOG` | `info` | Log-Filter |

### LTI-Plattformdatei

```json
[{
  "name": "test-moodle",
  "label": "Mit Moodle anmelden",
  "issuer": "https://moodle.example.org",
  "client_id": "<aus der Werkzeugkonfiguration>",
  "deployment_ids": ["<id des Werkzeugs>"],
  "auth_url": "https://moodle.example.org/mod/lti/auth.php",
  "jwks_url": "https://moodle.example.org/mod/lti/certs.php",
  "start_url": "https://moodle.example.org/mod/lti/launch.php?id=<cmid>"
}]
```

`jwks_file` statt `jwks_url`, wenn der Server die Moodle-Adresse nicht erreicht (etwa hinter einer SSO-Sperre). Dann nach einem Schlüsselwechsel in Moodle die Datei erneuern. `label` ist die Beschriftung des Anmeldeknopfs und erscheint so, wie es dasteht, in jeder Sprache.

In Moodle (Website-Administration › Plugins › Externes Tool › Tools verwalten), LTI 1.3:

| Feld | Wert |
|---|---|
| Tool-URL | `<public>/lti/launch` |
| Login-URL | `<public>/lti/login` |
| Weiterleitungs-URI | `<public>/lti/launch` |
| Public-Key-Typ | Keyset-URL, `<public>/lti/jwks` (leer, NextPad signiert nichts) |
| Name senden | immer |
| E-Mail senden | immer (für die Mail vor dem Löschen) |
| Standardstart | neues Fenster |

Danach eine Aktivität mit diesem Werkzeug auf der Moodle-Startseite anlegen. Ihre Adresse `mod/lti/launch.php?id=<cmid>` ist `start_url`.

### Pad, Ideensammlung oder Quiz als Kursaktivität

Im Kurs eine Aktivität „Externes Tool“ mit diesem Werkzeug anlegen und unter „Mehr anzeigen“ › „Benutzerdefinierte Parameter“ eintragen, was entstehen soll:

- `typ=pad`: ein gemeinsames Text-Pad (wie ein Etherpad-Gruppenpad). Alternativ Tool-URL `<public>/lti/pad`.
- `typ=ideen`: eine Ideensammlung (Details unten).
- `typ=quiz`: ein Quiz-Raster; Lehrende leiten, der Name der Aktivität wird zum Titel.

Bei jedem Start merkt sich NextPad die Adresse der Aktivität (`<moodle>/mod/lti/view.php?l=<resource_link.id>`, für den QR-Code) und die Kursseite (`<moodle>/course/view.php?id=<context.id>`, für „Zurück zum Kurs“). Beides nur, wenn Moodle Nummern schickt.

### Ideensammlung als Kursaktivität

Im Kurs eine Aktivität „Externes Tool“ mit diesem Werkzeug anlegen und unter „Mehr anzeigen“ › „Benutzerdefinierte Parameter“ `typ=ideen` eintragen. Der Name der Aktivität wird zur Frage der Sammlung. Alternativ zeigt ein eigener Werkzeugeintrag mit der Tool-URL `<public>/lti/ideen` direkt auf die Sammlung.

Jede Aktivität bekommt eine eigene Gruppe mit einer Sammlung. Hinein kommt nur, wer die Aktivität aus Moodle startet: NextPad legt dabei eine Sitzung an wie bei Etherpad-Gruppenpads (Cookie `NEXTPAD_COOKIE_NAME`, mehrere Sitzungen durch Komma getrennt, `SameSite=None`, damit die Aktivität auch eingebettet läuft). Rollen Instructor, TeachingAssistant, ContentDeveloper und Administrator moderieren. Löschen lässt sich eine Kurs-Sammlung nicht aus NextPad heraus, sie folgt keiner Aufbewahrungsfrist.

### Aufbewahrung prüfen

```sh
NEXTPAD_RETENTION=dry-run nextpad retention-run
```

zeigt, welche Pads angekündigt oder gelöscht würden, ohne etwas zu tun.

### Erscheinungsbild einstellen

Im Design `hsd` stellen Admins unter `/admin/erscheinungsbild` Name und Untertitel im Kopf, die Einrichtung (Name des Logos), das Logo, das Hintergrundbild der Startseite samt Bildnachweis und die Farben ein. Admin ist, wer sich über Moodle (LTI) anmeldet und dort zur Administration gehört (Rolle `institution/person#Administrator` oder `system/person#Administrator`) oder mit seiner Mailadresse in `NEXTPAD_ADMINS` steht. Angemeldete Admins sehen auf der Startseite den Knopf „Erscheinungsbild“.

- Farben: eine Akzentfarbe (Kopflinie, Fokusrahmen) und drei Hausfarben. Linien und Tönungen für hell und dunkel leitet NextPad ab (Linien auf mindestens 4,5:1 abgedunkelt, Tönungen als Anteil auf Weiß bzw. Dunkelgrau); wer will, legt sie selbst fest. Kontraste unter WCAG 2.2 meldet die Seite.
- Logo: PNG, JPEG, WebP oder SVG bis 2 MB; SVG nur ohne Skripte und Ereignisattribute, ausgeliefert mit `sandbox`. Mit Logo der Einrichtung entfällt das NextPad-Zeichen im Kopf.
- Hintergrundbild: JPEG, PNG oder WebP bis 6 MB.
- Ohne Einstellung gilt die Vorgabe des Servers (`NEXTPAD_THEME_DIR`, `NEXTPAD_BACKGROUND_CREDIT`). „Alles auf die Vorgabe zurücksetzen“ stellt sie wieder her; hochgeladene Bilder bleiben liegen und sind wieder wählbar.
- Gespeichert wird in der Datenbank (Tabelle `settings`), Bilder unter `<NEXTPAD_DATA>/erscheinungsbild/`.

### Sprache

Seiten und Meldungen gibt es auf Deutsch und Englisch, soweit sie schon umgestellt sind (bisher Fehlerseiten, Zugriffsmeldungen, alle Seiten für Nutzende einschließlich Ideensammlung, Quiz-Raster und Buzzer-Messung; Exporte und Druckansicht; die Mail vor dem Löschen und die Verwaltungsseite Erscheinungsbild; ein dort leer gelassener Untertitel erscheint als Vorgabe in der Sprache der Seite, ein eingetragener so, wie er dasteht). Die Mail vor dem Löschen geht in der Sprache, die die Besitzerin oder der Besitzer zuletzt bei Anmeldung über Moodle gewählt hat (Spalte `authors.sprache`, gesetzt über `/sprache/…`); ohne Wahl in beiden Sprachen in einer Mail, Deutsch zuerst, das PDF dann deutsch. Die Ideen-CSV trennt auf Deutsch mit Semikolon, auf Englisch mit Komma, wie Tabellenprogramme in der jeweiligen Sprache es erwarten; Inhaltsverzeichnis in gespeicherten Ständen und in der Etherpad-API bleibt deutsch. Die Sprache einer Anfrage, in dieser Reihenfolge:

1. eigene Wahl: Cookie `nextpad-lang`, gesetzt über den Umschalter „DE | EN“ (Kopf der übersetzten Seiten, Werkzeugleiste des Editors; Seitenrahmen mit `wahl: None` zeigen ihn nicht) bzw. `GET /sprache/de` oder `/sprache/en` (`?zurueck=<Pfad>` führt auf diesen Server zurück, fremde Ziele führen zur Startseite);
2. Sprache aus Moodle: Cookie `nextpad-lang-moodle`, bei jedem LTI-Start aus `launch_presentation.locale` aufgefrischt, eine nicht unterstützte Sprache löscht es;
3. Browsersprache (`Accept-Language`, nach Gewicht);
4. Deutsch.

Beide Cookies enthalten nur das Sprachkürzel, gelten ein Jahr und sind `HttpOnly; Secure; SameSite=None` (sonst gälte die Wahl nicht in der in Moodle eingebetteten Aktivität). Texte stehen mit Schlüssel in `src/texte/de.rs` und `en.rs`, die Addons tragen eigene (`Addon::texte`); ein Unit-Test prüft, dass beide Sprachen dieselben Schlüssel und Platzhalter haben. Schlüssel auf `_html` enthalten Markup und werden nicht maskiert. Die Hilfeseite liegt als `src/texte/hilfe.de.html` und `hilfe.en.html`. Skripte im Browser bekommen ihre Texte von der Seite: `texte::fuer_skript(sp, "startjs.")` erzeugt JSON für einen `<script type="application/json">`-Block (so liest `start.js` den Block `#start-texte`). Der Editor bekommt `texte` und `lang` im Konfigurationsblock `#nextpad-config` (`texte::texte_mit(sp, "editor.")`); `web/src/texte.js` stellt `tx()` und das Datumsformat für alle Editor-Module bereit.

## Endpunkte

`GET /api` (Version), `GET|POST /api/1.2*/<Funktion>` (siehe `docs/api-spezifikation.md`), `GET /p/<padID>` und `/p/<readOnlyID>` (Editor), `GET /ws/<id>` (Yjs-Sync), `GET /p/<id>/export/html|txt|md|pdf|print`, `GET /p/<id>/snapshots`, `GET /p/<id>/snapshots/<ts>`, `GET /status`, `GET /static/editor.js|css|print.js`, `GET /sprache/de|en`; mit freien Pads zusätzlich `GET /` (Startseite), `GET /new`, `GET /open?padname=`, `POST /me` (eigener Name), `GET /hilfe`, `GET /new?vorlage=gruppenarbeit|protokoll`, `GET /new?typ=zeichenbrett|ideen|quiz`, `GET|POST /p/<id>/ideen`, `GET /p/<id>/ideen/live` (Server-Sent Events), `POST /p/<id>/ideen/frage`, `POST /p/<id>/ideen/<n>/stimme`, `POST /p/<id>/ideen/<n>/moderation`, `GET /p/<id>/leinwand`, `POST /p/<id>/extend` und `POST /p/<id>/delete` (nur Besitz), `GET|POST /verlaengern` (Link aus der Mail); mit LTI `GET|POST /lti/login`, `POST /lti/launch` (auch Start einer Kursaktivität), `GET /lti/jwks`, `POST /logout`, `GET|POST /admin/erscheinungsbild` und `POST /admin/erscheinungsbild/vorgabe` (nur Admins), `GET /theme/logo.png` und `/theme/background.jpg` (eingestelltes Bild in seinem Format).

## Aufbau

`config.rs` Konfiguration · `ids.rs` Kennungen · `store.rs` SQLite (Gruppen, Pads, Autoren, Sessions, Update-Log, Snapshots) · `docs.rs` geladene Dokumente, Broadcast, Persistenz, Snapshots · `sync.rs` WebSocket mit `yrs::sync` · `api.rs` Etherpad-API · `render.rs` Yjs ↔ HTML/Text · `web.rs` Editorseite, Zugriffsprüfung, Export, Status · `look.rs` Erscheinungsbild und Verwaltungsseite · `live.rs` Live-Kanal (Server-Sent Events) für Addons · `qr.rs` QR-Codes · `texte/` Texte auf Deutsch und Englisch.

Addons liegen je in `addons/<kennung>/` und beschreiben sich mit einer `Addon`-Konstante (`addons/mod.rs`): Pad-Art, Tabellen (`daten.rs`, als `Zusatz` an `Store::open`), Routen, Seite, Export, Kachel, Hilfe, Moodle-Typ, Texte. Der Kern fragt keinen Pad-Typ einzeln ab. `addons/ideen` Ideensammlung · `addons/quiz` Quiz-Raster mit Buzzer und Buzzer-Messung (`messung.rs`) · `addons/zeichenbrett` Excalidraw-Seite (das Bündel baut `web/build.sh` nach `static/board`). Skripte und Stile der Addons liegen in `addons/<kennung>/static/`, ausgeliefert weiter unter `/static/…`.

## Fallen (gefunden im Proof of Concept)

- yrs rechnet Textpositionen standardmäßig in Bytes; der Browser (Yjs) in UTF-16. Ohne `OffsetKind::Utf16` bricht alles nach dem ersten Umlaut (`docs.rs`).
- `Text::insert` ohne Attribute erbt die Formatierung des Vorgängers; deshalb immer `insert_with_attributes`, auch mit leerer Attributmenge (`render.rs`).
- Multipart-Boundary ist groß-/kleinschreibungssensitiv; `Content-Type` nicht vor dem Auslesen in Kleinbuchstaben wandeln (`api.rs`).

## Sicherheitsgrenzen (Review 24.10.2026)

- WebSocket: höchstens 1 MiB je Nachricht, 2 MiB Updates je Verbindung und Minute, Origin-Prüfung, Ping alle 30 s, Trennung nach 90 s Stille, Trennung bei Rückstand (Client synchronisiert neu).
- API-Körper höchstens 16 MiB (setHTML bei Kurs-Wiederherstellung).
- Lesemodus: Seite nennt nur die r.-Kennung, der WebSocket verwirft Updates.
- Gäste: Cookie enthält ein geheimes Token, nicht die öffentliche Autorenkennung; Namensänderung nur für Gäste.
- Exporte und Stände mit eigener CSP (`default-src 'none'`) und `nosniff`; Farbe, Schriftgröße und Ausrichtung nur aus Positivlisten.
- Gelöschte Pads schreiben weder Updates noch Snapshots (Prüfung in der Datenbank), Löschen in einer Transaktion.
- Rendern mit Tiefengrenze (48) gegen Stack-Overflow durch verschachtelte Dokumente.
