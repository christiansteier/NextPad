# NextPad

Deutsche Fassung: [README.de.md](README.de.md)

A slim pad server in [Rust](https://www.rust-lang.org) that reproduces the
[Etherpad](https://etherpad.org) HTTP API, so Moodle keeps working through the unchanged
`mod_etherpadlite` plugin. Underneath sits a modern stack: collaborative writing
over [Yjs](https://yjs.dev) (CRDT), the [TipTap](https://tiptap.dev) editor, and a
timeline built from snapshots. No chat.

> **Naming.** The name for users is **NextPad** (address `/np`). In code, repositories
> and packages the project is still called `cooppad`.

## Overview

The ZWEK runs an Etherpad server for the university's Moodle platforms. Moodle attaches
pads through the plugin `mod_etherpadlite` (Humboldt-Universität zu Berlin) as a course
activity. Maintaining Etherpad and its plugins has become expensive: the plugin landscape
slows every upgrade down, and the dependency tree counts more than a thousand npm packages.

The actual requirement is small: headings, font size, bold, italic, underline,
strikethrough, text and author colors, a timeline, and Markdown. No chat. NextPad closes
that gap: a dedicated server that answers exactly the API calls the Moodle plugin makes,
and behind it runs a modern editor.

Etherpad stays in place as a fallback until the new server has proven itself in operation.

## Features

- Drop-in Etherpad API for `mod_etherpadlite`: 36 contract checks green, round-trip
  identical.
- Collaborative editing over Yjs/yrs (CRDT); no operational transforms.
- Timeline from snapshots, like Etherpad's timeslider: scrub, play, time and author per step.
- Export as HTML, Markdown, plain text, and PDF (typeset with [Typst](https://typst.app)).
- Whiteboard via Excalidraw on the same Yjs server.
- Moodle sign-in through LTI 1.3 (names and avatars).
- Guest pads with ownership and retention rules.
- One Rust binary, SQLite, roughly 50 MB RAM under load.

## Status

Pilot in production at a German university since 2026-10. Load test: one pad, 100 people,
400 inputs/s, median around 50 ms, p99 under 80 ms. The load tests are in `web/test/`.

## Documentation

| File | Content |
|---|---|
| `docs/api-spezifikation.md` | The Etherpad API to reproduce, derived from the plugin source |
| `docs/docker.md` | Running NextPad as a container |
| `server/README.md` | Configuration (environment variables), commands |
| `web/README.md` | Editor bundle and browser tests |
| `packaging/nextpad_ynh/` | YunoHost app |

Most documentation is German. The code is the source of truth.

## Try it locally

```bash
mkdir -p ~/.cache/cooppad-data
head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 48 > ~/.cache/cooppad-data/api-keys.txt
cd server
CARGO_TARGET_DIR=~/.cache/cargo-target/cooppad cargo build
COOPPAD_DATA=~/.cache/cooppad-data \
COOPPAD_FRAME_ANCESTORS="'self' http://127.0.0.1:5090" \
~/.cache/cargo-target/cooppad/debug/cooppad
```

Then create a group, pad, author and session through the API (examples in
`docs/api-spezifikation.md`) and open `http://127.0.0.1:5090/p/<padID>` with a
`sessionID` cookie set.

## Project background

This software was developed at Hochschule Düsseldorf – University of Applied Sciences
(HSD) by ZWEK – Centre for Training and Competence Development, as a successor to the
Etherpad service behind the Moodle platforms of HSD.

## Citation

Machine-readable citation metadata lives in [`CITATION.cff`](CITATION.cff).

## License

Released under the [European Union Public Licence 1.2](LICENSE) (EUPL-1.2; German legal
text, English translation in [`LICENSE.en`](LICENSE.en)).
