# NextPad server

Deutsche Fassung: [README.de.md](README.de.md)

Rust (Axum, yrs). Reproduces the Etherpad API for mod_etherpadlite and runs Yjs documents with WebSocket sync behind it.

## Build and run

    cd server
    CARGO_TARGET_DIR=~/.cache/cargo-target/nextpad cargo build --release

The editor bundle is prebuilt under `static/` and embedded at compile time (`include_str!`). To rebuild: `bash ../web/build.sh`, then `cargo build`.

Configuration through environment variables:

| Variable | Default | Meaning |
|---|---|---|
| `NEXTPAD_BIND` | `127.0.0.1:5090` | Address and port |
| `NEXTPAD_DATA` | `./data` | Data directory (SQLite `nextpad.sqlite`) |
| `NEXTPAD_API_KEYS_FILE` | `<data>/api-keys.txt` | One key per line, at least 32 characters, optionally followed by the base URL of the matching Moodle (`<key> https://moodle.example.org`): the editor then shows Moodle profile pictures (`user/pix.php/<id>/f1.jpg`, loaded by the browser with its Moodle session) and keeps equal user IDs from different Moodles apart; `#` starts a comment |
| `NEXTPAD_API_ALLOWED` | `127.0.0.1,::1` | Source addresses allowed for `/api/*`, addresses or networks (CIDR), comma-separated, `*` for all |
| `NEXTPAD_TRUST_PROXY` | `false` | `true`: source address from `X-Real-IP` or the last `X-Forwarded-For` entry, but only from addresses in `NEXTPAD_TRUSTED_PROXIES` |
| `NEXTPAD_TRUSTED_PROXIES` | `127.0.0.1,::1` | Reverse proxies whose headers count (CIDR allowed) |
| `NEXTPAD_ALLOWED_ORIGINS` | empty | Additional allowed origins for WebSocket connections; the own host is always allowed |
| `NEXTPAD_FRAME_ANCESTORS` | `'self'` | Value for `Content-Security-Policy: frame-ancestors` |
| `NEXTPAD_BASE_PATH` | empty | Path prefix behind the reverse proxy, e.g. `/np` (only for links in the page; the proxy strips the prefix) |
| `NEXTPAD_API_VERSION` | `1.2.15` | Response of `GET /api` |
| `NEXTPAD_ALLOW_FREE_PADS` | `false` | Free pads: start page with "New pad" and pads by name, guests write without Moodle (cookie identity), `createPad` through the API |
| `NEXTPAD_ADDONS` | all | Enabled add-ons, comma-separated from `ideen`, `quiz`, `zeichenbrett`; empty = none, unknown name = start error. Disabled: no tile, no routes, no help, no Moodle type; existing pads show a notice, their data stays and goes with the pad when it is deleted |
| `NEXTPAD_FREE_PADS_PER_HOUR` | `20` | At most this many new free pads per source address and hour |
| `NEXTPAD_AUTHOR_COOKIE` | `nextpad_author` | Name of the guest cookie |
| `NEXTPAD_THEME` | `colibris` | Look: `colibris` (like Etherpad) or `hsd` (corporate design of Hochschule Düsseldorf) |
| `NEXTPAD_THEME_DIR` | empty | Default directory with `logo.png`, `background.jpg` and `font.ttf` for the look; brand and house font are deliberately not in the repository |
| `NEXTPAD_BACKGROUND_CREDIT` | empty | Image credit for `background.jpg` in the theme directory (background of the HSD start page), e.g. `Photo: Jula2812, CC BY-SA 4.0, Wikimedia Commons`; required for images under a CC licence |
| `NEXTPAD_BACKGROUND_CREDIT_URL` | empty | Link of the image credit (file page on Commons) |
| `NEXTPAD_HISTORY_DAYS` | `30` | For this long the timeline shows every single step; older parts only as daily versions (`nextpad compact`) |
| `NEXTPAD_SITE_NAME` | empty | Operator shown in the footer of the start page (e.g. `ZWEK der Hochschule Düsseldorf`) |
| `NEXTPAD_ADMINS` | empty | Email addresses from Moodle (comma-separated) allowed to change the appearance; Moodle administrators always are |
| `NEXTPAD_COOKIE_NAME` | `sessionID` | Name of the session cookie set by Moodle |
| `NEXTPAD_PUBLIC_URL` | empty | Public address including the base path, e.g. `https://host/np`; required for LTI (redirect address) and for links in emails |
| `NEXTPAD_LTI_FILE` | `<data>/lti-platforms.json` | Moodle instances for sign-in through LTI 1.3; without the file: no sign-in |
| `NEXTPAD_LOGIN_COOKIE` | `nextpad_login` | Name of the sign-in cookie (14 days) |
| `NEXTPAD_FOOTER_LINKS` | empty | Links in the footer of the start page, `Label|https://…;Label|https://…` |
| `NEXTPAD_RETENTION` | `off` | Retention of free pads: `off`, `dry-run` (log only), `on` |
| `NEXTPAD_RETENTION_DAYS` | `365` | Period without activity in days (at least 30) |
| `NEXTPAD_NOTICE_DAYS` | `14` | Email notice this many days before |
| `NEXTPAD_SMTP_URL` / `NEXTPAD_SMTP_URL_FILE` | empty | Outgoing mail in lettre format, e.g. `smtp://127.0.0.1:25` or `smtps://user:pass@host:465`; with credentials better as a file (systemd `LoadCredential`) |
| `NEXTPAD_MAIL_FROM` | empty | Sender of the notices |
| `RUST_LOG` | `info` | Log filter |

