# NextPad (technisch cooppad): Editor- und Zeichenbrett-Bündel (pnpm + esbuild), Server (Rust), Laufzeit ohne Toolchain und ohne Shell.
# Bau:   docker build -t cooppad:<tag> .
# Start: docker run -d -p 127.0.0.1:5090:5090 -v cooppad-data:/data \
#          -v /pfad/api-keys.txt:/run/secrets/api-keys:ro -e COOPPAD_API_KEYS_FILE=/run/secrets/api-keys cooppad:<tag>
# Voreinstellung ist das Colibris-Design. Eigenes Logo und eigene Schrift (Design hsd): -v /pfad/theme:/theme:ro -e COOPPAD_THEME=hsd -e COOPPAD_THEME_DIR=/theme

# 1. Editor-Bundle
FROM docker.io/library/node:22-bookworm-slim AS web
RUN npm install -g pnpm@11.28.5 --no-fund --no-audit
WORKDIR /web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/src ./src
COPY web/patch-board.mjs ./
RUN pnpm run build

# 2. Server (Abhängigkeiten zuerst, damit Code-Änderungen den Cache nicht verwerfen)
FROM docker.io/library/rust:1.99.1-alpine3.23 AS server
# statisch gegen musl, damit das Programm auf der Alpine-Basis von linuxserver.io ohne Abhängigkeiten läuft
RUN apk add --no-cache musl-dev build-base
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release --locked && rm -rf src target/release/deps/cooppad* target/release/cooppad*
COPY server/static ./static
COPY --from=web /web/dist/editor.js /web/dist/editor.css ./static/
COPY --from=web /web/dist/board ./static/board
COPY server/src ./src
RUN touch src/main.rs && cargo build --release --locked && mkdir -p /out/data

# 3. Laufzeit nach dem Muster von linuxserver.io: Basis-Image mit s6-overlay, Nutzer abc (PUID/PGID), alles unter /config
FROM ghcr.io/linuxserver/baseimage-alpine:3.23
ARG BUILD_DATE
ARG VERSION
LABEL build_version="NextPad version:- ${VERSION} Build-date:- ${BUILD_DATE}"
LABEL maintainer="ZWEK der Hochschule Düsseldorf"
LABEL org.opencontainers.image.title="NextPad" \
      org.opencontainers.image.description="Etherpad-kompatibler Pad-Server mit Yjs, Zeitleiste, Zeichenbrett und Moodle-Anmeldung (LTI 1.3)" \
      org.opencontainers.image.source="https://github.com/christiansteier/nextpad" \
      org.opencontainers.image.licenses="EUPL-1.2"
ENV COOPPAD_BIND=0.0.0.0:5090 \
    COOPPAD_DATA=/config/data \
    COOPPAD_API_KEYS_FILE=/config/secrets/api-keys.txt \
    COOPPAD_LTI_FILE=/config/secrets/lti-platforms.json \
    COOPPAD_THEME_DIR=/config/theme \
    COOPPAD_TRUST_PROXY=false \
    RUST_LOG=info
COPY docker/root/ /
COPY --from=server /src/target/release/cooppad /app/nextpad
RUN chmod +x /etc/s6-overlay/s6-rc.d/*/run
EXPOSE 5090
VOLUME /config
