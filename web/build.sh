#!/usr/bin/env bash
# Baut das Editor-Bundle außerhalb des noexec-Mounts (pnpm + esbuild) und kopiert es nach server/static/.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BUILD="${NEXTPAD_WEB_BUILD:-$HOME/.cache/nextpad-web}"
mkdir -p "$BUILD"
rsync -a --delete --exclude node_modules --exclude dist "$HERE/" "$BUILD/"
cd "$BUILD"
if [ -f pnpm-lock.yaml ]; then pnpm install --frozen-lockfile || pnpm install; else pnpm install; fi
pnpm rebuild esbuild >/dev/null 2>&1 || true
pnpm run build
cp pnpm-lock.yaml "$HERE/pnpm-lock.yaml"
mkdir -p "$HERE/../server/static"
cp dist/editor.js dist/editor.css "$HERE/../server/static/"
rm -rf "$HERE/../server/static/board" && cp -r dist/board "$HERE/../server/static/board"
du -sh "$HERE/../server/static/board"; ls -la "$HERE/../server/static/"