### LTI platform file

```json
[{
  "name": "test-moodle",
  "label": "Sign in with Moodle",
  "issuer": "https://moodle.example.org",
  "client_id": "<from the tool configuration>",
  "deployment_ids": ["<id of the tool>"],
  "auth_url": "https://moodle.example.org/mod/lti/auth.php",
  "jwks_url": "https://moodle.example.org/mod/lti/certs.php",
  "start_url": "https://moodle.example.org/mod/lti/launch.php?id=<cmid>"
}]
```

Use `jwks_file` instead of `jwks_url` when the server cannot reach the Moodle address (for example behind an SSO barrier). Then renew the file after a key change in Moodle. `label` is the text of the sign-in button and is shown as written, in every language.

In Moodle (Site administration › Plugins › External tool › Manage tools), LTI 1.3:

| Field | Value |
|---|---|
| Tool URL | `<public>/lti/launch` |
| Initiate login URL | `<public>/lti/login` |
| Redirection URI(s) | `<public>/lti/launch` |
| Public key type | Keyset URL, `<public>/lti/jwks` (empty, NextPad signs nothing) |
| Share launcher's name with tool | Always |
| Share launcher's email with tool | Always (for the email before deletion) |
| Default launch container | New window |

Then create an activity with this tool on the Moodle front page. Its address `mod/lti/launch.php?id=<cmid>` is `start_url`.

### Pad, idea collection or quiz as a course activity

In the course, create an "External tool" activity with this tool and enter what should be created under "Show more" › "Custom parameters":

- `typ=pad`: a shared text pad (like an Etherpad group pad). Alternatively tool URL `<public>/lti/pad`.
- `typ=ideen`: an idea collection (details below).
- `typ=quiz`: a quiz grid; teachers lead, the name of the activity becomes the title.

At every launch NextPad remembers the address of the activity (`<moodle>/mod/lti/view.php?l=<resource_link.id>`, for the QR code) and the course page (`<moodle>/course/view.php?id=<context.id>`, for "Back to the course"). Both only if Moodle sends numbers.

### Idea collection as a course activity

