# The Workspace sheet-session contract

What CashGear Sheet and Cash Gear Workspace exchange so a person can open one of Workspace's
workbooks in the sheet, save new versions of it and save a copy beside it (Track S1 and S2). It is
the reference for the routes Workspace's Phase 5 builds in `D:\CashGear\Projects\cashgear-workspace`.

- The types are `packages/workspace-connector/src/contract.ts`, and `src/validate.ts` guards every
  answer.
- The mock in `tests/workspace-mock/server.mjs` behaves as this document says, and
  `packages/workspace-connector/src/session.test.ts` runs the sheet's side against it in
  `pnpm test`. A suite both repos run is Track S5.
- Workspace drew the first version in its `docs/INTEGRATIONS.md` ("Opening the full editor").
  The answers and defaults that fixed the rest are in `decisions.md` (2026-11-12, Track S1 and S2).

Contract version: `sheet-session.1`, 2026-11-12.

## The launch

1. In Workspace, the user chooses "Open in Cash Gear Sheet" on a workbook they may read.
   Workspace's own route `POST /api/v1/workbooks/{id}/launch` (its session, its permission check)
   issues a one-time ticket bound to the user, the workbook and the access level: 256 random bits,
   base64url without padding (43 characters), 120 seconds, stored as its SHA-256.
2. Workspace opens `https://sheet.cashgearerp.com/cloud#launch=<ticket>` in a new tab. The ticket
   is in the fragment, which browsers never send to a server.
3. The sheet reads the fragment before it renders, removes it from the address bar and the
   current history entry at once (`/cloud` becomes `/`), and redeems the ticket at Workspace's API
   for a file session.
4. The sheet reads the workbook by a signed URL and saves new versions with `If-Match` on the
   ETag.

**Which API the sheet trusts.** One origin, from the sheet's build, never from the link:
`VITE_WORKSPACE_API_URL` when the build names one, else `https://workspace-api.cashgearerp.com`,
and the mock (`http://127.0.0.1:4280`) for a page served from localhost. A link can't point the
sheet at another server. The desktop app has no Workspace launch until its deep link
(`desktop-deep-link.md`).

## Routes

JSON in camelCase, as Workspace's API writes it. Paths are under Workspace's API origin.

| Route | Method | Caller | Authentication | Body | Success |
| --- | --- | --- | --- | --- | --- |
| `/api/v1/workbooks/{id}/launch` | POST | Workspace's app | Workspace's session; read access to the workbook | none | 200 `{url}`, Workspace's own (not called by the sheet) |
| `/api/v1/sheet-sessions/redeem` | POST | The sheet, once, on launch | Anonymous; allow-listed `Origin`; rate-limited per IP | `{ticket}` | 200 `FileSession` |
| `/api/v1/sheet-sessions/renew` | POST | The sheet, while its user works | `X-Workspace-File-Session` | none | 200 `{expiresAt}` |
| `/api/v1/sheet-sessions/end` | POST | The sheet, on close | `X-Workspace-File-Session` | none | 204 |
| `/api/v1/sheet-sessions/workbook` | GET | The sheet, to read | `X-Workspace-File-Session`; read access now | none | 200 `WorkbookResponse` |
| `/api/v1/sheet-sessions/workbook/versions` | POST | The sheet, on Save | `X-Workspace-File-Session`; an `edit` session and edit access now; `If-Match` or `If-None-Match` | the xlsx | 201 `VersionResponse` |
| `/api/v1/sheet-sessions/workbook/copies` | POST | The sheet, on Save a copy | `X-Workspace-File-Session`; read access now, and the right to make workbooks in that workspace | the xlsx | 201 `CopyResponse` |

Every route decides on the server, on every request: a session never carries more than the
user's access at the time. A workbook trashed or deleted since the launch is 410, and access
taken away is 403, whatever the session said.

### Headers

| Header | Direction | Rule |
| --- | --- | --- |
| `X-Workspace-File-Session` | Request | The file session's token, on every route but Redeem. Never `Authorization`, never a cookie, never a URL |
| `If-Match` | Request | On a save: the ETag the sheet last read or saved, exactly as Workspace gave it (quoted) |
| `If-None-Match: *` | Request | On the first save of a workbook that has no version yet |
| `Content-Type` | Request | `application/json` for Redeem; `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet` for a save or a copy |
| `X-Correlation-Id` | Response | On every answer; a problem body repeats it as `correlationId` |
| `ETag` | Response | On `GET …/workbook` and on a save's or copy's 201: the version's quoted entity tag, the same text as the body's `etag` |
| `Retry-After` | Response | With every 429 and 503, in seconds |
| `Cache-Control: no-store` | Response | On every sheet-session answer |
| CORS | Both | The sheet-session routes allow the sheet's origins only: the live API lists `https://sheet.cashgearerp.com`; development and test hosts add `http://localhost:4173`, `http://127.0.0.1:4173`, `http://localhost:5173` and `http://127.0.0.1:5173`, as the mock does. Preflight allows `GET`, `POST`, `Content-Type`, `X-Workspace-File-Session`, `If-Match` and `If-None-Match`; answers expose `X-Correlation-Id`, `ETag`, `Retry-After` and `Location`. No credentials: the sheet sends none, and these routes must not take Workspace's cookie |

