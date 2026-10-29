## Dateien

| Ort | Inhalt |
|---|---|
| `__DATA_DIR__/data/` | Datenbank (SQLite mit WAL) |
| `__DATA_DIR__/secrets/api-keys.txt` | API-Schlüssel für Moodle; Zeile: `<schlüssel> [https://moodle-adresse]` (mit Adresse zeigt NextPad Profilbilder aus diesem Moodle) |
| `__DATA_DIR__/secrets/lti-platforms.json` | optional: Moodle-Instanzen für die Anmeldung per LTI 1.3 (Format in `server/README.de.md` des Projekts) |
| `__DATA_DIR__/theme/` | optional: `logo.png`, `font.ttf`, `background.jpg` für das HSD-Design (nicht im Paket, freigabepflichtig) |
| `__INSTALL_DIR__/nextpad.env` | erzeugte Einstellungen; ändern über das Konfigurationsfenster |

Nach Änderungen an Dateien in `secrets/` oder `theme/`: `systemctl restart __APP__`.

## Moodle einrichten

Plugin Etherpad Lite (Website-Administration › Plugins › Aktivitäten › Etherpad Lite):

- Server-URL: `https://__DOMAIN____PATH__/` (mit Schrägstrich am Ende)
- API-Schlüssel: erste Zeile aus `secrets/api-keys.txt`
- Die Quelladresse des Moodle-Servers im Konfigurationsfenster unter „Quelladressen“ eintragen.
- Moodle-Adresse unter „Seiten, die NextPad einbetten dürfen“ ergänzen.

## Wartung

```bash
sudo yunohost app shell __APP__        # Umgebung der App
systemctl status __APP__ ; journalctl -u __APP__ -f
```

Zusammenführen des Update-Logs und Probelauf der Aufbewahrung von Hand (mit den Einstellungen des Dienstes):

```bash
sudo -u __APP__ bash -c 'set -a; . __INSTALL_DIR__/nextpad.env; __INSTALL_DIR__/nextpad compact'
sudo -u __APP__ bash -c 'set -a; . __INSTALL_DIR__/nextpad.env; NEXTPAD_RETENTION=dry-run __INSTALL_DIR__/nextpad retention-run'
```

Die Oberfläche gibt es auf Deutsch und Englisch; jede Person wechselt mit „DE | EN“. Die Mail vor dem Löschen kommt in der Sprache, die die besitzende Person zuletzt gewählt hat, sonst in beiden.
