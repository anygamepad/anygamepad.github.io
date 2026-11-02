# Privacy, consent and retention

Task P3 of [ROADMAP/2026-10-14/](../../ROADMAP/2026-10-14/03-collection-pipeline.md#p3--privacy-consent-data-retention):
what the project handles, the texts the apps show before a report is saved
or shared, how long each copy of a report is kept, and how maintainers
handle shared reports. It applies
[ADR 0007](../adr/0007-local-first-device-reports.md) (local-first reports,
shared as GitHub issues).

The public privacy policy is the page
[`apps/web/privacy/index.html`](../../apps/web/privacy/index.html),
published at <https://anygamepad.github.io/privacy/>. This file is its
maintainer side: change both together.

Owner decisions (2026-10-14): reports are dedicated to the public domain
under CC0 1.0; the only contact channel is this repository's issues;
a curated report's issue is closed and kept.

## Principles

1. **Nothing leaves a device by itself.** The apps read controllers locally,
   save reports only when the person presses a button, and send nothing:
   sharing is the person posting an issue on GitHub.
2. **Reports are anonymous by construction.** They have no field for
   serial numbers, Bluetooth addresses or anything about the person, and
   `Report::scrub` and `Report::validate` catch identifiers that slip into
   texts ([`data/reports/`](../../data/reports/README.md#privacy)).
3. **Consent is an action.** A button to save, a required checkbox to
   share; never a default, never pre-checked, never a banner to dismiss.
4. **Every copy has a keeper and an end** (see [Retention](#retention)).
5. **One place to ask:** this repository's issues, also to ask for a
   private channel.

## Data inventory

The only personal data the project handles is the GitHub account that posts
a report's issue, and whatever a person writes in the issue besides the
report. Everything else stays on the person's device or holds no personal
data.

| Data | Where | Kept | Who decides |
|------|-------|------|-------------|
| Controller input (Gamepad API, WebHID) | The page's memory | Until the page closes | The person; never sent |
| Theme choice | `localStorage`, key `anygamepad.theme`, only for Light or Dark | Until Auto is picked or storage is cleared | The person |
| Sending choice (settings) | `localStorage`, key `anygamepad.sync`, only while sending is on | Until the person turns it off or storage is cleared | The person |
| Deletion tokens of what was sent | `localStorage`, key `anygamepad.sync.sent`: kind, ID and token of each upload, 1,000 at most | Until the person withdraws the uploads or clears storage | The person |
| WebHID permissions | The browser | Until revoked in the site's settings | The person and the browser |
| Activity log, "Record 2 seconds" | The tab's memory; files when downloaded | Until the tab closes; files until deleted | The person |
| Saved reports (W6) | IndexedDB `anygamepad`, store `reports`, with local metadata (when saved, whether copied to share) | Until the person deletes them or the browser drops the storage | The person |
| Report files (desktop, A3) | `reports/` in the app's data folder (ADR 0007) | Until the person deletes them | The person |
| Shared reports | Issues labelled `device-report` in this repository | See [Retention](#retention) | The maintainers (controller under the GDPR), on the person's consent; GitHub for its platform |
| Visitor IP addresses | GitHub Pages logs, for security | GitHub's retention | GitHub, under its Privacy Statement; the project has no access |
| Curated data (profiles, test fixtures) | This repository | Indefinitely | The maintainers; no personal data |
| Uploaded reports (P6, later) | The sync server (P2, P5) | See [Server rules](#server-rules) | The maintainers |
| Sent activity logs and test reports | The sync server: a Cloudflare Worker with D1 in the EU ([ADR 0009](../adr/0009-sync-server-on-cloudflare-workers.md)), once the person turns sending on | See [Retention](#retention) | The maintainers; Cloudflare as processor |

Legal bases (GDPR Art. 6): consent (6(1)(a)) for shared reports, given by
the issue form's required checkbox and withdrawn by asking for the issue's
deletion. The theme key is stored only on the person's request, which the
ePrivacy rules exempt from consent (strictly necessary for a service the
person asked for); the site sets no cookie.

## Consent texts

The apps show these texts verbatim: the web tester from W6, the desktop app
from A3, sync from P6. Changing one changes this file, the policy page if
it is affected, and the smoke tests that check them. `{...}` marks values
the app fills in.

### Saving a report (web)

- Button: **Save a report**
- Next to it: "Saves a report of this pad in this browser. Nothing is sent:
  you can look at it, download it or delete it at any time."
- Above the list of saved reports: "Saved reports stay in this browser. A
  private window drops them when it closes, and Safari may delete them
  after seven days without a visit: download the ones you want to keep."
- Delete asks no confirmation of the report's content, only: "Delete this
  report from this browser?"

### Downloading a report

- Button: **Download**, titled "Saves the report as a JSON file on this
  device."

### Sharing a report (web and desktop)

Before anything is copied, a view titled **Share this report** shows:

> Sharing posts this report as an issue on GitHub, from your GitHub
> account. Issues are public: anyone can read the report and see who posted
> it, and it stays public until you ask for its deletion.
>
> The report holds no serial number, Bluetooth address or other personal
> data. Below is exactly what you will share: read it before you post.
>
> By posting it, you dedicate the report to the public domain under
> CC0 1.0, so that anyone can use it, the anygamepad library included.

Then the whole report (compact JSON, scrollable), and, when `scrub`
redacted something: "Replaced with [redacted] before sharing: {n} texts that
looked like identifiers ({field paths})." A link to the
[privacy policy](https://anygamepad.github.io/privacy/) follows.

- Buttons: **Copy and open the issue form**, **Cancel**.
- After copying: "The report is copied. Paste it into the Report field of
  the form, check it and submit the issue: nothing is shared until you
  submit it."
- When the clipboard refuses: "This browser did not let the page copy the
  report. Select the report above and copy it, or download it and paste the
  file's contents."
- Under the buttons: "No GitHub account? Download the report instead: it
  stays yours."

The desktop app (A3) shows the same view for a report file, with "Open the
file" in place of the copy fallback, and "No GitHub account? Keep the
report file instead: it stays yours." in place of the download line. Its
delete asks only: "Delete this report file?"

### The issue form

`.github/ISSUE_TEMPLATE/device-report.yml` (added by W6), label
`device-report`, title prefilled by the app:

- Introduction: "This form shares a device report made by the anygamepad
  tester or desktop app. Issues are public and show your GitHub account.
  Privacy policy: https://anygamepad.github.io/privacy/"
- Field **Report** (required): "Paste the report the app copied, without
  editing it: reports are checked when they are read."
- Required checkbox: "I reviewed this report: it holds no personal data,
  and I dedicate it to the public domain under CC0 1.0
  (https://creativecommons.org/publicdomain/zero/1.0/)."

No other field: anything else a person wants to say goes in a comment,
which keeps free text out of the report.

### Collection mode (desktop, A3)

When collection mode starts for the first time: "Collection mode records
how each input provider sees your controllers and saves the reports as
files in {folder}. Nothing is sent: share a report yourself, from its file,
when you want to." Buttons: **Start collecting**, **Open the folder**.

### Sync (P6)

Device reports: not written yet. P6 writes them under the
[server rules](#server-rules), and the policy page says what the server
stores before it accepts its first report.

Activity log and test reports (web, owner request of 2026-11-02). The
settings dialog, opened from the sliders icon in the header, titled
**Settings**:

- Switch, off by default: **Send the activity and the test reports to the
  anygamepad database**
- Under it: "While this is on, the page sends its activity log as it goes
  (the pads it detects, their inputs, reports and errors: what the
  downloaded log holds) and each test report when you save it, to improve
  how controllers are detected. Neither holds serial numbers, Bluetooth
  addresses or anything about you. It stays on in this browser until you
  turn it off."
- While the build names no database: "Not available yet: the database is
  not running. Until it is, nothing leaves this browser." (the switch
  cannot be turned on).
- While it is on: "Sent since this page loaded: {n} parts of the activity
  log, {m} test reports."
- Once this browser sent something: "This browser keeps a key for each of
  the {n} uploads it sent, to delete them from the database whenever you
  want. Sending stays as set above." Button: **Delete what this browser
  sent**; then "Deleted from the database: {deleted} of {n}.", and "Some
  could not be deleted now: try again later." when some failed.
- A link to the privacy policy.
- In the test mode, the save button reads **Save and send** while sending
  is on, and the note after saving adds "Sent to the anygamepad database."
  or "Not sent to the anygamepad database ({error}): save it again to try
  again."

What is sent (`apps/web/src/sync.ts`): the activity log's events in parts,
each with the page, the environment (browser engine and version, OS), a
random ID of the page load that joins its parts (never stored by the
page), the part's number and the day (UTC), never the start time; and
the test report exactly as the test mode downloads it. Each request
carries a deletion token made by the page (one per page load's log, one
per test report). No cookies, no referrer.

The policy page says what the server stores ("Sending to the anygamepad
database", 2026-11-02). The production build names the server once the
owner sets the repository variable `ANYGAMEPAD_SYNC_URL` after deploying
it ([`apps/server/README.md`](../../apps/server/README.md)); until then the
switch cannot be turned on.

## Retention

| Copy | Kept until | Deleted |
|------|------------|---------|
| Saved reports (browser), report files (desktop) | The person deletes them | Never by the apps; the browser may drop its storage |
| Shared report, not curated yet | Curated (P4) | On request, or when it holds personal data |
| Shared report, curated | Kept, closed, as the public source of the data | On request, or when it holds personal data |
| Profiles and fixtures made from reports | Indefinitely | They hold no personal data and cite issues by number only |
| Sent activity logs (sync server) | 180 days after they arrived (`ACTIVITY_LOG_RETENTION_DAYS`) | Every day by the server; earlier when the person withdraws them |
| Sent test reports (sync server) | The person withdraws them | With the deletion token, from the settings |
| Database restore points (D1 Time Travel) | 30 days (7 on Cloudflare's free plan) | By Cloudflare |
| Copies outside the project (GitHub's backups and emails, other people's copies of a public CC0 report) | Out of the project's reach | The policy says so |

Requests are answered within one month (GDPR Art. 12(3)).

## Handling shared reports

### Setup

- Organization owners allow deleting issues in the organization's
  repositories (organization settings, member privileges). In a repository
  owned by an organization nobody can delete an issue until they do, and
  without it a deletion request cannot be honoured.
- The issue form adds the `device-report` label; the label must exist.

### Every `device-report` issue

1. Read the report with `Report::from_json` and `Report::validate` (P4
   does). If it fails because of a finding (an identifier, an email
   address), or a person sees personal data in the issue (a name, an
   address, a photo), **delete the issue**. Do not edit it: anyone who can
   read the repository can read an issue's edit history. Deleting the
   revision from the history also works, but leaves the issue up.
2. Curate it (P4). Profile changes, commit messages and fixtures cite the
   issue by number (`#123`), never the account that posted it, and take
   nothing from the issue but the report.
3. Close it: as completed once a change uses it (with a comment naming the
   change), as not planned when it adds nothing new. Keep it.

### Requests

- **Deletion:** the account that posted a report asks on the issue, or in
  a new issue that links it. Delete the issue within one month. Data
  already curated from it stays: it holds no personal data, and the person
  dedicated it to the public domain.
- **Access, correction, restriction, objection, portability:** the project
  holds nothing about a person beyond their public issues; answer on the
  issue, within one month.
- **Private matters:** the person opens an issue that only asks to be
  contacted privately. A maintainer answers on it with a private channel
  and closes it. Never ask anyone for personal data in an issue.

## Server rules

For the sync server (P2, P5, P6; `apps/server`,
[ADR 0009](../adr/0009-sync-server-on-cloudflare-workers.md)). The policy
page changes before the server accepts anything new.

- It receives only what a person chooses to send, validated with
  `anygamepad-report` (test reports) or checked and scrubbed by the server
  (activity logs, `apps/server/src/activity.rs`), and stores the
  re-serialized document with the day it arrived. No accounts, no cookies.
- IP addresses are never stored, logged or passed on. Rate limiting gives
  Cloudflare's rate limiter an HMAC of the address and the day with a
  secret (`RATE_LIMIT_KEY`), which counts in memory for a minute.
- No request logs: Workers observability is off, and the Worker logs no
  addresses, user agents or contents.
- Each upload carries a deletion token that the app makes and keeps; with
  it the person withdraws the upload. The server stores only a hash of the
  token.
- Backups are D1's restore points (Time Travel): 30 days on the Workers
  Paid plan, 7 on the free one, so a withdrawn upload is gone from them
  within 30 days.
- Cloudflare processes the data under its Data Processing Addendum (GDPR
  Art. 28), and the policy names it; the database is created in the EU
  jurisdiction.
- Published dumps of the dataset hold the reports only, under CC0 1.0.

## Data license

Device reports are dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/)
([`data/reports/LICENSE-CC0`](../../data/reports/LICENSE-CC0)): every report
shared through the issue form or uploaded later, the dataset made of them,
and the reports in `data/reports/`.

Reason: the library embeds profiles built from reports. Reports are mostly
facts, but in the EU a collection of them can carry database rights; CC0
waives every right, so the profiles can be released under the library's
MIT/Apache-2.0 and games that embed them take on no attribution or
share-alike terms. Code, the report schema and the rest of the repository
stay under MIT or Apache-2.0; the SDL GameControllerDB import keeps its
zlib license ([`data/sdl-gamecontrollerdb/`](../../data/sdl-gamecontrollerdb/)).

## Changing the policy

- The policy page shows the date of its last change; its history is the
  file's history in this repository.
- A change that makes the project collect or share more is announced in
  the tester (and the desktop app) before it applies, and asks for consent
  again where a consent text changes.
