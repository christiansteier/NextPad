# NextPad as a container

Deutsche Fassung: [docker.de.md](docker.de.md)

An alternative to the YunoHost app (`packaging/nextpad_ynh`) for your own server or a VM. No ready-made image is published at the moment; build it from the root of the repository with `docker build -t nextpad .`.

Built after the pattern of the linuxserver.io images: base `ghcr.io/linuxserver/baseimage-alpine:3.23` with s6-overlay, the service runs as user `abc` with a configurable ID (`PUID`/`PGID`), everything that changes lives under `/config`. The program is statically linked against musl and uses mimalloc as its allocator.

## docker compose (recommended)

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
      - /path/to/nextpad/config:/config
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
  -v /path/to/nextpad/config:/config \
  --restart unless-stopped \
  nextpad
```

## Parameters

| Parameter | Function |
|---|---|
| `-p 5090` | HTTP and WebSocket; run only behind a reverse proxy with TLS |
| `-e PUID=1000` | User ID that owns `/config/data` |
| `-e PGID=1000` | Group ID |
| `-e TZ=Europe/Berlin` | Time zone |
| `-e NEXTPAD_*` | All settings from [`server/README.md`](../server/README.md); paths are already set to `/config` in the image |
| `-v /config` | `data/` (SQLite), `secrets/` (`api-keys.txt`, `lti-platforms.json`, platform keys), `theme/` (`logo.png`, `font.ttf`, `background.jpg`) |

On first start the container creates the folders and generates an API key in `/config/secrets/api-keys.txt` if there is none (notice in the log). Enter this key in Moodle in the Etherpad Lite plugin.

The interface is in German and English; each person switches with "DE | EN" in the page header. Nothing needs to be configured for this.

## Limits as for a small VM

Recommended when running next to other services:

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

s6-overlay needs these five capabilities at start (adjust the user, set permissions under `/config`, drop privileges); after that the service itself runs without privileges as `abc`. A read-only file system is not easily supported by this pattern.

## Update

Back up `/config/data` first: a new version may add columns to the database when it starts.

```bash
docker compose -f /opt/nextpad/compose.yaml pull
docker compose -f /opt/nextpad/compose.yaml up -d
docker image prune -f
```

## Maintenance in the container

`with-contenv` passes the container's environment (paths, settings) to the commands:

```bash
docker exec nextpad with-contenv s6-setuidgid abc /app/nextpad compact        # merge the update log
docker exec nextpad with-contenv s6-setuidgid abc /app/nextpad retention-run  # retention (mode from NEXTPAD_RETENTION)
docker logs -f nextpad
```
