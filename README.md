<h1><img src="docs/bilder/nextpad-logo.svg" alt="NextPad" width="252" height="48"></h1>

Deutsche Fassung: [README.de.md](README.de.md)

A slim pad server in [Rust](https://www.rust-lang.org) that reproduces the
[Etherpad](https://etherpad.org) HTTP API, so Moodle keeps working through the unchanged
`mod_etherpadlite` plugin. Underneath sits a modern stack: collaborative writing
over [Yjs](https://yjs.dev) (CRDT), the [TipTap](https://tiptap.dev) editor, and a
timeline built from snapshots. No chat.

## Overview

The ZWEK runs an Etherpad server for the university's Moodle platforms. Moodle attaches
pads through the plugin `mod_etherpadlite` (Humboldt-Universität zu Berlin) as a course
activity. With the current resources, maintaining Etherpad and its plugins is barely
sustainable: the plugin landscape slows every upgrade down, and the dependency tree counts
more than a thousand npm packages.

The actual requirement is small: headings, font size, bold, italic, underline,
strikethrough, text and author colors, a timeline, and Markdown. No chat. NextPad closes
that gap: a dedicated server that answers exactly the API calls the Moodle plugin makes,
and behind it runs a modern editor.

Etherpad stays in place as a fallback until the new server has proven itself in operation.

## Why Rust

A pad server sits openly on the network and processes input from everyone who opens a pad.
Besides the effort of running it, what matters most is how much attack surface it offers.

- **Memory safety.** Rust rules out buffer overflows, use-after-free and data races at
  compile time. These classes of bugs are behind a large share of security holes in server
  software. NextPad's own code contains no `unsafe`; the compiler enforces this
  (`#![forbid(unsafe_code)]`).
- **Small runtime.** The server runs a single program, without Node.js, without a package
  manager and without plugins that load third-party code at runtime. In Etherpad, plugins
  run with full rights inside the server process. The editor bundle for the browser is
  built with npm, but that code only runs in the browser, not on the server.
- **Pinned and audited dependencies.** `Cargo.lock` pins every crate to an exact version;
  a large part of them belongs to PDF typesetting with Typst. CI checks them on every push
  with `cargo audit` against the RustSec database. Reported issues that demonstrably cannot
  be reached from NextPad are listed with a reason in `server/.cargo/audit.toml`; any new
  report fails the build.
- **Input stays data.** Pad content only enters HTML, Markdown and PDF typesetting as
  strings, never as markup or code. Sign-in through Moodle accepts RS256-signed tokens only.
- **Little to operate.** An update means replacing one file. Under load the server needs
  around 50 MB of memory.

Rust does not protect against logic errors or misconfiguration. Reverse proxy, API keys and
the list of allowed addresses remain the operator's job.

## Features

- Drop-in Etherpad API for `mod_etherpadlite`: 36 contract checks green, round-trip
  identical.
- Collaborative editing over Yjs/yrs (CRDT); no operational transforms.
- Timeline from snapshots, like Etherpad's timeslider: scrub, play, time and author per step.
- Export as HTML, Markdown, plain text, and PDF (typeset with [Typst](https://typst.app)).
- Tables (Markdown paste works too), task lists, thumbs-up/thumbs-down ratings inside the
  text, table of contents as a sidebar and as a block in the text.
- Whiteboard via Excalidraw on the same Yjs server.
- Idea collection modelled on [liveask](https://github.com/liveask/liveask): post ideas and questions without a name, vote, live for everyone, with a projector view and QR code. The server counts one vote per person, only whoever created the collection can moderate. In a Moodle course it runs as an "External tool" activity (LTI 1.3), open only to course members and moderated by the teachers.
- Moodle sign-in through LTI 1.3 (names and avatars).
- Guest pads with ownership and retention rules.
- Interface, help, exports and emails in German and English, switchable by each person.
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
mkdir -p ~/.cache/nextpad-data
head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 48 > ~/.cache/nextpad-data/api-keys.txt
bash web/build.sh            # editor and whiteboard bundle (needs Node 22 and pnpm)
cd server
CARGO_TARGET_DIR=~/.cache/cargo-target/nextpad cargo build
NEXTPAD_DATA=~/.cache/nextpad-data \
NEXTPAD_FRAME_ANCESTORS="'self' http://127.0.0.1:5090" \
~/.cache/cargo-target/nextpad/debug/nextpad
```

Then create a group, pad, author and session through the API (examples in
`docs/api-spezifikation.md`) and open `http://127.0.0.1:5090/p/<padID>` with a
`sessionID` cookie set.

## Project background

Idea, concept and development: Christian-Maximilian Steier.

This software was developed at Hochschule Düsseldorf – University of Applied Sciences
(HSD) by ZWEK – Centre for Training and Competence Development, as a successor to the
Etherpad service behind the Moodle platforms of HSD.

## Citation

Machine-readable citation metadata lives in [`CITATION.cff`](CITATION.cff).

## License

Released under the [European Union Public Licence 1.2](LICENSE) (EUPL-1.2; German legal
text, English translation in [`LICENSE.en`](LICENSE.en)).