In the course, create an "External tool" activity with this tool and enter `typ=ideen` under "Show more" › "Custom parameters". The name of the activity becomes the question of the collection. Alternatively, a separate tool entry with the tool URL `<public>/lti/ideen` points straight to the collection.

Each activity gets its own group with one collection. Only those who launch the activity from Moodle get in: NextPad then creates a session as with Etherpad group pads (cookie `NEXTPAD_COOKIE_NAME`, several sessions separated by commas, `SameSite=None` so the activity also works embedded). The roles Instructor, TeachingAssistant, ContentDeveloper and Administrator moderate. A course collection cannot be deleted from within NextPad and is not subject to retention.

### Check retention

```sh
NEXTPAD_RETENTION=dry-run nextpad retention-run
```

shows which pads would be announced or deleted, without doing anything.

### Set the appearance

With the `hsd` look, admins set the name and subtitle in the header, the institution (name of the logo), the logo, the start page background with its credit and the colours under `/admin/erscheinungsbild`. An admin is whoever signs in through Moodle (LTI) and belongs to the administration there (role `institution/person#Administrator` or `system/person#Administrator`) or is listed with their email address in `NEXTPAD_ADMINS`. Signed-in admins see the "Appearance" button on the start page.

- Colours: one accent colour (header line, focus ring) and three house colours. NextPad derives lines and tints for light and dark (lines darkened to at least 4.5:1, tints as a share on white or dark grey); they can also be set by hand. The page reports contrasts below WCAG 2.2.
- Logo: PNG, JPEG, WebP or SVG up to 2 MB; SVG only without scripts and event attributes, served with `sandbox`. With the institution's logo, the NextPad mark in the header is dropped.
- Background image: JPEG, PNG or WebP up to 6 MB.
- Without a setting, the server default applies (`NEXTPAD_THEME_DIR`, `NEXTPAD_BACKGROUND_CREDIT`). "Reset everything to the default" restores it; uploaded images stay and can be chosen again.
- Stored in the database (table `settings`), images under `<NEXTPAD_DATA>/erscheinungsbild/`.

### Language

All pages, messages, help, exports and the email before deletion are available in German and English, including the appearance settings page; a subtitle left empty there uses the default in the page’s language, an entered one is shown as written. The text in pads is not translated. The email before deletion uses the language the owner last chose while signed in through Moodle (column `authors.sprache`, set through `/sprache/…`); without a choice it contains both languages in one email, German first, and the PDF is then German. The idea collection's CSV is separated by semicolons in German and by commas in English, as spreadsheet programs in each language expect; the table of contents in stored versions and in the Etherpad API stays German. The language of a request, in this order:

1. Own choice: cookie `nextpad-lang`, set through the "DE | EN" switch (header of the pages, editor toolbar; page frames with `wahl: None` do not show it) or `GET /sprache/de` and `/sprache/en` (`?zurueck=<path>` returns to a path on this server, other targets lead to the start page);
2. Language from Moodle: cookie `nextpad-lang-moodle`, refreshed from `launch_presentation.locale` at every LTI launch; an unsupported language deletes it;
3. Browser language (`Accept-Language`, by weight);
4. German.

Both cookies contain only the language code, last a year and are `HttpOnly; Secure; SameSite=None` (otherwise the choice would not apply in the activity embedded in Moodle). Texts are stored by key in `src/texte/de.rs` and `en.rs`, add-ons carry their own (`Addon::texte`); a unit test checks that both languages have the same keys and placeholders. Keys ending in `_html` contain markup and are not escaped. The help page is `src/texte/hilfe.de.html` and `hilfe.en.html`. Scripts in the browser get their texts from the page: `texte::fuer_skript(sp, "startjs.")` produces JSON for a `<script type="application/json">` block (this is how `start.js` reads the block `#start-texte`). The editor gets `texte` and `lang` in the configuration block `#nextpad-config` (`texte::texte_mit(sp, "editor.")`); `web/src/texte.js` provides `tx()` and the date format for all editor modules.

## Endpoints

