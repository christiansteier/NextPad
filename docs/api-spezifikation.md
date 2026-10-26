# API-Spezifikation: Was NextPad für mod_etherpadlite nachbilden muss

Stand 23.10.2026 · Quelle: Quellcode mod_etherpadlite v5.0.3 (github.com/moodlehu/moodle-mod_etherpadlite, Commit cb350cf, 23.01.2026), abgeglichen mit den Zugriffslogs des bestehenden Pad-Servers (09.07. bis 23.10.2026) · Status: Pflichtenheft für den Proof of Concept

Die Tabellen trennen, was das Plugin aufruft (Pflicht), was es nur im Client definiert (optional) und was es nie anfasst (weglassen). Fundstellen sind Datei und Zeile im Plugin-Repo.

## Grundregeln der Schnittstelle

| Punkt | Verhalten, das das Plugin erwartet | Fundstelle |
|---|---|---|
| Basis-URL | Einstellung `etherpadlite/url` mit Schrägstrich am Ende, z. B. `https://<pad-server>/pad/`. API unter `<url>/api`, Editor unter `<url>/p/<padID>` | client.php 75–81, view.php 92–94 |
| Versionsabfrage | `GET <url>/api` liefert JSON `{"currentVersion": "1.2.15"}`. Das Plugin verlangt `currentVersion >= apiversion` (Einstellung, Standard `1.2`), sonst Abbruch mit `error_wrong_api_version` | client.php 225–256 |
| Aufrufpfad | `<url>/api/<apiversion>/<Funktion>`, also mit `1.2` als Pfadsegment, nicht `1.2.15`. NextPad akzeptiert jede Version `1.2*` | client.php 164–166 |
| Authentifizierung | Parameter `apikey` in jedem Aufruf. Bei GET im Query-String, bei POST im Body (Moodle-curl sendet Formulardaten, `multipart/form-data`). NextPad akzeptiert Query, `application/x-www-form-urlencoded` und `multipart/form-data` | client.php 165–172 |
| Antwortformat | JSON `{"code": <int>, "message": "<text>", "data": <objekt oder null>}`. `code` 0 = ok; 1 = falsche Parameter; 2 = interner Fehler; 3 = unbekannte Funktion; 4 = kein oder falscher API-Key. Das Plugin wertet nur `code == 0` als Erfolg; `data` wird zurückgegeben, alles andere ist `false` und führt in Moodle zu einer Ausnahme | client.php 38–46, 193–221 |
| Fehlerfreundlichkeit | Antwort muss gültiges JSON sein; leere Antwort oder HTML (z. B. Login-Seite eines Reverse-Proxys) führt zu `false` | client.php 175–185 |
| Zeitlimits | Verbindungsaufbau 300 s, Gesamtzeit unbegrenzt (Einstellungen `connecttimeout`, `timeout`). NextPad antwortet in Millisekunden, die Werte sind nur Toleranz | client.php 95–106 |
| TLS | `check_ssl` steuert, ob Moodle das Zertifikat prüft. Bei 0 verifiziert Moodle weder Host noch Kette. NextPad liefert ein gültiges Zertifikat der Hochschul-CA, damit `check_ssl=1` möglich wird | client.php 108–112 |
| Blockierte Hosts | Moodle blockiert private Netze (`curlsecurityblockedhosts`); `ignoresecurity` hebt das auf. Betrifft nur Moodle | client.php 83–90, 740–766 |

## Pflicht: Aufrufe, die das Plugin absetzt