The sheet sends every request with `credentials: 'omit'`, `redirect: 'error'` (a redirect would
carry the session header with it) and no referrer.

## Bodies

```ts
type Access = 'edit' | 'view'; // Workspace's full and edit are edit; comment and view are view

interface RedeemRequest {
  ticket: string; // 43 characters of base64url
}

interface FileSession {
  token: string; // opaque; only ever sent back as X-Workspace-File-Session
  expiresAt: string; // ISO 8601: when it ends unless renewed (30 minutes idle), never past maxExpiresAt
  maxExpiresAt: string; // ISO 8601: 10 hours after Redeem
  workbook: { id: string; name: string };
  access: Access;
  user: { id: string; name: string };
  language: 'en' | 'ar'; // the Workspace user's; the sheet's cloud texts follow it
}

interface RenewResponse {
  expiresAt: string; // the idle expiry slid by 30 minutes, never past maxExpiresAt
}

interface WorkbookResponse {
  readUrl: string | null; // a signed URL for exactly the current version's blob, about 5 minutes
  etag: string | null; // the current version's quoted entity tag
  name: string;
  size: number | null; // bytes
} // readUrl, etag and size are null for a workbook with no version yet

interface VersionResponse {
  versionId: string;
  etag: string;
}

interface CopyResponse {
  workbook: { id: string; name: string }; // named by Workspace: "<name> (copy)", "<name> (نسخة)"
  access: Access; // the copy's: edit for its maker
  versionId: string;
  etag: string;
}
```

**Names.** `name` is the workbook's name as Workspace shows it. The sheet shows it with `.xlsx`
after it unless it already ends in a workbook extension, and saves an xlsx whatever it is called.

**Saving.** The body is the workbook's bytes, nothing else. Workspace checks `If-Match` against
the current version and writes the new version in one step, so two saves from the same ETag can't
both win. It caps the size (413 `limit.size`, with `limit` in bytes; 100 MB proposed) and refuses
a body that isn't a zip (400 `request.invalid`).

**A copy** is a new workbook in the same workspace with the bytes as its first version. The file
session moves to the copy: from then on `GET …/workbook`, saves and copies act on it, with the
copy's access. Workspace refuses a copy (403 `access.denied`) to someone who may not make
workbooks there; the sheet then offers to save to the device.

## Errors

Every error is RFC 9457 ProblemDetails (`application/problem+json`), as all of Workspace's are,
with Workspace's stable `code` and the request's `correlationId`. The sheet switches on `code`
(and on the status where a code is missing), never on `title`.

```json
{
  "type": "https://tools.ietf.org/html/rfc9110#section-15.5.13",
  "title": "A newer version was saved",
  "status": 412,
  "code": "version.conflict",
  "correlationId": "0193a1c0f2e47000…",
  "current": {
    "versionId": "…",
    "etag": "\"…\"",
    "sha256": "…",
    "savedBy": { "id": "…", "name": "Omar" },
    "savedAt": "2026-11-12T10:41:07Z"
  }
}
```

| Status | `code` | When | What the sheet does |
| --- | --- | --- | --- |
| 400 | `request.invalid` | No ticket; a body that isn't an xlsx; the wrong `Content-Type` | Says the save was refused |
| 401 | `ticket.invalid` | Redeem: the ticket is unknown, used or expired | "This link has expired or has already been used. Open the workbook again from Cash Gear Workspace." |
| 401 | `session.expired` | The session ended, expired or is unknown | Ends the session here; the edits stay; Save offers Reconnect |
| 403 | `origin.denied` | Redeem from an `Origin` not on the list, checked before the ticket is looked at, so the ticket stays usable | "This site can't connect to Cash Gear Workspace." |
| 403 | `access.denied` | Access taken away; a save from a `view` session or without edit access now; a copy Workspace won't make | Reading: "You don't have access to this workbook." Saving: treats the session as view-only and offers Save a copy. A copy: offers the device |
| 410 | `workbook.gone` | The workbook was trashed or deleted | "This workbook was deleted in Cash Gear Workspace." Save a copy, or the device |
| 412 | `version.conflict` | `If-Match` isn't the current version, or `If-None-Match: *` and a version exists. The body's `current` names the current version | Offers Save a copy, Reload or Cancel, naming who saved and when. If `current.sha256` is the SHA-256 of the bytes it sent, its own earlier attempt landed: that counts as saved |
| 413 | `limit.size` | The body is over Workspace's cap, given as `limit` | "This workbook is too large to save to Cash Gear Workspace." |
| 428 | `version.required` | A save with neither `If-Match` nor `If-None-Match` | Never sent by the sheet |
| 429 | `limit.rate` | Over a rate limit, with `Retry-After` | Waits as asked (at most 10 s a time, three times), then says Workspace is busy |
| 500, 503 | `server.error` | Workspace failed | Retries a 503 as a 429; says Workspace couldn't do it |

