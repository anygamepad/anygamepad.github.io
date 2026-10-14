# 0007 — Local-first device reports

- Status: proposed (2026-10-14). Points 2, 3, 4 and 6 apply the owner's
  decisions of the same day (see Context).
- Date: 2026-10-14
- Roadmap: P1-P6, W6, W7, A3, M2, M5 (re-plan of ROADMAP/2026-10-14/ in
  place)

## Context

[ADR 0006](0006-server-stack.md) and the first roadmap send every device
report straight to an ingestion server. That puts the server, its hosting
and its GDPR duties (IP addresses, retention) on the critical path to the
web launch: P1 -> P2 -> P5 -> W7 -> M2, with one contributor.

The owner decided on 2026-10-14:

- The desktop binaries write device reports to files, and the web app keeps
  them in the browser's IndexedDB; both synchronize with a database later.
- The web launch (M2) needs local reports that people can share, not the
  server: sync (P2, P5) moves after M2 and stops blocking the desktop
  release (M4).
- Until sync exists, reports people choose to share arrive as GitHub issues
  in this repository.

## Decision

1. **One document.** A report is one JSON document of the
   `anygamepad-report` format (P1, `data/reports/`): a file, an IndexedDB
   entry, an issue body and a sync upload carry the same document, built,
   scrubbed and validated by the same Rust code. Its compact JSON stays
   under 60,000 bytes so it fits in an issue body (65,536 characters).
   Reports carry the day they were made (UTC), not a time, and no ID: a
   store keys its reports itself, and the server deduplicates by content.
2. **Desktop: files.** The desktop app (A3) writes one indented JSON file
   per report to `reports/` in its data directory:
   `$XDG_DATA_HOME/anygamepad/` (default `~/.local/share/anygamepad/`) on
   Linux, `%LOCALAPPDATA%\anygamepad\` on Windows,
   `~/Library/Application Support/anygamepad/` on macOS. File names are
   `<fingerprint of the first view>-<YYYY-MM-DD>-<n>.json`; nothing else
   identifies the person. The app opens the folder on request.
3. **Web: IndexedDB.** The web tester (W6) keeps reports in an IndexedDB
   database `anygamepad`, object store `reports` (auto-increment key).
   Next to each report it stores local metadata that is never shared: when
   it was saved, and whether it was copied for an issue. The tester lists,
   previews, downloads (the same JSON) and deletes reports, and asks the
   browser to keep its storage (`navigator.storage.persist()`). Nothing
   leaves the browser by itself.
4. **Sharing: a GitHub issue form.** `.github/ISSUE_TEMPLATE/device-report.yml`
   (W6) has a JSON field for the report (compact form) and a required
   checkbox confirming the person reviewed it. The tester's "Share" copies
   the report and opens the form with the title prefilled (issue forms take
   field values from the URL, but a report is too long for a URL). The desktop
   app does the same with a file (A3). Issues are labelled `device-report`;
   curation (P4) reads them with the GitHub API.
5. **Consent.** Saving a report locally is the person's action (a button),
   never automatic. Sharing shows the whole report first (W6, A3) and says
   that an issue is public and tied to their GitHub account; the report
   itself holds no personal data (P1). P3 writes the texts and the
   retention rules for issues and, later, the server: they are in
   [`docs/privacy/`](../privacy/README.md), and the public policy is
   <https://anygamepad.github.io/privacy/> (2026-10-14).
6. **Sync later.** The ingestion server of ADR 0006 becomes a sync target
   after M2 (P2, P5): clients upload the reports the person chooses (P6),
   the server validates them with the same crate, stores the re-serialized
   document (so unknown data never passes through) and deduplicates by a
   SHA-256 of its compact JSON. ADR 0006's stack (axum, PostgreSQL, CORS
   limited to the site) is unchanged; only its timing changes.

## Alternatives

- **Server first** (ADR 0006's timing): the data loop starts with the
  launch, but M2 waits for P2 and P5, and hosting, backups and IP address
  handling start before anyone has sent a report.
- **Pull requests to `data/reports/`**: reports versioned in the repository,
  but too much friction for people who test a controller once; P4 can
  still commit reviewed reports as fixtures.
- **An email address or a hosted form**: personal data (addresses) and a
  vendor's terms, for no gain over issues.
- **`localStorage` or OPFS in the browser**: `localStorage` holds a few MB
  of strings and blocks the page; OPFS is file storage without queries.
  IndexedDB stores structured objects in Chromium, Firefox and Safari.
- **An ID in every report** (random or a hash): a random ID is one more
  value to correlate; a hash can be computed by whoever needs one.

## Consequences

- The re-plan of ROADMAP/2026-10-14/ (in place, same day): W6 becomes
  "Local report store, sharing and consent flow" and no longer waits for
  P2; W7 no longer waits for P5; A3 writes files and no longer waits for
  P2; P4 reads issues and files after P1; P2 becomes the sync API; new P6
  (sync in the apps) and M5 (reports synchronized). M2 moves from
  2026-11-25 to 2026-11-13, M4 from 2027-01-29 to 2027-01-17.
- Reports leave a device only when its owner downloads, copies or shares
  one. Browser storage can still be lost: private windows drop it, and
  Safari may delete a site's storage after seven days without a visit; the
  tester says so and offers downloads.
- Issues are public: the reports in them are anonymous by construction,
  but the issue shows the GitHub account that posted it. People without an
  account can still download a report.
- The size limit binds the format: P1 caps views, samples and HID
  structure. A desktop report with the DualSense's descriptor and HID
  structure in three views takes about 26,000 bytes, under half the limit.
- Curation needs no server until sync: P4 can start right after P1.
