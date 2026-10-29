# NextPad als Container

English version: [docker.md](docker.md)

Alternative zur YunoHost-App (`packaging/nextpad_ynh`) für einen eigenen Server oder eine VM. Ein fertiges Image wird derzeit nicht veröffentlicht; gebaut wird es aus dem Wurzelverzeichnis des Repos mit `docker build -t nextpad .`.

Aufbau nach dem Muster der Images von linuxserver.io: Basis `ghcr.io/linuxserver/baseimage-alpine:3.23` mit s6-overlay, Dienst läuft als Nutzer `abc` mit einstellbarer Kennung (`PUID`/`PGID`), alles Veränderliche liegt unter `/config`. Das Programm ist statisch gegen musl gebaut und nutzt mimalloc als Speicherverwaltung.

## docker compose (empfohlen)

```yaml
services:
  nextpad:
    image: nextpad
    container_name: nextpad
    environment:
      - PUID=1000
      - PGID=1000
      - TZ=Europe/Berlin
      - NEXTPAD_PUBLIC_URL=https://pads.example.org
      - NEXTPAD_TRUST_PROXY=true
      - NEXTPAD_TRUSTED_PROXIES=192.0.2.1
      - NEXTPAD_API_ALLOWED=192.0.2.10
      - NEXTPAD_THEME=colibris
    volumes:
      - /pfad/zu/nextpad/config:/config
    ports:
      - 127.0.0.1:5090:5090
    restart: unless-stopped
```

## docker cli

```bash
docker run -d \
  --name=nextpad \
  -e PUID=1000 -e PGID=1000 -e TZ=Europe/Berlin \
  -e NEXTPAD_PUBLIC_URL=https://pads.example.org \
  -p 127.0.0.1:5090:5090 \
  -v /pfad/zu/nextpad/config:/config \
  --restart unless-stopped \
  nextpad
```

## Parameter

| Parameter | Funktion |
|---|---|
| `-p 5090` | HTTP und WebSocket; nur hinter einem Reverse-Proxy mit TLS betreiben |
| `-e PUID=1000` | Benutzerkennung, der `/config/data` gehört |
| `-e PGID=1000` | Gruppenkennung |
| `-e TZ=Europe/Berlin` | Zeitzone |
| `-e NEXTPAD_*` | alle Einstellungen aus [`server/README.de.md`](../server/README.de.md); Pfade sind im Image schon auf `/config` gesetzt |
| `-v /config` | `data/` (SQLite), `secrets/` (`api-keys.txt`, `lti-platforms.json`, Plattformschlüssel), `theme/` (`logo.png`, `font.ttf`, `background.jpg`) |

Beim ersten Start legt der Container die Ordner an und erzeugt einen API-Schlüssel in `/config/secrets/api-keys.txt`, wenn keiner vorhanden ist (Hinweis im Log). Diesen Schlüssel in Moodle beim Plugin Etherpad Lite eintragen.

Die Oberfläche gibt es auf Deutsch und Englisch; jede Person wechselt mit „DE | EN“ im Seitenkopf. Einzustellen ist dafür nichts.

## Grenzen wie bei einer kleinen VM

Empfohlen für den Betrieb neben anderen Diensten:

```yaml
    cap_drop: [ALL]
    cap_add: [CHOWN, DAC_OVERRIDE, FOWNER, SETUID, SETGID]
    security_opt: ["no-new-privileges:true"]
    mem_limit: 512m
    memswap_limit: 512m
    cpus: 1.0
    pids_limit: 256
    ulimits:
      nofile: { soft: 65536, hard: 65536 }
```

Die fünf Capabilities braucht s6-overlay beim Start (Nutzer anpassen, Rechte unter `/config` setzen, Rechte abgeben); der Dienst selbst läuft danach ohne Rechte als `abc`. Ein schreibgeschütztes Dateisystem unterstützt dieses Muster nicht ohne Weiteres.

## Aktualisieren

Vorher `/config/data` sichern: Eine neue Fassung kann beim Start Spalten in der Datenbank ergänzen.

```bash
docker compose -f /opt/nextpad/compose.yaml pull
docker compose -f /opt/nextpad/compose.yaml up -d
docker image prune -f
```

## Wartung im Container

`with-contenv` gibt den Befehlen die Umgebung des Containers mit (Pfade, Einstellungen):

```bash
docker exec nextpad with-contenv s6-setuidgid abc /app/nextpad compact        # Update-Log zusammenführen
docker exec nextpad with-contenv s6-setuidgid abc /app/nextpad retention-run  # Aufbewahrung (Modus aus NEXTPAD_RETENTION)
docker logs -f nextpad
```
