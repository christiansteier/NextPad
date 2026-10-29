#!/usr/bin/env bash
# Browser- und API-Tests gegen frisch gestartete Server mit leerem Datenbestand.
# Läuft lokal und in der CI gleich. Endet mit Code 1, sobald ein Test fehlschlägt.
#
#   bash web/test/browsertests.sh [<nextpad-programm>] [<test> ...]
#
#   <nextpad-programm>  Vorgabe: server/target/debug/nextpad (bzw. $CARGO_TARGET_DIR/debug/nextpad)
#   <test>              nur diese Tests, z. B. synctest darktest; Vorgabe: alle
#   NEXTPAD_WEB_BUILD   Bau-Verzeichnis von web/build.sh (node_modules), Vorgabe ~/.cache/nextpad-web
#   NEXTPAD_TEST_PORT   erster von vier freien Ports, Vorgabe 5195
#   NEXTPAD_TEST_LOGS   bei Fehlschlag Serverprotokolle und Testausgaben hierhin kopieren (CI-Artefakt)
#
# Vier Server: Hauptserver (Erscheinungsbild hsd, freie Pads, alle Addons), LTI mit
# nachgespielter Plattform, Addon-Server (erst alle Addons, dann nur die Ideensammlung),
# Verwaltung Erscheinungsbild (LTI wie oben, mit NEXTPAD_ADMINS).
# Ohne Theme-Ordner: Schriften und Logo der HSD gehören nicht ins Repo.
# Hohe Grenze für neue freie Pads: alle Tests kommen von derselben Adresse und legen zusammen mehr als 20 an.
set -uo pipefail
HERE="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="${1:-${CARGO_TARGET_DIR:-$HERE/server/target}/debug/nextpad}"
[ $# -gt 0 ] && shift
NUR=" $* "
WEB="${NEXTPAD_WEB_BUILD:-$HOME/.cache/nextpad-web}"
PORT="${NEXTPAD_TEST_PORT:-5195}"
[ -x "$BIN" ] || { echo "Programm fehlt: $BIN (erst cargo build)" >&2; exit 2; }
[ -d "$WEB/node_modules/playwright" ] || { echo "Playwright fehlt in $WEB (erst web/build.sh)" >&2; exit 2; }
# Die Tests laufen aus dem Bau-Verzeichnis, weil dort node_modules liegt
[ "$(cd "$HERE/web/test" && pwd)" = "$(cd "$WEB/test" 2>/dev/null && pwd)" ] || rsync -a --delete "$HERE/web/test/" "$WEB/test/"

TMP="$(mktemp -d)"
KEY="$(openssl rand -hex 24)"
pids=(); fehler=()
aufraeumen() {
  [ ${#pids[@]} -gt 0 ] && kill "${pids[@]}" 2>/dev/null; wait 2>/dev/null
  if [ ${#fehler[@]} -gt 0 ] && [ -n "${NEXTPAD_TEST_LOGS:-}" ]; then
    mkdir -p "$NEXTPAD_TEST_LOGS"; cp "$TMP"/*.log "$TMP"/*.out "$TMP"/*.png "$NEXTPAD_TEST_LOGS/" 2>/dev/null
  fi
  rm -rf "$TMP"
}
trap aufraeumen EXIT

# starte <name> <port> [VAR=wert ...]: Server mit eigenem Datenordner, wartet auf Antwort
starte() {
  local name="$1" port="$2"; shift 2
  mkdir -p "$TMP/$name"
  [ -f "$TMP/$name/api-keys.txt" ] || echo "$KEY" > "$TMP/$name/api-keys.txt"
  env NEXTPAD_DATA="$TMP/$name" NEXTPAD_BIND="127.0.0.1:$port" NEXTPAD_ALLOW_FREE_PADS=true \
    NEXTPAD_RETENTION=dry-run NEXTPAD_THEME=hsd NEXTPAD_FREE_PADS_PER_HOUR=1000 "$@" "$BIN" >> "$TMP/$name.log" 2>&1 &
  pids+=($!)
  for _ in $(seq 100); do
    curl -fs -o /dev/null "http://127.0.0.1:$port/" && return 0
    sleep 0.2
  done
  echo "Server $name startet nicht:" >&2; cat "$TMP/$name.log" >&2; exit 1
}
stoppe_letzten() { local p="${pids[-1]}"; kill "$p"; wait "$p" 2>/dev/null; unset 'pids[-1]'; }

gesamt=0
# pruefe <test> [argumente ...]
pruefe() {
  local t="$1"; shift
  case "$NUR" in "  ") ;; *" $t "*) ;; *) return 0 ;; esac
  gesamt=$((gesamt + 1))
  if timeout 300 node "$WEB/test/$t.mjs" "$@" > "$TMP/out" 2>&1; then
    echo "ok    $t  $(grep -E 'ok, ' "$TMP/out" | tail -1)"
  else
    echo "FAIL  $t"; sed 's/^/      /' "$TMP/out"; cp "$TMP/out" "$TMP/$t.out"; fehler+=("$t")
  fi
}

U="http://127.0.0.1:$PORT"
starte haupt "$PORT"
export NEXTPAD_URL="$U" NEXTPAD_KEY_FILE="$TMP/haupt/api-keys.txt" SHOT="$TMP/board.png"
IDS="$TMP/ids.txt"
pruefe apitest "$IDS"
pruefe synctest "$IDS"
pruefe timelinetest "$IDS"
pruefe wiederherstelltest "$U"
pruefe aufgabentest
pruefe einruecktest
pruefe markdowntest
pruefe guesttest
pruefe tasklist "$U" "$NEXTPAD_KEY_FILE"
pruefe startform "$U"
pruefe recenttest "$U"
pruefe darktest "$U"
pruefe sprachtest "$U"
pruefe ideentest "$U"
pruefe quiztest "$U"
pruefe buzzertest "$U"
pruefe messungtest "$U"
pruefe boardtest "$U"

case "$NUR" in "  "|*" ltitest "*)
  L="$TMP/lti"; node "$WEB/test/ltitest.mjs" setup "$L" > /dev/null
  starte lti $((PORT + 1)) NEXTPAD_PUBLIC_URL=https://nextpad.test
  pruefe ltitest run "$L" "http://127.0.0.1:$((PORT + 1))" ;;
esac

case "$NUR" in "  "|*" addontest "*)
  A="http://127.0.0.1:$((PORT + 2))"
  starte addon $((PORT + 2))
  pruefe addontest anlegen "$A" "$TMP/addon.json"
  stoppe_letzten
  starte addon $((PORT + 2)) NEXTPAD_ADDONS=ideen
  pruefe addontest pruefen "$A" "$TMP/addon.json" ;;
esac

case "$NUR" in "  "|*" admintest "*)
  M="$TMP/admin"; node "$WEB/test/ltitest.mjs" setup "$M" > /dev/null
  starte admin $((PORT + 3)) NEXTPAD_PUBLIC_URL=https://nextpad.test NEXTPAD_ADMINS=admin@example.org
  pruefe admintest "$M" "http://127.0.0.1:$((PORT + 3))" ;;
esac

echo
if [ ${#fehler[@]} -gt 0 ]; then
  echo "${#fehler[@]} von $gesamt Tests fehlgeschlagen: ${fehler[*]}"; exit 1
fi
echo "alle $gesamt Tests bestanden"
