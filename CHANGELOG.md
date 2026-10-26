# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-10-26

### Added

- Idea collection as a Moodle activity: an "External tool" (LTI 1.3) with the custom parameter
  `typ=ideen` (or tool URL `…/lti/ideen`) opens a collection of its own for each activity.
  Only people who start it from Moodle get in (group session as with Etherpad group pads,
  works embedded too); teachers moderate, the activity name becomes the question
  (`web/test/ltitest.mjs`).

### Changed

- Idea collection: empty-state text fits the situation (course, everything done).

## [0.4.0] - 2026-10-26

### Added

- Idea collection (start page tile "Ideensammlung", modelled on liveask): everyone posts ideas
  or questions without a name and votes; one vote per person and post, checked by the server.
  Whoever created the collection sets the question, marks posts as done or hides them.
  Changes reach all open pages live (Server-Sent Events). Read-only link for the projector,
  export as Markdown, text or CSV (`web/test/ideentest.mjs`, `web/test/ideen-a11y.mjs`).

### Changed

- Start page: the tile "Leeres Pad" is gone (opening a new name already creates a pad); the
  text template "Ideensammlung" is replaced by the idea collection, old links lead there.
  Desktop shows three tiles per row.

## [0.3.3] - 2026-10-26

### Changed

- Start page (HSD theme, desktop): the entry panel is as wide as the panel below it.

## [0.3.2] - 2026-10-26

### Changed

- Start page (HSD theme, desktop): the photo now reaches down to the footer; sign-in, recently
  opened pads and the note sit on their own translucent panel, aligned with the entry panel.

### Fixed

- A hidden "recently opened" list no longer leaves an empty gap above the sign-in.

## [0.3.1] - 2026-10-26

### Changed

- Only the name `nextpad` remains: settings must use the prefix `NEXTPAD_*`, the database
  file is `nextpad.sqlite`, cookies are `nextpad_*`.

### Fixed

- Opening a pad no longer clears the list of recently opened pads (introduced in 0.3.0).

## [0.3.0] - 2026-10-26

### Changed

- Technical name is now `nextpad` throughout: program and crate, settings `NEXTPAD_*`,
  database file `nextpad.sqlite`, cookies `nextpad_login`, `nextpad_author`, `nextpad_lti`,
  status name, browser object `window.nextpad`.

### Added

- Help page: section "Über NextPad" with author and link to the source code.
- READMEs: section on why Rust, including attack surface.
- CI runs `cargo audit`; advisories that cannot be reached from NextPad are documented in
  `server/.cargo/audit.toml`.
- `#![forbid(unsafe_code)]` for the server.

## [0.2.12] - 2026-10-26

### Added

- CI workflow for GitHub (`.github/workflows/ci.yml`): build, tests and clippy of the server.

### Changed

- Example addresses in the documentation and the configuration panel use the documentation
  range `192.0.2.0/24`; the LTI example platform is called `test-moodle`.
- `web/test/markdowntest.mjs` creates its own group and pad and no longer needs a fixture file.
- The operator name (`NEXTPAD_SITE_NAME`) is empty by default; without it the footer shows
  only "NextPad". Previously the operator was hard-coded in the plain start and info pages.
- Binaries for the YunoHost app come from the GitHub releases.

### Fixed

- Container image label states the licence correctly as EUPL-1.2 (was Apache-2.0).

## [0.2.11] - 2026-10-25

### Added

- Dark mode in the pad: toggle in the toolbar (◐), the choice is kept in the browser; without
  a choice the pad follows the device setting. Works for both themes; text on author colours
  stays dark (`web/test/darktest.mjs`, axe colour contrast clean).

## [0.2.10] - 2026-10-25

### Changed