| Funktion | Methode | Parameter | Erwartete Antwort (`data`) | Wann | Fundstelle | Im Log gesehen |
|---|---|---|---|---|---|---|
| `checkToken` | GET | `apikey` | `null` bei code 0 | bei jeder Client-Erzeugung, also bei jedem Seitenaufruf, jeder Aktivität anlegen/ändern/löschen, Backup, Restore | client.php 258–260, 114–118 | ja (3.807) |
| `createGroup` | POST | – | `{"groupID": "g.<16 Zeichen>"}` | Aktivität anlegen; Kurs-Wiederherstellung | lib.php 62, restore_stepslib 67 | ja (160) |
| `createGroupPad` | POST | `groupID`, `padName`, optional `text` | `{"padID": "g.<id>$<padName>"}`; Fehler code 1 „padName does already exist“, wenn vorhanden | Aktivität anlegen (Hauptpad `padname`, je Moodle-Gruppe `padname<groupid>`), Gruppen-Ereignisse, Kurs-Reset, Restore | lib.php 67, 90; locallib 74; observer/group.php 67, 132, 223; util.php 87, 91; restore 75, 106 | ja (160) |
| `createAuthorIfNotExistsFor` | POST | `authorMapper` = Moodle-User-ID, `name` = Anzeigename | `{"authorID": "a.<16 Zeichen>"}` | jeder Seitenaufruf durch angemeldete Nutzende; der Name wird bei jedem Aufruf mitgegeben und soll aktualisiert werden | view.php 105 | ja (3.152) |
| `createAuthor` | POST | `name` (`Guest-<5 Ziffern>`) | `{"authorID": "a.…"}` | Gast-Zugriff (wenn erlaubt) und Nicht-Gruppenmitglieder mit Lehrrechten; bei jedem Aufruf ein neuer Autor | view.php 103, lib.php 283–292 | ja (7) |
| `createSession` | POST | `groupID`, `authorID`, `validUntil` (Unix-Sekunden, jetzt + `cookietime`) | `{"sessionID": "s.<16 Zeichen>"}` | jeder Seitenaufruf, direkt vor dem iframe | client.php 426–447, view.php 112 | ja (3.159) |
| `getReadOnlyID` | GET | `padID` | `{"readOnlyID": "r.<16 Zeichen>"}` | Gast ohne Schreibrecht, Zeitfenster geschlossen, Nicht-Gruppenmitglied ohne Lehrrechte | view.php 89–92 | ja (82) |
| `getHTML` | GET | `padID`, optional `rev` | `{"html": "<!DOCTYPE HTML><html><body>…</body></html>"}` | Kurs-Sicherung mit Nutzerdaten, je Hauptpad und je Gruppenpad | backup_stepslib 86, 98 | ja (4.920) |
| `getText` | GET | `padID`, optional `rev` | `{"text": "…"}` | Kurs-Sicherung, zusammen mit getHTML | backup_stepslib 87, 99 | ja (4.873) |
| `setText` | POST | `padID`, `text` | `null` | Kurs-Wiederherstellung, vor setHTML | restore_stepslib 140, 153 | nein (kein Restore im Zeitraum) |
| `setHTML` | POST | `padID`, `html` | `null` | Kurs-Wiederherstellung, direkt nach setText; der HTML-Stand gewinnt | restore_stepslib 141, 154 | nein |
| `deletePad` | POST | `padID` | `null` | Aktivität löschen (Haupt- und Gruppenpads), Kurs-Reset, verzögerte Löschung von Gruppenpads | lib.php 172; locallib 42; util.php 86, 90; task/delete_moodle_group_pad.php 60 | ja (22) |
| `deleteGroup` | POST | `groupID` | `null`; löscht alle Pads der Gruppe | Aktivität löschen | lib.php 173 | ja (22) |

Zahlen in Klammern: Aufrufe in den Logs 09.07. bis 23.10.2026, zwei Moodle-Quellen zusammen.

## Optional: im Client definiert, vom Plugin nicht aufgerufen

`createGroupIfNotExistsFor`, `listPads`, `listAllGroups`, `listPadsOfAuthor`, `getAuthorName`, `deleteSession`, `getSessionInfo`, `listSessionsOfGroup`, `listSessionsOfAuthor`, `createPad`, `getRevisionsCount`, `padUsersCount`, `getLastEdited`, `listAuthorsOfPad`, `setPublicStatus`, `getPublicStatus`, `setPassword`, `isPasswordProtected`, `padUsers`, `sendClientsMessage` (client.php 286–735). NextPad liefert für unbekannte Funktionen code 3. Sinnvoll für Betrieb und Migration sind trotzdem: `listAllPads`, `getRevisionsCount`, `getLastEdited`, `listAuthorsOfPad`, `getAttributePool` (für Exportwerkzeuge). Sie werden als zweite Stufe geführt.

## Kennungen

| Kennung | Format bei Etherpad | Regel für NextPad |
|---|---|---|
| groupID | `g.` + 16 Zeichen `[A-Za-z0-9]` | gleich; kryptografisch zufällig |
| padID (Gruppenpad) | `<groupID>$<padName>`; `padName` ist Freitext (Einstellung `padname`, dazu Moodle-Gruppen-ID) | gleich; `$` ist Trenner, der Teil vor dem ersten `$` ist die Gruppe (lib.php 169–170 zerlegt so) |
| authorID | `a.` + 16 Zeichen | gleich |
| sessionID | `s.` + 16 Zeichen | gleich |
| readOnlyID | `r.` + 16 Zeichen, eindeutig je Pad, stabil | gleich; Auflösung r → Pad serverseitig |

