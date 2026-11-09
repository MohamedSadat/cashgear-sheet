# The ERP sheet contract

What CashGear Sheet and Cash Gear ERP exchange to edit ERP records in a sheet, Vendors first. It is
the reference for Track E in `D:\CashGear\Projects\CashGear`.

- The types are `packages/erp-connector/src/contract.ts`, and `src/validate.ts` guards every
  response.
- The mock in `tests/erp-mock/server.mjs` behaves as this document says.
- The contract suite, `packages/erp-connector/src/contract.test.ts`, proves a server does. It runs
  in `pnpm test` against the mock, and against UnifiedAPI as [below](#running-the-suite-against-unifiedapi).
- The design is in `ERP_SHEET_INTEGRATION_PLAN.md` section C, and the answers and defaults are in
  `decisions.md` (2026-11-09, Planning the ERP integration, and Phase 14).

Contract version: `vendors.1`, Phase 14 (2026-11-09).

## Routes

Routes are module first, as the ERP's are. Every route takes and returns JSON
(System.Text.Json, camelCase), except End, which returns no body.

| Route | Method | Who calls it | Authentication | Success |
| --- | --- | --- | --- | --- |
| `api/system/SheetLaunch/CreateTicket` | POST | `ERPClient_React`'s Vendors list | `X-API-Key`, and the Vendors unit's View | 200 `CreateTicketResponse` |
| `api/sheet/Session/Redeem` | POST | The sheet, once, on launch | Anonymous; allow-listed `Origin`; rate-limited per IP | 200 `SheetSession` |
| `api/sheet/Session/Renew` | POST | The sheet, while its user works | `X-Sheet-Session` | 200 `RenewResponse` |
| `api/sheet/Session/End` | POST | The sheet, on close | `X-Sheet-Session` | 204 |
| `{entityRoute}/Schema` | GET | The sheet | `X-Sheet-Session`, View | 200 `EntitySchema` |
| `{entityRoute}/Query` | POST | The sheet | `X-Sheet-Session`, View | 200 `QueryResponse` |
| `{entityRoute}/Commit` | POST | The sheet; Validate is `dryRun: true` | `X-Sheet-Session`, Edit | 200 `CommitResponse` |
| `{entityRoute}/CommitStatus/{commitId}` | GET | The sheet, after a lost response | `X-Sheet-Session`, View | 200 `CommitStatusResponse` |

`entityRoute` comes from the session, so the sheet names no entity. Vendors' is
`api/payable/VendorSheet`. Routes match without regard to case, as ASP.NET's do.

### Headers

| Header | Direction | Rule |
| --- | --- | --- |
| `X-Sheet-Session` | Request | The session token. Never in `Authorization`: the API-key handler reads a raw `Authorization` value as a key and looks it up uncached |
| `X-API-Key` | Request | CreateTicket only. An API key is refused on every sheet route, and a sheet token everywhere else. CreateTicket runs under the API-key scheme, so a bad key gets that handler's 401 with no body; the entity module's policy is checked inside the action (`IAuthorizationService.AuthorizeAsync(User, "Payable")`, since one action serves every entity), and its 403 carries an `ErrorBody` |
| `X-Correlation-Id` | Response | On every response. Error bodies repeat it as `correlationId` |
| `X-Company-Id` | Response | On every response with a session |
| `Cache-Control: no-store` | Response | On every sheet response, because `UseResponseCaching` runs between authentication and authorization |
| `Retry-After` | Response | With 429 and 503, in seconds |
| CORS | Both | The sheet routes allow the sheet's origins only: `https://sheet.cashgearerp.com`, `http://localhost:4173`, `http://127.0.0.1:4173`, `http://localhost:5173` and `http://127.0.0.1:5173`. Preflight allows `GET`, `POST`, `Content-Type` and `X-Sheet-Session`, and responses expose `X-Correlation-Id`, `X-Company-Id` and `Retry-After`. No credentials |

## Launch and session

1. The ERP client calls CreateTicket with `{entity, filter, language}` and opens the returned
   `url` (`https://sheet.cashgearerp.com/erp#launch=<ticket>`) in a new tab.
2. The sheet reads the ticket from the fragment, removes it from the address bar, and calls Redeem
   with `{ticket}`.
3. Redeem returns the session: its token, expiries, company, user, language, entity,
   `entityRoute` and filter.

**Tickets**
- 256 random bits in base64url (43 characters), stored as a SHA-256 hash.
- Single use, and expire after 120 seconds.
- Bound to the user, company, entity, filter and language, and to the API key that made them. A
  ticket whose key has been deactivated is refused.
- Unknown, used and expired tickets get the same answer, 401 `ticket.invalid`, so their state never
  leaks.
- Redeem checks `Origin` first. A missing or unlisted Origin gets 403 `origin.denied` without
  looking at the ticket, so a refused site can't use it up.
- Redeem is anonymous, so it calls `SetCurrentTenant(ticket.Company)` before opening any tenant
  context; otherwise `TenantService` would take `X-Company-Id` from the request. It marks the ticket used in
  the same statement that checks it (`UPDATE … WHERE RedeemedAt IS NULL AND ExpiresAt > @now`), so
  two redeems can't both win.
- On its own a ticket calls nothing: as `X-Sheet-Session` it gets 401.

**Sessions**
- An opaque 256-bit token, stored as a SHA-256 hash in CGService.
- 30 minutes idle and 10 hours at most. Only Renew slides the idle time, never past
  `maxExpiresAt`. The sheet renews while its user is working, whether or not it is sending
  requests, so the server writes the session row only on Renew.
- Every request checks that the session hasn't ended or expired, and that its API key and company
  are still active.
- Scoped to `api/sheet/*` and the session's `entityRoute`. Any other entity route answers 403
  `privilege.denied`.
- End ends it at once; later calls get 401.

## Schema

`GET {entityRoute}/Schema` returns the entity's fields, already resolved for this caller.

| Property | Meaning |
| --- | --- |
| `schemaVersion` | Names this field list (`vendors.1`). A commit naming another gets 400 `schema.changed` |
| `keyField` | The business key, shown in a read-only cell. Vendors: `vendorAccount` |
| `idField` | The surrogate id, which travels only as a record's `id`, never in `values`. Vendors: `recId` |
| `maxPageSize`, `maxRecords`, `maxCommitRecords` | 1,000, 10,000 and 500 |
| `fields[]` | `name`, `label {en, ar}`, `type`, `editable`, and when read-only `editReason`; then `required`, `maxLength`, `min`, `max`, `scale`, `lookup` as they apply |
| `lookups` | Each lookup list by name, as `{code, label, active?}`, for the session's company. Inactive items come with `active: false`, so records that hold one still show it, but no change may choose it (`lookup.notFound`). The ERP keeps one description per item, so `en` and `ar` may be the same text |

Field types: `text`, `integer`, `decimal` (with `scale`), `boolean`, `date`, `dateTime`, `lookup`.
`editReason` is `notEditable` (no one may edit it from the sheet), `needsEdit` or `needsAdvEdit`.

### Vendors (`vendors.1`)

Decided on 2026-11-09 (Phase 14): the fields `UpdateEx` writes today, plus read-only context.

| Field | Type | Constraints | Editable with | `VendorModel` column |
| --- | --- | --- | --- | --- |
| `vendorAccount` | text | 50, the key | Never | `VendorAccount` |
| `vendorName` | text | 150, required | Edit | `VendorName` |
| `vendorGroup` | lookup `vendorGroups` | required | Edit | `VendorGroup` → `VendorsGroup (Company, GroupId)`, whose `Active` flag gives `active` |
| `payMethod` | lookup `paymentModes` | required | Edit | `PayMethod` → `PaymentModes (Company, PaymentId)`: payment terms with `PaymentDays`, all active |
| `currency` | lookup `currencies` | required, as `Save` requires it | AdvEdit | `Currency` → `LedgerCurrencies (Company, Code)`, with no foreign key; an old blank `""` is sent as `null`, and can't be chosen |
| `mobile` | text | 50 | Edit | `Mobile` |
| `taxFile` | text | 50 | Edit | `TaxFile` |
| `address` | text | 150 | Edit | `Address` |
| `blocked` | boolean | required | AdvEdit | `Blocked` (0 is false, anything else true; written as 0 or 1) |
| `bankAccount` | text | 50 | Never: bulk bank-account edits are a payment-fraud risk | `BankAccount` |
| `debitLimit` | decimal, scale 2 | from 0 | Never | `DebitLimit`: EF's default `decimal(18,2)`, to be confirmed in `sys.columns` before E1 declares the scale |
| `modifiedBy` | text | 50 | Never | `ModifiedBy` |
| `modifiedDate` | dateTime | | Never | `ModifiedDate`, written as `DateTime.Now`, so sent with the host's own offset |

Never sent: `Token`, `SecRule`, `CreatedBy`, `CreatedDate` and the rest of `VendorModel`. `RecId`
goes only as each record's `id`. The column order above is the mock's; Phase 15 asks for the
sheet's.

## Query

`POST {entityRoute}/Query` with `{filter, ids, afterId, pageSize}`.

- Keyset paging: records come in ascending `id`, after `afterId`, at most `pageSize` (1 to
  `maxPageSize`) at a time.
- `nextAfterId` is the last record's id when more follow, and `null` on the last page. `total`
  counts every match, whatever `afterId`.
- `filter`: `search` matches the key or the name, ignoring case (in C#, as `VendorViewService`
  does, to avoid Arabic collation quirks); any other property names a field
  whose value must be equal. A filter on a field the ERP doesn't filter by is refused with
  `request.invalid`.
- `ids` (refresh): only those records, those still in the ERP and in the session's company, and
  `filter` is ignored. More than `maxRecords` ids gets 413.
- Another company's records are never returned, though Kit and Dot share a database.
- Each record is `{id, key, version, values}`. `values` holds every schema field and nothing
  else, and the key field's value equals `key`.
- The ERP reads the database (`AsNoTracking`), never its cache.
- Vendors have no ownership or row-lock rules in the ERP: `SysSecRules` is a class nothing reads,
  `Vendors.SecRule` is never read, and `LoadOwnedRowsPurch` filters purchase orders only (Track
  E0). So Vendors filter by company alone, as the ERP's own vendor list does, and `record.locked`
  is reserved for an entity that has locks. The mock's locked `V-1005` tests the code path.

## Commit

`POST {entityRoute}/Commit` with `{commitId, company, schemaVersion, dryRun, records}`. Each record
is `{id, key, version, changes}`, and each change `{from, to}`. Only changed fields are sent.

**Checks, in order.** The first that fails answers, and nothing is written.

| # | Check | Answer |
| --- | --- | --- |
| 1 | A live session | 401 `session.expired` |
| 2 | The Vendors unit's Edit privilege, dry runs included | 403 `privilege.denied` |
| 3 | The request's shape: a UUID `commitId`, at least one record, ids not repeated, every record with changes and every change with a `to` | 400 `request.invalid` |
| 4 | At most `maxCommitRecords` records | 413 `batch.tooLarge` |
| 5 | Not a dry run, and `(company, commitId)` already stored | The stored 200 body if the payload's hash matches, else 400 `commit.reused` |
| 6 | `company` is the session's | 400 `company.mismatch` |
| 7 | `schemaVersion` is current | 400 `schema.changed` |
| 8 | Each record and field: the record exists in the session's company (`record.notFound`), carries a version (`version.missing`) and isn't locked (`record.locked`); each field is editable (`field.readOnly`, for unknown fields too), allowed for this caller (`privilege.denied`) and valid (`type`, `required`, `maxLength`, `range`, `scale`, and `lookup.notFound` for an unknown or inactive code). Only the changed fields are checked, so a record may keep an old inactive code | 400 listing **every** error |
| 9 | Each record's `version` equals its row version now, read under `UPDLOCK, HOLDLOCK` | 409 listing **every** conflicting record |
| 10 | Write all records in one transaction, with `ModifiedBy`, `ModifiedDate`, audit and the `SheetCommit` row | 200 `CommitResponse` |

**Rules**
- **All or nothing.** Up to 500 records in one transaction. MARS rules out savepoints, so any
  failure rolls the whole batch back.
- **Strict versions.** Any write since the record was read is a conflict, even to other fields.
  `from` is for audit and diagnostics only. A conflict's `fields` lists the commit's fields whose
  value now differs from their `from`, and may be empty.
- **Dry runs** run every check, versions included, and write nothing, not even the `SheetCommit`
  row, so the real commit may reuse their `commitId`. They answer with `dryRun: true`,
  `committedAt: null`, versions unchanged and values as they would be saved.
- **Idempotency.** The client makes one `commitId` (UUID v4) per commit attempt and reuses it on
  every retry. `SheetCommit (Company, CommitId)` is written in the commit's transaction with a
  SHA-256 hash of the payload: `company`, `schemaVersion` and the records sorted by id, as JSON
  with sorted keys (the mock's `canonical`). A retry returns the stored body, and the same id with
  another payload is refused. CommitIds are per company, because Kit and Dot share a database.
- **CommitStatus** answers `{status: "committed", …the stored 200 body}` or `{status: "unknown"}`.
  Unknown means nothing was written under that id in the session's company, so a retry is safe. A
  refused commit stores nothing, so it is unknown too.
- **Audit.** One `SheetChangeAudit` row per changed field, and a SystemLog summary row, in the same
  transaction.
- **After commit.** `cache.RemoveByTag("Vendors")`, as `UpdateEx` does, outside the transaction's
  `catch` as UnifiedAPI's CLAUDE.md requires, in its own try/catch. `{Company}:Vendors` is a cache
  key, not a tag, and the company tag would clear everything cached for the company. The Blazor
  host's cache has no backplane and stays stale for up to 8 hours.
- `ModifiedBy` is set to the session's user, and `ModifiedDate` to now, as `UpdateEx` sets them.

## Values on the wire

| Type | On the wire | Notes |
| --- | --- | --- |
| text, lookup | String | `""` and `null` are equal, and `""` is saved as `null` (or `""` where the column isn't nullable, as `Currency`) |
| integer | Number | |
| decimal | String, `-?digits[.digits]` | At most `scale` decimals. Sent as strings because cells hold doubles; past 15 significant digits the sheet refuses rather than rounds |
| boolean | `true` or `false` | Vendors' `Blocked` int is mapped by the server |
| date | `YYYY-MM-DD` | |
| dateTime | ISO 8601 with an offset (`Z` or `+02:00`) | Read-only in the MVP. The ERP writes local `DateTime.Now`, so it sends `new DateTimeOffset(value, TimeZoneInfo.Local.GetUtcOffset(value))`, and the sheet shows the wall-clock time in that offset, as the ERP shows it. The hour a DST change repeats can be an hour out |
| Empty | `null` | |
| Version | Base64 of 8 bytes | SQL Server's `rowversion` as System.Text.Json writes a `byte[]` |

## Status codes and error codes

| Status | Meaning | Body |
| --- | --- | --- |
| 200 | Done, or a dry run passed | The route's response |
| 204 | End | None |
| 400 | The request has errors; nothing written | `ErrorBody` |
| 401 | The session or ticket is missing, unknown, used or expired | `ErrorBody` with `session.expired` or `ticket.invalid`; CreateTicket's bad key gets the API-key handler's empty 401 |
| 403 | A unit privilege or the Origin is missing | `ErrorBody` with `privilege.denied` or `origin.denied` |
| 409 | Records changed since they were read; nothing written | `ConflictBody` |
| 413 | Too many records or ids | `ErrorBody` with `batch.tooLarge` |
| 429, 503 | Try later, after `Retry-After` | `ErrorBody` |
| 500 | Something failed; nothing written | `ErrorBody` |

`ErrorBody` is `{message, correlationId, errors[]}`, the shape of the ERP's item endpoints. Each
error is `{id?, key?, field?, code, message}`: a field error has all three, a record error `id` and
`key`, a batch error neither. `message` is in the session's language and is only the sheet's
fallback, since the sheet shows its own text by code.

| Code | Level | Status |
| --- | --- | --- |
| `required`, `maxLength`, `type`, `range`, `scale`, `lookup.notFound` | Field | 400 |
| `field.readOnly` | Field | 400 |
| `privilege.denied` | Field (an AdvEdit field), or batch | 400, or 403 for the whole request |
| `record.notFound`, `record.locked`, `version.missing` | Record | 400 |
| `version.conflict` | Record | Reserved: 409 carries conflicts, not errors |
| `batch.tooLarge` | Batch | 413 |
| `company.mismatch`, `schema.changed`, `commit.reused`, `request.invalid` | Batch | 400 |
| `session.expired`, `ticket.invalid` | Batch | 401 |
| `origin.denied` | Batch | 403 |

Phase 14 added `ticket.invalid`, `origin.denied`, `commit.reused` and `request.invalid` to plan
C.8's list.

## C# DTOs

For `CG.Infrastructure/CGModels/APIModel/SheetModels.cs`. Values travel as `JsonElement` and each
entity's connector converts them by field type, so the DTOs stay entity-neutral. Properties that
are null are left out (`JsonIgnoreCondition.WhenWritingNull`), so `{status: "unknown"}` and an
editable field's missing `editReason` serialize as the contract says; the sheet's guards also
take a `null` optional property as absent.

- `commitId` is a `string`, parsed by the action, so a bad one is `request.invalid` rather than
  `[ApiController]`'s ProblemDetails 400. Leave `[ApiController]` off the sheet controllers, or
  put a filter ahead of it; don't change the global `ApiBehaviorOptions`.
- `AddControllers()` uses System.Text.Json's defaults, which write decimals as numbers. Don't add
  a global decimal converter: build `values` from strings formatted with the invariant culture.

```csharp
using System.Text.Json;
using System.Text.Json.Serialization;

namespace CG.Infrastructure.CGModels.APIModel;

public sealed record SheetLocalized(string En, string Ar);

// Launch and session
public sealed record SheetCreateTicketRequest(
    string Entity, Dictionary<string, JsonElement>? Filter, string Language);

public sealed record SheetCreateTicketResponse(string Ticket, DateTimeOffset ExpiresAt, string Url);

public sealed record SheetRedeemRequest(string Ticket);

public sealed record SheetSessionResponse(
    string Token, DateTimeOffset IdleExpiresAt, DateTimeOffset MaxExpiresAt,
    string Company, string UserId, string UserName, string Language,
    string Entity, string EntityRoute, Dictionary<string, JsonElement>? Filter);

public sealed record SheetRenewResponse(DateTimeOffset IdleExpiresAt);

// Schema
public sealed record SheetFieldSchema
{
    public required string Name { get; init; }
    public required SheetLocalized Label { get; init; }
    public required string Type { get; init; }          // text, integer, decimal, boolean, date, dateTime, lookup
    public required bool Editable { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? EditReason { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public bool? Required { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public int? MaxLength { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public JsonElement? Min { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public JsonElement? Max { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public int? Scale { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? Lookup { get; init; }
}

public sealed record SheetLookupItem
{
    public required string Code { get; init; }
    public required SheetLocalized Label { get; init; }   // one Description, sent as both
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public bool? Active { get; init; }  // false only
}

public sealed record SheetEntitySchema(
    string Entity, string SchemaVersion, SheetLocalized Label, string KeyField, string IdField,
    int MaxPageSize, int MaxRecords, int MaxCommitRecords,
    IReadOnlyList<SheetFieldSchema> Fields,
    IReadOnlyDictionary<string, IReadOnlyList<SheetLookupItem>> Lookups);

// Query
public sealed record SheetQueryRequest(
    Dictionary<string, JsonElement>? Filter, int[]? Ids, int AfterId, int PageSize);

/// Values hold every schema field: strings, decimals as strings, bools, dates as text, or null.
public sealed record SheetRecord(int Id, string Key, string Version, IReadOnlyDictionary<string, object?> Values);

public sealed record SheetQueryResponse(
    IReadOnlyList<SheetRecord> Records, int? NextAfterId, int Total, DateTimeOffset AsOf);

// Commit
public sealed record SheetFieldChange(JsonElement From, JsonElement To);

public sealed record SheetRecordChange(
    int Id, string Key, string? Version, Dictionary<string, SheetFieldChange> Changes);

public sealed record SheetCommitRequest(
    string CommitId, string Company, string SchemaVersion, bool DryRun,
    IReadOnlyList<SheetRecordChange> Records);

public sealed record SheetCommitResponse(
    string CommitId, bool DryRun, DateTimeOffset? CommittedAt, IReadOnlyList<SheetRecord> Records);

public sealed record SheetCommitStatusResponse
{
    public required string Status { get; init; }        // committed, unknown
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? CommitId { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public bool? DryRun { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public DateTimeOffset? CommittedAt { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public IReadOnlyList<SheetRecord>? Records { get; init; }
}

// Errors
public sealed record SheetApiError
{
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public int? Id { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? Key { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? Field { get; init; }
    public required string Code { get; init; }
    public required string Message { get; init; }
}

public sealed record SheetErrorResponse(string Message, string CorrelationId, IReadOnlyList<SheetApiError> Errors);

public sealed record SheetConflict(
    int Id, string Key, string Version, IReadOnlyDictionary<string, object?> Values,
    IReadOnlyList<string> Fields, string? ModifiedBy, DateTimeOffset? ModifiedAt);

public sealed record SheetConflictResponse(string Message, string CorrelationId, IReadOnlyList<SheetConflict> Conflicts);

public static class SheetErrorCodes
{
    public const string Required = "required", MaxLength = "maxLength", Type = "type", Range = "range",
        Scale = "scale", LookupNotFound = "lookup.notFound", FieldReadOnly = "field.readOnly",
        PrivilegeDenied = "privilege.denied", RecordNotFound = "record.notFound",
        RecordLocked = "record.locked", VersionMissing = "version.missing",
        VersionConflict = "version.conflict", BatchTooLarge = "batch.tooLarge",
        CompanyMismatch = "company.mismatch", SchemaChanged = "schema.changed",
        SessionExpired = "session.expired", TicketInvalid = "ticket.invalid",
        OriginDenied = "origin.denied", CommitReused = "commit.reused",
        RequestInvalid = "request.invalid";
}
```

Notes for the DTOs:
- `DateTimeOffset` serializes with its offset (`2026-11-09T10:00:05+00:00`), which the contract
  takes. A `DateTime` read from a `datetime` column has no offset, so the connector gives it the
  host's local offset before sending it.
- A `byte[]` RowVersion is sent as base64, as `CustPaymentCmdController` sends its own, and a
  version that isn't 8 bytes of base64 is `request.invalid`.
- The sheet makes lower-case UUIDs. Store `CommitId` as a `uniqueidentifier`, which compares
  without regard to case, and echo the string the request sent.

## Table scripts

DB-first and idempotent, as the ERP's scripts are: named `<Verb>_<Subject>.sql`, a header saying
where to run them, `SET ANSI_NULLS ON; SET QUOTED_IDENTIFIER ON;`, and each change guarded so a
second run changes nothing. Run each against the Test database first, then each tenant.

**1. `CashGear.UnifiedAPI/DbScripts/Add_VendorsRowVersion.sql`, in every tenant database.** Kept on
its own, because it gates the deploy. Adding a `rowversion` column fills it on every row; Vendors
is small, so it takes moments. `Create_Projects.sql` adds `ProjTable.RowVersion` the same way.

```sql
-- DB-first, idempotent. Run against Test first, then every tenant database, BEFORE any host
-- with [Timestamp] on VendorModel is deployed (docs/erp-contract.md, RowVersion rollout plan).
SET ANSI_NULLS ON;
SET QUOTED_IDENTIFIER ON;
GO
IF COL_LENGTH(N'dbo.Vendors', N'RowVersion') IS NULL
    ALTER TABLE [dbo].[Vendors] ADD [RowVersion] [rowversion] NOT NULL;
GO
-- Check: RowVersionBytes 8 means done. DebitLimit's precision and scale confirm the schema's scale.
SELECT DB_NAME() AS [Database],
       COL_LENGTH(N'dbo.Vendors', N'RowVersion') AS RowVersionBytes,
       (SELECT CONCAT(c.precision, ',', c.scale) FROM sys.columns c
         WHERE c.object_id = OBJECT_ID(N'dbo.Vendors') AND c.name = N'DebitLimit') AS DebitLimit;
```

**2. `CashGear.UnifiedAPI/DbScripts/Create_SheetCommit.sql`, in every tenant database.** Keyed by
company, because Kit and Dot share a database.

```sql
-- DB-first, idempotent. Run against Test first, then every tenant database.
SET ANSI_NULLS ON;
SET QUOTED_IDENTIFIER ON;
GO
IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = N'SheetCommit' AND schema_id = SCHEMA_ID(N'dbo'))
CREATE TABLE [dbo].[SheetCommit] (
    [Company]     nvarchar(50)      NOT NULL,
    [CommitId]    uniqueidentifier  NOT NULL,
    [Entity]      nvarchar(50)      NOT NULL,
    [UserId]      nvarchar(50)      NOT NULL,
    [PayloadHash] binary(32)        NOT NULL,
    [Result]      nvarchar(max)     NOT NULL,  -- the 200 body, returned again on a retry
    [CreatedAt]   datetimeoffset(3) NOT NULL CONSTRAINT [DF_SheetCommit_CreatedAt] DEFAULT SYSDATETIMEOFFSET(),
    CONSTRAINT [PK_SheetCommit] PRIMARY KEY ([Company], [CommitId])
);
GO
IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = N'SheetChangeAudit' AND schema_id = SCHEMA_ID(N'dbo'))
CREATE TABLE [dbo].[SheetChangeAudit] (
    [AuditId]       bigint IDENTITY(1,1) NOT NULL CONSTRAINT [PK_SheetChangeAudit] PRIMARY KEY,
    [Company]       nvarchar(50)      NOT NULL,
    [CommitId]      uniqueidentifier  NOT NULL,
    [Entity]        nvarchar(50)      NOT NULL,
    [RecId]         int               NOT NULL,
    [Field]         nvarchar(50)      NOT NULL,
    [OldValue]      nvarchar(max)     NULL,
    [NewValue]      nvarchar(max)     NULL,
    [UserId]        nvarchar(50)      NOT NULL,
    [At]            datetimeoffset(3) NOT NULL,
    [CorrelationId] nvarchar(64)      NOT NULL,
    INDEX [IX_SheetChangeAudit_Record] ([Company], [Entity], [RecId]),
    INDEX [IX_SheetChangeAudit_Commit] ([Company], [CommitId])
);
```

**3. `CashGear.IdentitySystem/DbScripts/Create_SheetSessions.sql`, in CGService.** CGService's
scripts live in IdentitySystem. These tables are read through a UnifiedAPI-only `SheetDbContext`
on `DefaultConnection`, not the shared identity model, and `TenantRegistry` refuses
`DefaultConnection` as a tenant, so they can't be reached through the tenant factory. Only hashes
are stored. `ApiKeyId` is the key's `APIKeys` row, so a revoked key ends its tickets and sessions.

```sql
-- DB-first, idempotent. Run against CGService.
SET ANSI_NULLS ON;
SET QUOTED_IDENTIFIER ON;
GO
IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = N'SheetLaunch' AND schema_id = SCHEMA_ID(N'dbo'))
CREATE TABLE [dbo].[SheetLaunch] (
    [TicketHash]   binary(32)        NOT NULL CONSTRAINT [PK_SheetLaunch] PRIMARY KEY,
    [ApiKeyId]     nvarchar(64)      NOT NULL,
    [UserId]       nvarchar(50)      NOT NULL,
    [Company]      nvarchar(50)      NOT NULL,
    [Entity]       nvarchar(50)      NOT NULL,
    [Filter]       nvarchar(max)     NULL,     -- JSON
    [Language]     char(2)           NOT NULL,
    [CreatedAt]    datetimeoffset(3) NOT NULL,
    [ExpiresAt]    datetimeoffset(3) NOT NULL,
    [RedeemedAt]   datetimeoffset(3) NULL,
    [RedeemOrigin] nvarchar(200)     NULL
);
GO
IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = N'SheetSession' AND schema_id = SCHEMA_ID(N'dbo'))
CREATE TABLE [dbo].[SheetSession] (
    [TokenHash]     binary(32)        NOT NULL CONSTRAINT [PK_SheetSession] PRIMARY KEY,
    [TicketHash]    binary(32)        NOT NULL,
    [ApiKeyId]      nvarchar(64)      NOT NULL,
    [UserId]        nvarchar(50)      NOT NULL,
    [Company]       nvarchar(50)      NOT NULL,
    [Entity]        nvarchar(50)      NOT NULL,
    [EntityRoute]   nvarchar(100)     NOT NULL,
    [Filter]        nvarchar(max)     NULL,
    [Language]      char(2)           NOT NULL,
    [CreatedAt]     datetimeoffset(3) NOT NULL,
    [IdleExpiresAt] datetimeoffset(3) NOT NULL,
    [MaxExpiresAt]  datetimeoffset(3) NOT NULL,
    [EndedAt]       datetimeoffset(3) NULL,
    INDEX [IX_SheetSession_User] ([UserId], [Company])
);
```

`UserId` is the ERP's user id (the AspNetUsers ShortName in the `UserId` claim), which
`ModifiedBy` holds too. Rows past their expiry are deleted by Track E3's clean-up; they are useless
after `MaxExpiresAt`.

## RowVersion rollout plan

`[Timestamp] public byte[] RowVersion` on the shared `VendorModel` makes EF select and compare
`Vendors.RowVersion` in every query that loads a vendor, directly or through the many vendor
`Include`s, in **every** host built on `CG.Infrastructure`. On a database without the column, those
queries fail. So the order is fixed:

1. **List every database a host reads vendors from.** Track E0 found:

   | Database | Connection names |
   | --- | --- |
   | ASTDataTest | UnifiedAPI's `Kit`, `Dot` and `DataConnection`; Portal's `DataConnection` and `PortalConnection` |
   | Kitchino | UnifiedAPI's `Dat` |
   | ASTData | UnifiedAPI's `AST` |
   | ASTDataNew | Portal's `NewConnection`. Not a UnifiedAPI tenant, and no vendor query was found in Portal, but Portal maps the same model, so script it too |
   | Each Blazor deployment's `cn` | The Blazor host, `D:\CashGear\Projects\assistoWebApp` (`CashGear.App`), runs one deployment per company profile: `Configurations:Kit` and `Configurations:Dat` in its `appsettings.json`, chosen by `ActiveConfiguartionId`. Each reads vendors from its profile's `cn`, set in secrets or the environment and empty in the repo. Inferred: Kit's is ASTDataTest and Dat's Kitchino, as UnifiedAPI maps them. **You confirm each deployment's `cn`** before step 2 |
   | CGService | `DefaultConnection` in every host: identity, and the sheet's session tables; no `Vendors` |

   The hosts are UnifiedAPI, Portal and the Blazor host. The Blazor host builds the ERP's
   libraries by project reference from `..\..\CashGear`, so a `[Timestamp]` commit reaches it at
   its next build from that checkout, not only when UnifiedAPI deploys.
2. **Script every database from step 1** with `Add_VendorsRowVersion.sql`, then
   `Create_SheetCommit.sql`; and CGService with `Create_SheetSessions.sql`. Test first, then each
   tenant. You run them; Claude never touches the live server.
3. **Check** that `Add_VendorsRowVersion.sql`'s last query answers `RowVersionBytes` 8 in every
   database, and `DebitLimit` `18,2` (or tell Track E the scale it shows).
4. **Only then** does `[Timestamp]` go onto `VendorModel`. Track E works on `main` and pushes
   (decisions, 2026-11-09, Phase 14), so that commit waits for your word that step 3 is done. It
   runs the whole ERP test suite and the Blazor host's `CashGear.Blazor.UI.Tests`, whose test
   databases seed vendors (`VendorPagesTests` among them). Tests that seed vendors without
   RowVersion handling may break (inferred in E0: DocumentPrint, FxRevaluation, GroupPostingVM,
   Intercompany, PartyGroupCode, ProjAdvancePosting, PurchOrderSearch and
   PurchReportQueryService).
5. **Deploy** UnifiedAPI, Portal and the Blazor host in any order: all of them read the column now.
6. **New tenants:** whatever creates a tenant database must include the column from then on.

**What changes for the Blazor host.** With the column mapped, EF adds `RowVersion` to each vendor
`UPDATE`'s `WHERE`.
- Its vendor page saves through `VendorDataService.UpdateAsync`
  (`CashGear.App/Services/VendorDataService.cs`), which reads the row and saves it in one context.
  So it fails only when another write lands in between, and then says "Unable to update vendor
  data. Please try again." `UpdateEx` in `CashGear.Operation` reads and saves the same way, and
  returns false with the message.
- No path in `CashGear.App` attaches or updates a vendor loaded earlier (searched for `Update`,
  `Attach` and `Entry` on vendors), so none starts failing where it used to overwrite. Track E2
  tests both save paths on LocalDB before step 4.
- That page edits name, phone, mobile, address, tax file and Blocked with Edit or AdvEdit, where the
  sheet asks AdvEdit for Blocked, and its writes go through no audit. Those are the Blazor host's
  rules, left as they are; its cache sees a sheet commit within 8 hours.

**Rolling back.** Remove `[Timestamp]` from the model and deploy. The column can stay: EF ignores
columns it doesn't map. Never drop the column while a host that maps it is running.

## Running the suite against UnifiedAPI

From Phase 15, the same suite runs against UnifiedAPI on LocalDB, and in Phase 19 on the test
tenant (Kit). Never against the live database.

```bash
ERP_BASE_URL=http://localhost:5000 ERP_TEST_KEY_EDITOR=… ERP_TEST_KEY_ADV=… ERP_TEST_KEY_VIEWER=… ERP_TEST_KEY_NONE=… ERP_TEST_KEY_OTHER=… pnpm vitest run packages/erp-connector
```

| Variable | Meaning |
| --- | --- |
| `ERP_BASE_URL` | The server, without a trailing slash. Unset, the suite starts the mock |
| `ERP_TEST_KEY_EDITOR` | A Kit user with Vendors View and Edit. Needed by nearly every case |
| `ERP_TEST_KEY_ADV` | A Kit user with AdvEdit too |
| `ERP_TEST_KEY_VIEWER` | A Kit user with View only |
| `ERP_TEST_KEY_NONE` | A Kit user without the Vendors unit |
| `ERP_TEST_KEY_OTHER` | An editor in Dot, which shares Kit's database |
| `ERP_TEST_ORIGIN` | The Origin sent, `http://localhost:4173` by default; it must be allow-listed |
| `ERP_TEST_SKIP_KEYS` | Vendor accounts the suite must not write to, comma-separated |
| `ERP_ALLOW_WRITES=1` | Needed when the server isn't on this machine, because the suite writes to the vendors it reads |
| `ERP_BASE_IS_MOCK=1` | The server is the mock started by hand, so its clock and fault cases run too |

A case whose key is missing skips. So do the cases that need the mock's clock (ticket and session
expiry) or its failure injection (lost responses, a locked record, Retry-After); Track E covers
those in its own WebApplicationFactory tests (plan I.4), with its LocalDB cases behind
`CASHGEAR_SHEET_SQL_TESTS=1`, as each ERP suite has its own `CASHGEAR_<AREA>_SQL_TESTS`.

Measured on 2026-11-09 against a mock started by hand: with `ERP_BASE_IS_MOCK=1`, all 55 cases
pass; with only `ERP_TEST_KEY_EDITOR`, as a real server with one key would be run, 34 pass and 21
skip.

## The mock

`pnpm erp:mock` (port 4180) or `node tests/erp-mock/server.mjs --port <port>` starts it by hand;
the suite starts its own on a free port.

- **Data:** one database shared by Kit (2,345 vendors) and Dot (40), interleaved in the id space,
  both with a `V-1001`. Kit's first vendors carry the values the wire rules must keep: Arabic and
  English names, `01001234567`, tax files `001` and `0045`, `15000.00`, empty fields, a blocked
  vendor, a locked one (`V-1005`, mock only: Vendors have no locks in the ERP), an empty currency
  (`V-1007`) and one in an inactive group (`V-1008`).
- **Lookups** are shaped as the ERP's tables are: one description sent as both languages, payment
  terms (`CASH`, `NET30`, `NET60`), and Kit's inactive `OLD` group.
- **Keys:** `mock-kit-editor` (Sara, View and Edit), `mock-kit-adv` (Omar, AdvEdit),
  `mock-kit-viewer` (Mona, View), `mock-kit-none` (no Vendors unit) and `mock-dot-editor` (Karim,
  Dot).
- **Versions:** one 8-byte counter per database, as `rowversion` is, moved by every write.

| Control route | Does |
| --- | --- |
| `POST /__mock/reset` | Back to the seed, with the clock at real time |
| `POST /__mock/clock` `{advanceMs}` or `{set}` | Moves the clock tickets and sessions expire by; answers `{now}` |
| `POST /__mock/fault` `{match, status?, retryAfter?, latencyMs?, drop?, times?}` | The next `times` (1 by default, 0 for every) requests whose path matches the regular expression `match`, ignoring case, wait `latencyMs`, answer `status`, or drop the connection `before` or `after` the server acts. Anchor it: `VendorSheet/Commit$`, since `VendorSheet/Commit` matches CommitStatus too |
| `POST /__mock/edit` `{company, key, values, by}` | Another ERP user's edit, moving the version |
| `POST /__mock/key` `{key, active?, privileges?}` | Revokes a key or changes its privileges |
| `GET /__mock/state?company=` | `{now, vendors, writes, commits, audit}` |
| `GET /__mock/vendor?company=&key=` | One vendor as stored, `Token` and version included |

## Track E0: the contract against UnifiedAPI's code

Reviewed read-only on 2026-11-09 in `D:\CashGear\Projects\CashGear`: files read and `git log`
only, with no build, no restore, no database and no user secrets opened. Nothing there changed.
Each finding names where it was found; "inferred" marks what wasn't confirmed in code.

**Confirmed to fit**
- Avoiding `Authorization`: the API-key handler reads `X-API-Key`, then the raw `Authorization`
  value, then `?api_key=` (`ApiKeyAuthenticationHandler.cs:52-71`), and caches only valid keys
  (`:83-93`), so any other value costs an uncached CGService lookup.
- A second scheme: `AddAuthentication("ApiKey")` (`Program.cs:63`) keeps the API key the default,
  and the `Payable` policy names no scheme (`Program.cs:129`), so
  `[Authorize(AuthenticationSchemes = "SheetSession", Policy = "Payable")]` takes sheet sessions
  only.
- Setting the tenant from an auth handler: `TenantService` takes an explicit value first, then the
  `CompanyId` claim, then `X-Company-Id` on anonymous requests only (`TenantService.cs:26-48`).
- Response caching runs before authorization (`Program.cs:365-367`), and nothing sets public
  caching today; `no-store` plus `[ResponseCache(NoStore = true, Location = None)]` covers it.
- The field list is the eight fields `UpdateEx` writes (`VendorCmdService.cs:171-180`), with
  `VendorModel`'s lengths. `ModifiedBy` holds the user id (ShortName). `Blocked` reads as `!= 0`.
- RowVersion's pattern, `[Timestamp] byte[]` sent as base64 and answered with 409
  (`CustPaymentCmdController.cs:168-179`, `:577-581`, `:637-643`), with 8 bytes as `isVersion`
  expects. The plan's `UPDLOCK, HOLDLOCK` re-read closes that controller's read-then-compare gap.
- `Program` is public, so `WebApplicationFactory<CashGear.UnifiedAPI.Program>` needs no partial
  class.

**Changed in the contract from the review**
- Lookups carry `active: false` for inactive items (`VendorsGroup.Active`); a change may not
  choose one, and a record may keep one. Their labels are one description, sent as both languages.
- `payMethod` is `PaymentModes (Company, PaymentId)`: payment terms with `PaymentDays`, not cash
  or bank. The mock's codes follow.
- `currency` is required and checked against `LedgerCurrencies (Company, Code)`, as `Save` checks
  it (`VendorCmdService.cs:137-141`).
- `dateTime` keeps its offset: the ERP sends its local `DateTime.Now` with the host's offset, and
  the sheet shows that wall-clock time (decisions, Phase 14).
- `commitId` is a string in the DTOs, and the guards take a `null` optional property as absent.
- Vendors have no ownership or row locks: `SysSecRules` is a class nothing reads,
  `Vendors.SecRule` is never read, and `LoadOwnedRowsPurch` filters purchase orders only
  (`ShowPOVM.cs:134-143`). `record.locked` stays reserved.
- Cache: `RemoveByTag("Vendors")`, since `{Company}:Vendors` is a key, not a tag
  (`VendorQueryService.cs:119, 143, 174, 198`).
- CGService's scripts go in `CashGear.IdentitySystem/DbScripts`, read through a UnifiedAPI-only
  `SheetDbContext`; script names follow the ERP's `<Verb>_<Subject>.sql`.
- CreateTicket's bad key gets the API-key handler's empty 401; its 403 is the action's own.

**Corrections to the plan**
- `CallerPrivileges` falls back to another company's privilege row for every non-admin user, not
  only Admin (`CallerPrivileges.cs:41`). The sheet uses a company-strict check.
- Start with a concrete `VendorSheetConnector`: one implementation of `ISheetEntityConnector` goes
  against `CashGear.Operation/CLAUDE.md`'s rule of no abstraction before a second consumer.
- `ERPClient_React` is its own repository (`D:\CashGear\Projects\ERPClient_React`, `main`), not a
  folder of the ERP repo. It reads `payload.message`, which every error body has.

**For E1 (tickets, sessions, Schema, Query)**
- The SheetSession handler emits the API-key handler's claims (`:143-170`) except the raw key, plus
  the entity scope, and overrides `HandleChallengeAsync` and `HandleForbiddenAsync`: the default
  ones write only a status. Each writes the `ErrorBody`, `X-Correlation-Id` and `no-store`.
- Revocation per request: the key cache is per process and only `ApiKeysController.Deactivate`
  evicts it, so read `APIKeys` by `ApiKeyId` (`IsActive`, `ExpiresDate`) on each request or with a
  cache of 60 s at most, and the company with `TenantRegistry.Find`, answering 401 rather than
  `UnknownTenantException`'s 400. Decide whether roles are taken at Redeem or read again.
- Errors: `ItemUpdateErrorResponse` has no `code`, `id` or `key`, and
  `GlobalExceptionHandlerMiddleware` writes `{statusCode, message, errorCode}`, so the sheet
  controllers get the new DTOs and an exception filter. Both correlation and company headers are
  set per action today; set them on every sheet response from `TraceIdentifier`.
- Privileges: read View, Edit and AdvEdit for unit 2004 in one query; `admin` passes everything.
- Test host: the real `Program` opens the Kit tenant at start-up (`Program.cs:342-352`), runs
  dimension maintenance for Dat every 5 s, loads user secrets in Development, and reads
  `DefaultConnection` before `Build`. Run it as "Testing" with settings from environment
  variables, remove the hosted services, replace `ApplicationDbContext`, `IActiveCompanySource`
  and `IDbContextFactory<CashGearDbContext>`, and guard the warm-up.
- CORS: put `[EnableCors("SheetOrigins")]` on the sheet controllers from E1, since the suite checks
  preflight; an endpoint's policy answers the preflight before authentication. Redeem checks
  `Origin` itself, since CORS doesn't refuse requests.

**For E2 (Commit)**
- Don't reuse `UpdateEx`: it is last-write-wins, has no transaction and swallows exceptions.
  `ValidateSave` returns only its first error, in Arabic.
- MARS is on everywhere, so no savepoints. Roll back with `RollbackAsync(CancellationToken.None)`
  inside try/catch (`MasterCodeService.cs:150-160`), and track whether the commit was attempted,
  as `CustomerMaintenanceService` does, so an unknown outcome sends the sheet to CommitStatus.
  `SystemLog.Msg` holds 250 characters.
- The sheet's SQLite test context must stamp a new 8-byte RowVersion on every vendor write
  (`SqliteRowVersionContext` freezes it), or conflicts never show. `UPDLOCK` is SQL Server only, so
  conflicts and idempotency are proven on LocalDB too. Dry runs read without locks.

**For E4 (rate limits)**
- Nothing exists. Use `AddRateLimiter` with `RejectionStatusCode = 429` (503 is the default), the
  `ErrorBody` and `Retry-After` in `OnRejected`, after `UseCors`, partitioned on
  `RemoteIpAddress`: hosting is in-process IIS, and `X-Forwarded-For` comes from the caller.