- "Leeres Pad oder Vorlage" is open by default.
- Signing out is easier to find: a proper button on the start page ("Über Moodle angemeldet
  als …"), a sign-out button in the pad under "Teilnehmende", and `GET /logout` shows a short
  page with a sign-out button instead of 405 (`web/test/logouttest.mjs`).

## [0.2.9] - 2026-10-25

### Changed

- "Neues Pad" moved into the entry panel as a collapsible section ("Leeres Pad oder Vorlage",
  native `details`/`summary`, works without JavaScript); closed by default, so the start page
  shows the main task first. Compact tiles inside, two columns from 480 px.

## [0.2.8] - 2026-10-25

### Changed

- Larger photo: 200 px on phones, 300 px on tablets; on wide screens the entry panel sits fully
  on the photo, which grows with the panel. Entry panel slightly translucent over the photo
  (88 %). Less black: only the main action stays black, "Leeres Pad" and the Moodle sign-in
  get thin grey borders.

## [0.2.7] - 2026-10-25

### Changed

- Start page for phones first: slim photo band (112 to 200 px) with the entry panel overlapping
  it, so the main action sits in the first screen even at 320 × 640. One button "Öffnen oder
  anlegen" with a sentence on what happens, the access note ("Kein Passwortschutz …") right
  under the field. Title in Arial Bold, sentence case. Sign-in via Moodle as a secondary button
  above the templates. "Neues Pad" as a list with icon, name, short description and chevron on
  phones, five cards on wide screens. Field and button stacked on phones, smaller header logo.
  Type sizes in rem so they follow the browser's font size.

## [0.2.6] - 2026-10-25

### Changed

- Start page reduced: a photo band with a single opaque panel (title, pad name, open) as the
  one main task; below on a calm surface a row of five tiles to create a pad (names only),
  the recent list without frame, sign-in via Moodle as one sentence and a short note above
  the footer instead of the "Gut zu wissen" box. Body text 17 px, hints about half as long,
  header aligned with the content column.

## [0.2.5] - 2026-10-25

### Changed

- Split start page from 0.2.4 withdrawn: header, photo behind the panels and footer are back
  as in 0.2.3. With a background photo the panels are now slightly translucent (88 % opacity);
  secondary text keeps at least 6.3:1 contrast in the worst case.

## [0.2.4] - 2026-10-25

### Changed

- Start page on wide screens (1000 px and up) with a background photo: split layout like the
  Moodle login page (Boost theme, e.g. ZBIW). The photo fills the left side on its own, the
  controls sit on a calm surface on the right, header and footer belong to the right column.
  Narrower screens keep the previous layout.

## [0.2.3] - 2026-10-25

### Added

- Task lists you can tick off: Markdown `- [ ]` / `- [x]` while typing and when pasting,
  toolbar button, sync of the checked state; exported to HTML (checkboxes), Markdown, text
  and PDF; kept through `getHTML` → `setHTML` (Moodle course backup and restore)
  (`web/test/tasklist.mjs`).

### Changed

- Incoming remote changes are applied at most once per frame, with a longer interval when
  applying gets expensive (up to 250 ms). With 100 people typing into the same paragraph,
  remote input now appears after about 170 ms instead of several seconds
  (`web/src/yjs-batch.js`, also used by the whiteboard).

## [0.2.2] - 2026-10-25

### Added

- Load test with 100 people on one pad (400 inputs/s): median around 50 ms, p99 under
  80 ms; footer aligned (`web/test/belastung.mjs`, `belastung-leicht.mjs`,
  `browser-unter-last.mjs`).

## [0.2.1] - 2026-10-25

### Added

- Feedback on tap, side safe areas, hint box, arrows in "recently opened"; package source
  Gitea.

## [0.2.0] - 2026-10-24

### Added

- Renamed to NextPad, address `/np`; write thread for SQLite and update-log compaction;
  Typst cache bounded.
- Whiteboard prototype via Excalidraw; profile pictures from Moodle; PDF typesetting with
  Typst; free pads with ownership and retention; LTI 1.3 sign-in; switchable appearance.

### Changed

- Pilot switched to a YunoHost app (`packaging/nextpad_ynh`).