Pad-Kennungen aus der Migration werden unverändert übernommen, damit `etherpadlite.uri` in Moodle weiter stimmt.

## Session und Cookie

| Punkt | Verhalten | Fundstelle |
|---|---|---|
| Wer setzt das Cookie | Moodle, nicht der Pad-Server: `setcookie('sessionID', <sessionID>, <validUntil>, '/', <cookiedomain>, <secure>)` | client.php 437 |
| Name | `sessionID` | ebd. |
| Domain | Einstellung `cookiedomain`, z. B. `.hochschule.tld`; deshalb müssen Moodle und Pad-Server dieselbe Second-Level-Domain haben | ebd., README „Prerequirement“ |
| Pfad | `/` | ebd. |
| Ablauf | `validUntil` = jetzt + `cookietime` (Einstellung, in Sekunden; beobachtet 10.800 = 3 h) | client.php 427 |
| Secure | nur wenn `url` mit `https://` beginnt | client.php 435 |
| HttpOnly, SameSite | nicht gesetzt (PHP-Standard); Browser behandeln SameSite dann als Lax. Innerhalb derselben Site (gleiche eTLD+1) reicht das für den iframe | ebd. |
| Was NextPad tun muss | Beim Laden von `/p/<padID>` und beim WebSocket-Aufbau das Cookie `sessionID` lesen, Session nachschlagen, Ablauf prüfen, Gruppe der Session gegen Gruppe des Pads prüfen; Autor der Session liefert Name und Farbe. Mehrere `sessionID`-Werte (durch Kommas) tolerieren, Etherpad erlaubt das | Etherpad-Semantik, nachgebildet |
| Ohne gültige Session | Gruppenpads verweigern (HTTP 403 mit verständlicher Seite). Read-only-Kennungen `r.*` von Gruppenpads ebenfalls nur mit Session der Gruppe | Etherpad-Semantik |
| Eigene Cookies | NextPad setzt für den Editor nur ein eigenes, HttpOnly, Secure, SameSite=Lax gesetztes Sitzungscookie, falls nötig; keine Cookies auf der Hochschuldomain | Festlegung |

## iframe

| Punkt | Verhalten | Fundstelle |
|---|---|---|
| URL | `<url>/p/<padID>` zum Schreiben, `<url>/p/<readOnlyID>` zum Lesen. Keine Query-Parameter (kein `showChat`, kein `userName`, keine `userColor`, kein `showLineNumbers`) | view.php 89–94 |
| Einbettung | `<iframe class="etherpadiframe …" src="…">`, Vollbild über zweiten iframe in einem Modal; `minwidth` aus den Einstellungen als CSS-Mindestbreite | templates/iframe.mustache 32, modal.mustache 48, renderer.php 50 |
| Folgerung für NextPad | Der Editor muss ohne URL-Parameter auskommen: Name und Farbe aus der Session, kein Chat, Zeilennummern als Editor-Einstellung. Antwort-Header müssen Einbettung durch die Moodle-Hosts erlauben: `Content-Security-Policy: frame-ancestors 'self' https://<moodle-hosts>` statt eines pauschalen `X-Frame-Options` | Festlegung |
| Gruppenwahl | Moodle rendert sein Gruppenmenü über dem iframe; bei Gruppenmodus wird `padID` um die aktive Moodle-Gruppen-ID verlängert | view.php 74–83, 139 |

## Gäste und Rechte

| Fall | Was das Plugin tut | Fundstelle |
|---|---|---|
| Gast (Moodle-Gastkonto), Gäste erlaubt (`adminguests` global und `guestsallowed` je Instanz) | `createAuthor('Guest-<5 Ziffern>')`, Session, Schreibzugriff | view.php 103, lib.php 283–292 |
| Gast, Gäste nicht erlaubt | Read-only-Link | view.php 86–92 |
| Angemeldet, Gruppenmitglied oder keine Gruppen | Autor über `createAuthorIfNotExistsFor(userid, fullname)`, Schreibzugriff | view.php 105 |
| Angemeldet, nicht Gruppenmitglied, mit `mod/etherpadlite:addinstance` (Lehrende) | Gast-Autor mit Schreibzugriff | view.php 102–104 |
| Angemeldet, nicht Gruppenmitglied, ohne Lehrrechte | Read-only | view.php 86 |
| Zeitfenster (`timeopen`/`timeclose`) geschlossen, ohne Lehrrechte | Read-only | view.php 85–88 |
| Rechte | Einzige Capability `mod/etherpadlite:addinstance` (editingteacher, manager). Schreiben/Lesen folgt aus Kursmitgliedschaft, Gruppen und Zeitfenster; der Pad-Server kennt nur Sessions | db/access.php 29–36 |