## Retries

| Request | Sent again after no answer | Why |
| --- | --- | --- |
| Redeem | Never | Workspace may have used the ticket up |
| Renew, End, `GET …/workbook` | Yes | They change nothing twice |
| A save | Yes | A repeat after a lost answer gets 412 with `current.sha256` equal to the bytes', which the sheet counts as saved, so no version is ever made twice |
| A copy | Never | The copy may have been made |

## Reading the bytes

`readUrl` is a signed URL for one blob (Workspace's user-delegation SAS), good for about five
minutes. The sheet asks for a fresh one each time it reads, reads it with no credentials, no
referrer and no redirects, and asks for a fresh one once more if Blob Storage refuses it (an
expired signature, 403 or 404).

- **CORS on Blob Storage:** the storage account's CORS rule allows `GET` from the sheet's origins
  (as above), so the sheet's cross-origin isolated page (COEP `require-corp`) can read the answer.
- **The sheet's CSP:** `connect-src` names the storage account's origin exactly
  (`https://<account>.blob.core.windows.net`, or Azurite's in development). Until Workspace's
  account exists the live sheet lists only Workspace's API, so it can't read a workbook yet; the
  account's origin goes into `apps/web/src/csp.ts` and `staticwebapp.config.json` when it does.
- The mock's blob server is one port above its API (`http://127.0.0.1:4281`), and refuses an
  expired or wrong signature with 403 `AuthenticationFailed`, as Azure does.

## Lifetimes and the session's life

| What | Lifetime |
| --- | --- |
| A ticket | 120 seconds, one use |
| A file session | 30 minutes idle, 10 hours at most; Renew slides the idle expiry by 30 minutes, never past the maximum |
| A signed read URL | About 5 minutes |

- **Renew:** after input in the tab (keys, pointer, wheel), at most every 5 minutes. The sheet's
  own timer ends the session at its idle or longest expiry without asking.
- **End:** sent with `keepalive` when the tab closes (`pagehide`), and when another workbook
  replaces Workspace's in the tab. Its answer doesn't matter.
- **The token** is held in memory only, as a private field. It is never written to a URL,
  `localStorage`, `sessionStorage`, IndexedDB, OPFS, a recovery draft, a log or an error.
- **Reconnect:** when a session ends with changes unsaved, the tab waits for the user's next
  launch of the same workbook. The new tab redeems its own ticket, then asks over
  `BroadcastChannel` (`cgs-cloud-reconnect`, this origin's tabs only) whether a tab waits for the
  same workbook id, user id and API origin; if one does, it hands its session over in memory and
  closes without ending it. The waiting tab checks the scope again and carries on, still saving
  with the ETag it had, so a version saved meanwhile is still a 412.

## View access

A `view` session opens the workbook, and the person can change it in the sheet. Save offers
"Save a copy" (a copy in Workspace) instead of saving; Save As still saves to the device. Nothing
reaches the workbook itself.

## A workbook with no version

Workspace's "Create a new workbook" makes the record and lets the sheet make the first version:
`GET …/workbook` answers `readUrl`, `etag` and `size` null, the sheet opens a blank workbook (right
to left for an Arabic session), and its first save sends `If-None-Match: *`.

## The mock

`pnpm workspace:mock` (`node tests/workspace-mock/server.mjs [--port 4280] [--blob-port 4281]
[--sheet-url http://127.0.0.1:4173]`). Playwright starts it for the browser tests, or reuses one
already running.

| Route | What it does |
| --- | --- |
| `POST /__mock/reset` | Back to the seed: Budget 2027 (one version, by Omar Other; the viewer may only view it), New workbook (no version), the four users, copies allowed |
| `POST /__mock/launch` | `{user, workbook, access?, language?}`: what Workspace's launch route does, giving `{ticket, expiresAt, url}` |
| `POST /__mock/version` | `{workbook, user, bytes?}`: someone else saves a version (base64 bytes, or the seed's) |
| `POST /__mock/level` | `{workbook, user, level}`: `edit`, `view` or `none` |
| `POST /__mock/trash` | `{workbook, trashed?}` |
| `POST /__mock/copies` | `{allowed}` |
| `POST /__mock/clock` | `{advanceMs}`: moves the mock's clock for tickets, sessions and signatures |
| `POST /__mock/fault` | `{match, status?, code?, retryAfter?, times?, drop?, latencyMs?}`, as the ERP mock's: `match` is a regular expression over the path (`workbook$`, `versions$`, `^blobs/`) |
| `GET /__mock/bytes?workbook=` | The current version's bytes |
| `POST /__mock/content` | `{workbook}`: what Workspace's `GET /api/v1/workbooks/{id}/content` gives its page for the embed, `{readUrl, etag, size, versionId}`, signed on the mock's clock (Track S3) |
| `GET /__mock/state` | Counts, saves with their preconditions, tickets, sessions and workbooks, with no tokens |

Users and workbooks are named `editor`, `viewer`, `other` and `arabic`, `budget` and `empty`, or
by their ids (`USERS` and `WORKBOOKS` in the mock).