`GET /api` (version), `GET|POST /api/1.2*/<function>` (see `docs/api-spezifikation.md`), `GET /p/<padID>` and `/p/<readOnlyID>` (editor), `GET /ws/<id>` (Yjs sync), `GET /p/<id>/export/html|txt|md|pdf|print`, `GET /p/<id>/snapshots`, `GET /p/<id>/snapshots/<ts>`, `GET /status`, `GET /static/editor.js|css|print.js`, `GET /sprache/de|en`; with free pads also `GET /` (start page), `GET /new`, `GET /open?padname=`, `POST /me` (own name), `GET /hilfe`, `GET /new?vorlage=gruppenarbeit|protokoll`, `GET /new?typ=zeichenbrett|ideen|quiz`, `GET|POST /p/<id>/ideen`, `GET /p/<id>/ideen/live` (server-sent events), `POST /p/<id>/ideen/frage`, `POST /p/<id>/ideen/<n>/stimme`, `POST /p/<id>/ideen/<n>/moderation`, `GET /p/<id>/leinwand`, `POST /p/<id>/extend` and `POST /p/<id>/delete` (owner only), `GET|POST /verlaengern` (link from the email); with LTI `GET|POST /lti/login`, `POST /lti/launch` (also launches a course activity), `GET /lti/jwks`, `POST /logout`, `GET|POST /admin/erscheinungsbild` and `POST /admin/erscheinungsbild/vorgabe` (admins only), `GET /theme/logo.png` and `/theme/background.jpg` (configured image in its own format).

## Structure

`config.rs` configuration · `ids.rs` identifiers · `store.rs` SQLite (groups, pads, authors, sessions, update log, snapshots) · `docs.rs` loaded documents, broadcast, persistence, snapshots · `sync.rs` WebSocket with `yrs::sync` · `api.rs` Etherpad API · `render.rs` Yjs ↔ HTML/text · `web.rs` editor page, access checks, export, status · `look.rs` appearance and its settings page · `live.rs` live channel (server-sent events) for add-ons · `qr.rs` QR codes · `texte/` texts in German and English.

Add-ons live in `addons/<name>/` and describe themselves with an `Addon` constant (`addons/mod.rs`): pad kind, tables (`daten.rs`, passed as `Zusatz` to `Store::open`), routes, page, export, tile, help, Moodle type, texts. The core never asks for a single pad type. `addons/ideen` idea collection · `addons/quiz` quiz grid with buzzer and buzzer measurement (`messung.rs`) · `addons/zeichenbrett` Excalidraw page (the bundle is built by `web/build.sh` into `static/board`). Scripts and styles of the add-ons live in `addons/<name>/static/` and are still served under `/static/…`.

## Pitfalls (found in the proof of concept)

- By default yrs counts text positions in bytes, the browser (Yjs) in UTF-16. Without `OffsetKind::Utf16` everything breaks after the first umlaut (`docs.rs`).
- `Text::insert` without attributes inherits the formatting of the preceding text; therefore always use `insert_with_attributes`, even with an empty attribute set (`render.rs`).
- The multipart boundary is case-sensitive; do not lowercase `Content-Type` before reading it (`api.rs`).

## Security limits (review 2026-10-24)

- WebSocket: at most 1 MiB per message, 2 MiB of updates per connection and minute, origin check, ping every 30 s, disconnect after 90 s of silence, disconnect when falling behind (the client syncs again).
- API bodies at most 16 MiB (setHTML when restoring a course).
- Read-only mode: the page names only the r. identifier, the WebSocket discards updates.
- Guests: the cookie holds a secret token, not the public author ID; only guests can change their name.
- Exports and versions with their own CSP (`default-src 'none'`) and `nosniff`; colour, font size and alignment only from allow-lists.
- Deleted pads write neither updates nor snapshots (checked in the database), deletion in one transaction.
- Rendering with a depth limit (48) against stack overflow from nested documents.