## Datenbank in Moodle und Zuordnung

| Tabelle | Felder | Bedeutung |
|---|---|---|
| `etherpadlite` | `id`, `course`, `name`, `intro`, `introformat`, `timecreated`, `timemodified`, `timeopen`, `timeclose`, `uri`, `guestsallowed` | eine Zeile je Aktivität; `uri` = padID des Hauptpads (`g.<id>$<padname>`), daraus leitet das Plugin die groupID ab |
| `etherpadlite_mgroups` | `id`, `padid` (= `etherpadlite.id`), `groupid` (Moodle-Gruppe) | Gruppenpads: padID = `uri` + `groupid` |

Zuordnung: Kurs → `etherpadlite.course`; Aktivität → eine Etherpad-Gruppe; Moodle-Gruppe → Pad in derselben Etherpad-Gruppe; Nutzende → Autor über `authorMapper` = Moodle-User-ID; Gäste → je Aufruf ein neuer Autor. Quelle: db/install.xml.

Gruppen-Ereignisse: `group_created`, `grouping_group_assigned` legen Gruppenpads an; `group_deleted`, `grouping_group_unassigned`, Umschalten auf Gruppenmodus 0 löschen sie verzögert über einen Ad-hoc-Task (`deletemgrouppad`: 0 nie, 1 sofort, 2 nach 1 h, 3 nach 12 h, 4 nach 24 h). Quelle: observer/group.php, settings.php 170–188, task/delete_moodle_group_pad.php.

## Sicherung und Wiederherstellung von Kursen

| Schritt | Verhalten | Fundstelle |
|---|---|---|
| Sicherung ohne Nutzerdaten | nur Aktivitätsdaten (Name, Intro, Zeiten); kein Inhalt | backup_stepslib 60–70 |
| Sicherung mit Nutzerdaten | Element `content` mit `html` (getHTML), `text` (getText) des Hauptpads und `grouppaddata` (PHP-`serialize` eines Arrays Moodle-Gruppen-ID → {html, text}) | backup_stepslib 72–104 |
| Wiederherstellung | neue Gruppe (`createGroup`), neues Hauptpad, je Moodle-Gruppe ein Gruppenpad; dann `setText` und `setHTML` mit den gesicherten Ständen (Gruppen über das Moodle-Gruppen-Mapping) | restore_stepslib 60–160 |
| Folgerung für NextPad | `setHTML` muss den HTML-Dialekt verstehen, den Etherpad in alte Sicherungen geschrieben hat (Absätze als `<br>`-getrennte Zeilen, `<strong>`, `<em>`, `<u>`, `<s>`, `<ul>/<ol>/<li>`, Überschriften `<h1>`–`<h6>` aus ep_headings2, `<span>` mit Schriftgröße und Farbe aus ep_font_size/ep_font_color). Der Export aller Pads liefert den Testkorpus | Festlegung |
| Kurs-Reset | Pads löschen und leer neu anlegen (`deletePad`, `createGroupPad` mit `text=''`) | util.php 70–98 |

## Was NextPad bewusst nicht nachbildet

- Chat, Chat-API, `sendClientsMessage`.
- Passwortgeschützte Pads, öffentliche Statusänderung (`setPassword`, `setPublicStatus`): das Plugin nutzt sie nicht.
- Import/Export über `/p/<id>/import` und `/export/<format>` außer HTML und Text (Export als HTML und Text bleibt, für Lehrende nützlich).
- Die Admin-Oberfläche von Etherpad. Betrieb läuft über Konfigurationsdatei, Logs und eine kleine Statusseite ohne Login-Formular.

## Prüfverfahren

1. Vertragstest je Pflichtfunktion gegen diese Tabelle (Request-Form, Antwortform, Fehlercodes).
2. Moodle-Testinstanz mit mod_etherpadlite gegen NextPad: Aktivität anlegen, Gruppenmodus, Gast, Zeitfenster, Sicherung mit Nutzerdaten, Wiederherstellung in anderen Kurs, Kurs-Reset, Aktivität löschen.
3. Alle 764 exportierten HTML-Stände durch `setHTML` → `getHTML` schleifen und Abweichungen zählen (Migrationsprobe).
