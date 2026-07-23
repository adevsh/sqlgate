---
name: sqlgate
description: >
  Build guide for sqlgate — a web-based SQL query preview and approval gateway.
  A requester submits a raw SQL query through a web UI; the query is hashed
  and stored, executed as a lazy, row-limited preview (via Polars) against
  either the primary or a replica target, and shown to an approver before it
  is allowed to run for real. Use this skill whenever building, extending, or
  debugging any part of sqlgate: the raw std::net HTTP server, HTMX
  fragment/OOB routing, Alpine.js sprinkles, the Postgres persistence layer,
  the MySQL/PostgreSQL target-database drivers, Cloudflare Access identity
  trust, or the preview/approve/execute state machine. Rust standard library
  HTTP only — no axum/hyper/actix. Auth is delegated entirely to Cloudflare
  Access; the app never manages passwords or sessions.
---

# sqlgate — SQL Query Preview & Approval Gateway

A self-hosted web tool that gates ad-hoc SQL execution behind a human
approval step, with a real data preview (not just a syntax check) shown
before that approval is granted. Inspired by the Atlantis PR-comment
plan/apply pattern, but web-based since tabular data previews don't render
well in PR comments.

Flow: submit query → hash + persist → run row-limited preview (Polars lazy
scan) against primary or replica → approver reviews the actual preview rows
→ approve or reject → on approval, re-verify the query hash and execute for
real against the target database → log everything.

---

## Stack

| Layer            | Technology                                              |
|-------------------|----------------------------------------------------------|
| Language          | Rust (2021 edition)                                       |
| HTTP server       | `std::net::TcpListener` + hand-rolled HTTP/1.1 parsing (no axum/hyper/actix) |
| Frontend behavior | HTMX (server round trips) + Alpine.js (local-only UI state) |
| Styling           | Tailwind CSS (CLI build, no JS framework)                 |
| Persistence       | PostgreSQL (hand-written SQL, no ORM)                     |
| Preview engine    | Polars (Rust), lazy scan, row-limited                      |
| Target DBs        | PostgreSQL, MySQL (drivers used directly — no ORM)         |
| Auth              | Cloudflare Access (fronts the Cloudflare Tunnel) — app trusts verified identity header only |
| Distribution      | Docker container behind the existing Cloudflare Tunnel     |
| Build             | Makefile with standard targets                              |
| Commit convention | Conventional Commits                                          |

---

## Why std::net instead of a web framework

Explicitness over abstraction. A hand-rolled HTTP/1.1 parser over
`TcpListener` means every request-parsing decision (headers, chunked bodies,
keep-alive, routing) is visible and owned by the project instead of hidden
in framework internals. This is a deliberate constraint, not a default —
confirm before ever adding `axum`, `hyper`, `actix-web`, or similar.

## Why Cloudflare Access instead of in-app auth

The app is already reachable only through a Cloudflare Tunnel (same pattern
as `miso`). Cloudflare Access sits in front of the tunnel and handles
SSO/identity before a request ever reaches the Rust binary. The app reads
and trusts the `Cf-Access-Authenticated-User-Email` header (after verifying
the request actually came through the tunnel, not the public internet
directly) instead of owning passwords, sessions, or an OAuth handshake. This
also avoids needing a TLS client crate purely to talk to an OAuth provider,
which would violate the "no external HTTP lib" constraint.

## Why no ORM

Schema-first, hand-written SQL against Postgres. Query strings live next to
the Rust functions that run them, are reviewable in diffs, and never
generate SQL the author didn't write. Matches the project-wide no-ORM rule.

---

## Architecture

```
Browser (HTMX + Alpine.js, Tailwind)
        |
        v
Cloudflare Access  (SSO / identity check)
        |
        v
Cloudflare Tunnel
        |
        v
cloudflared container
        |
        v
sqlgate HTTP server (std::net, Rust)
        |
        |-- Persistence: Postgres (requests, approvals, executions, audit_log)
        |
        |-- Preview path: read-only DB role --> Polars lazy scan --> top-5 rows
        |                  (target: primary or replica, chosen at submission)
        |
        `-- Execute path: privileged DB role --> real query execution
                           (only reachable after hash-verified approval)
```

## Request Lifecycle (state machine)

```
submitted --> previewed --> pending_approval --> approved --> executed
                                    |
                                    `--> rejected (terminal)

approved --> expired   (if not executed within TTL; must resubmit)
```

Each state transition is a row insert/update in Postgres, never a mutation
of the original submitted query text. The submitted query is immutable once
created — a requester who wants to change the query creates a new request.

---

## Repository Layout

```
sqlgate/
|- Makefile
|- Dockerfile
|- .env.example
|- Cargo.toml
|- src/
|  |- main.rs                 # TcpListener accept loop, thread/task dispatch
|  |- http/
|  |  |- request.rs           # raw HTTP/1.1 request parsing
|  |  |- response.rs          # response writer, status lines, headers
|  |  `- router.rs            # path + method -> handler dispatch
|  |- auth/
|  |  `- cf_access.rs         # header trust + tunnel-origin verification
|  |- db/
|  |  |- schema.sql           # source of truth for the Postgres schema
|  |  |- requests.rs          # hand-written SQL: create/read requests
|  |  |- approvals.rs         # hand-written SQL: approve/reject/expire
|  |  |- executions.rs        # hand-written SQL: record execution results
|  |  `- audit.rs             # hand-written SQL: append-only audit log
|  |- targets/
|  |  |- mod.rs               # target DB abstraction (Postgres | MySQL)
|  |  |- postgres_target.rs
|  |  `- mysql_target.rs
|  |- preview/
|  |  |- validator.rs         # SQL parse + SELECT-only allow-list
|  |  |- wrapper.rs           # forces LIMIT via subquery wrap, never trusts user LIMIT
|  |  `- engine.rs            # Polars lazy scan against preview role/connection
|  |- execute/
|  |  |- hash.rs              # SHA-256 query hashing + comparison
|  |  `- engine.rs            # privileged-role execution, transaction handling
|  |- templates/              # server-rendered HTML fragments (HTMX targets)
|  `- state.rs                # shared app state (pool handles, config)
|- static/
|  |- tailwind.css            # built output
|  |- tailwind.input.css
|  `- alpine.min.js           # vendored, not npm-fetched at runtime
`- tests/
   |- integration/
   `- fixtures/
```

---

## Database Schema (Postgres, hand-written — source of truth in `db/schema.sql`)

```sql
CREATE TABLE requests (
    id                BIGSERIAL PRIMARY KEY,
    requester_email   TEXT NOT NULL,
    query_text        TEXT NOT NULL,
    query_hash        TEXT NOT NULL,          -- sha256(query_text)
    target_kind       TEXT NOT NULL,          -- 'postgres' | 'mysql'
    target_topology   TEXT NOT NULL,          -- 'primary' | 'replica'
    target_database   TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'submitted',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE previews (
    id                BIGSERIAL PRIMARY KEY,
    request_id        BIGINT NOT NULL REFERENCES requests(id),
    row_count         INT NOT NULL,
    preview_json      JSONB NOT NULL,         -- top-5 rows, column-named
    ran_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    duration_ms       INT NOT NULL
);

CREATE TABLE approvals (
    id                BIGSERIAL PRIMARY KEY,
    request_id        BIGINT NOT NULL REFERENCES requests(id),
    approver_email    TEXT NOT NULL,
    decision          TEXT NOT NULL,          -- 'approved' | 'rejected'
    decided_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at        TIMESTAMPTZ            -- set only when decision = approved
);

CREATE TABLE executions (
    id                BIGSERIAL PRIMARY KEY,
    request_id        BIGINT NOT NULL REFERENCES requests(id),
    executed_query_hash TEXT NOT NULL,       -- re-hashed at execute time
    hash_matched      BOOLEAN NOT NULL,
    rows_affected     BIGINT,
    executed_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    error_message     TEXT
);

CREATE TABLE audit_log (
    id                BIGSERIAL PRIMARY KEY,
    request_id        BIGINT REFERENCES requests(id),
    actor_email       TEXT NOT NULL,
    action            TEXT NOT NULL,          -- 'submitted' | 'previewed' | 'approved' | 'rejected' | 'executed' | 'expired'
    detail            JSONB,
    at                TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

`audit_log` is append-only — never update or delete rows, only insert.

---

## Security Model (non-negotiable guardrails)

- [ ] **Two DB roles per target**, never one:
  - [ ] `sqlgate_preview` — read-only grant, used only for the preview path
  - [ ] `sqlgate_execute` — full grant, used only after an approval is hash-verified
- [ ] **Query hash integrity**: SHA-256 of `query_text` stored at submission; re-computed
      immediately before execution; execution aborts on any mismatch
- [ ] **Never trust a user-supplied LIMIT**: preview always wraps the submitted
      query as `SELECT * FROM (<query>) sub LIMIT 5`
- [ ] **Statement timeout enforced at the connection/session level** for the
      preview role (Postgres `SET statement_timeout`, MySQL `MAX_EXECUTION_TIME`)
- [ ] **Preview-only allow-list**: parse the submitted query and reject
      non-`SELECT` statements before ever running the preview path
- [ ] **Approval TTL**: an `approved` request not executed within its `expires_at`
      transitions to `expired` and must be resubmitted — never silently
      re-approved
- [ ] **Immutable request text**: no update path exists for `requests.query_text`
      after creation
- [ ] **CF Access origin check**: verify the request actually arrived through the
      Cloudflare Tunnel (e.g. a shared secret header only `cloudflared` sets)
      before trusting the identity header, so the identity header can't be
      spoofed by a request that reaches the container directly

---

## Code Commenting Standards (mandatory)

- File-level module doc: `//!` at the top of every `.rs` file, describing the
  module's responsibility and its position in the request lifecycle
- Struct/enum doc: `///` on every public struct and enum, one-sentence summary
- Function doc: `///` on every public fn, with `# Errors` documenting every
  `Result::Err` path and `# Panics` documenting any panic condition
  (there should be very few — prefer `Result` throughout, especially in
  `execute/` and `db/`)
- Inline comments (`//`) explain **why**, not what — e.g. why a query is
  wrapped in a subquery, why a role switch happens at a specific point, why
  a hash comparison uses constant-time equality
- Test doc: every `#[test]` function gets a `///` doc line stating what
  guarantee it protects (e.g. "hash mismatch between preview and execute
  must abort execution") plus an inline checklist comment for what an agent
  extending the test should verify

---

## Phases

### Phase 0 — Project Scaffolding

- [x] `git init`, adopt Conventional Commits from the first commit
- [x] `Cargo.toml` with workspace metadata (edition 2024)
- [x] `Makefile` with standard targets: `build`, `run`, `test`, `fmt`, `lint`,
      `db-migrate`, `docker-build`, `docker-up`, `docker-down`
- [x] `.env.example` documenting every required env var (DB DSNs, CF Access
      shared-secret header name, target DB connection strings, preview/execute
      role credentials)
- [x] `.gitignore` (target/, .env, static/tailwind.css build output)
- [x] `Dockerfile` — multi-stage build, minimal runtime base image
- [x] This `SKILL.md` committed at repo root
- [x] Confirm no `axum`/`hyper`/`actix-web`/ORM crate appears anywhere in `Cargo.toml`
---

### Phase 1 — Raw HTTP Server Core

- [x] `src/http/request.rs`: parse request line, headers, and body from a
      `TcpStream` without any external HTTP-parsing crate
  - [x] Support `Content-Length` bodies; explicitly reject or handle chunked
        transfer-encoding (decide and document the decision)
  - [x] Parse query string and form-encoded body separately from JSON bodies
- [x] `src/http/response.rs`: status line + header writer, helpers for
      `text/html`, `application/json`, and HTMX-specific headers
      (`HX-Trigger`, `HX-Redirect`, `HX-Reswap` as needed)
- [x] `src/http/router.rs`: method + path -> handler table; support path
      params (e.g. `/requests/:id`)
- [x] `src/main.rs`: `TcpListener::bind`, accept loop, one OS thread per
      connection (or a small thread pool) — document the concurrency model
      chosen and why
- [x] Graceful shutdown on SIGTERM (needed for clean Docker stop)
- [x] Test: malformed request line does not panic the server
- [x] Test: request with `Content-Length` larger than actual body does not hang forever (enforce a read timeout)

---

### Phase 2 — Static Assets & Tailwind Pipeline

- [x] `static/tailwind.input.css` with project design tokens (parchment/cream
      with rust accents, per the established web-UI palette)
- [x] Tailwind CLI build step wired into `make build` (`npx tailwindcss` or
      the standalone Tailwind binary — decide and document; avoid a JS
      runtime dependency if the standalone binary suffices)
- [x] Static file handler in the router for `/static/*`, with correct
      `Content-Type` per extension and basic caching headers
- [x] Vendor Alpine.js as a static file (`static/alpine.min.js`) — no CDN
      dependency at runtime
- [x] Test: static handler rejects path traversal (`/static/../../etc/passwd`)
---

### Phase 3 — Base Layout & HTMX/Alpine Conventions

- [x] Reuse the fragment/OOB swap patterns already documented in
      `htmx-adminlte-ref` — port the conventions, not the Bootstrap/AdminLTE
      markup
- [x] `templates/layout.html` shell: nav, content slot, HTMX config
      (`hx-boost` where appropriate)
- [x] Establish the fragment-detection convention: full page on direct GET,
      fragment-only response when `HX-Request` header is present
- [x] Define OOB swap targets for: status badges (submitted/previewed/
      pending/approved/rejected/executed/expired) so multiple UI elements can
      update from one response
- [x] Alpine.js scope reserved for: textarea enhancements, copy-to-clipboard,
      collapsible panels — nothing that requires a server round trip
- [x] Test: fragment vs full-page response toggles correctly on `HX-Request`
---

### Phase 4 — Postgres Persistence Layer

- [x] `db/schema.sql` committed as source of truth; `make db-migrate` applies
      it (plain `psql -f`, no migration framework unless a clear need
      emerges)
- [x] `db/requests.rs`: `insert_request`, `get_request`, `list_requests`
      (hand-written SQL, parameterized — never string-concatenated)
- [x] `db/approvals.rs`: `insert_approval`, `expire_stale_approvals`
- [x] `db/executions.rs`: `insert_execution`
- [x] `db/audit.rs`: `append_audit_event` (insert-only, no update/delete path
      exposed anywhere in the codebase)
- [x] Connection pooling: minimal hand-rolled pool or a single-purpose crate
      — document the choice; this is not the HTTP layer, so a DB driver
      crate is expected and fine
- [x] Test: every table has a passing round-trip insert + read test
- [x] Test: `audit_log` has no code path that updates or deletes existing rows
---

### Phase 5 — Cloudflare Access Identity Trust

- [x] `auth/cf_access.rs`: extract `Cf-Access-Authenticated-User-Email`
- [x] Verify the request path actually came through `cloudflared` — require
      a shared-secret header set only by the tunnel config, reject direct
      requests that lack it
- [x] Reject any request missing a verified identity before it reaches a
      handler that mutates state (submit/approve/reject/execute)
- [x] Read-only preview browsing may be allowed more loosely if desired —
      decide and document the boundary explicitly
- [x] Test: request without the tunnel shared-secret header is rejected even
      if it forges the CF Access email header
- [x] Test: request with both headers correctly attributes the identity to
      the resulting audit log row
---

### Phase 6 — Query Submission

- [x] Submission form: query text, target kind (Postgres/MySQL), target
      database name, target topology (primary/replica toggle)
- [x] `preview/validator.rs`: parse the submitted SQL and reject anything
      that isn't a single `SELECT` statement for the preview path
- [x] On submit: compute `query_hash`, insert `requests` row with
      `status = 'submitted'`, write an `audit_log` entry
- [x] Reject empty queries, queries above a configured max length, and
      multi-statement submissions (`;`-separated stacked queries)
- [x] Test: non-SELECT submission is rejected with a clear error fragment
- [x] Test: stacked-query submission (`SELECT ...; DROP TABLE ...`) is rejected

---

### Phase 7 — Preview Engine

- [x] `targets/postgres_target.rs` and `targets/mysql_target.rs`: open a
      connection using the `sqlgate_preview` role only
- [x] `preview/wrapper.rs`: wrap the validated query as
      `SELECT * FROM (<query>) sub LIMIT 5` regardless of any LIMIT the user wrote
- [x] Set a statement/session timeout on the preview connection before running
- [x] `preview/engine.rs`: run the wrapped query, load results, serialize to
      `previews.preview_json` (no Polars — direct postgres→serde_json)
- [x] Respect `target_topology`: connect to the replica endpoint when toggled,
      primary otherwise — store which one was actually used
- [x] Update `requests.status` to `previewed`, write an `audit_log` entry with
      row count and duration
- [x] Render the preview as an HTMX fragment: table of rows + row count +
      target topology badge
- [x] Test: preview against a query with an embedded `LIMIT 999999` still
      returns at most 5 rows
- [x] Test: preview role cannot execute DDL/DML even if a non-SELECT slips
      past the validator (defense in depth)
- [x] Test: preview times out and surfaces a clear error rather than hanging
      the request thread

---

### Phase 8 — Approval Workflow

- [x] Approver-facing view: pending requests list, each showing requester,
      submitted query, the stored preview, target topology
- [x] Approve action: insert `approvals` row with `decision = 'approved'`,
      set `expires_at` (configurable TTL, e.g. 15 minutes), update
      `requests.status = 'pending_approval'` -> `approved`
- [x] Reject action: insert `approvals` row with `decision = 'rejected'`,
      update `requests.status = 'rejected'` (terminal state)
- [x] Background sweep (or lazy check on access): any `approved` request past
      `expires_at` transitions to `expired`, writes an `audit_log` entry
- [x] Prevent an approver from approving their own submitted request
      (compare `requester_email` vs the CF Access identity performing the
      approval)
- [x] Test: approving an already-expired request is rejected
- [x] Test: a requester cannot approve their own request
- [x] Test: rejected requests cannot later be approved

---

### Phase 9 — Execution Engine

- [ ] `execute/hash.rs`: re-hash the stored `query_text` immediately before
      execution; compare against the hash stored at submission using a
      constant-time comparison
- [ ] Abort execution (and log `hash_matched = false`) on any mismatch —
      never silently proceed
- [ ] Open the target connection using the `sqlgate_execute` role only,
      and only after hash verification succeeds
- [ ] Wrap execution in an explicit transaction; commit only on success
- [ ] Record `rows_affected`, `executed_query_hash`, `hash_matched`, and any
      `error_message` in `executions`
- [ ] Update `requests.status = 'executed'`, write a final `audit_log` entry
- [ ] Test: a request whose stored `query_text` is somehow altered between
      approval and execution fails closed (hash mismatch aborts)
- [ ] Test: execution failure (e.g. constraint violation) rolls back cleanly
      and is recorded with `error_message` populated

---

### Phase 10 — Audit Trail & History Views

- [ ] Request detail view: full timeline (submitted → previewed → approved/
      rejected → executed/expired) sourced entirely from `audit_log`
- [ ] Filterable history list: by requester, approver, status, target database
- [ ] Read-only — no view in the app ever allows editing or deleting an
      audit entry
- [ ] Test: history view timeline matches `audit_log` insertion order exactly

---

### Phase 11 — Polish

- [ ] Apply the parchment/cream + rust-accent palette consistently across
      all templates
- [ ] Status badges with consistent color coding across list and detail views
- [ ] Loading state on preview run (HTMX indicator) since preview may take a
      few seconds against a real database
- [ ] Empty states: no pending approvals, no history yet
- [ ] Copy-to-clipboard for query text (Alpine.js, no server round trip)
- [ ] Confirm dialogs (Alpine.js) before reject and before execute-adjacent
      approve action

---

### Phase 12 — Testing & Hardening

- [ ] Integration test suite spins up a real Postgres (and MySQL, if in
      scope for tests) via Docker for realistic driver-level testing
- [ ] Load test: verify the raw `std::net` server handles concurrent
      connections without dropping requests (document the concurrency model
      limits discovered)
- [ ] Security test pass covering every item in the **Security Model**
      section above, each as an explicit automated test, not just a manual
      check
- [ ] Fuzz or property-test the HTTP request parser against malformed input
- [ ] Confirm `sqlgate_preview` role's actual grants in a real database
      (not just assumed) — write a test that attempts a write through the
      preview connection and expects a permission-denied error

---

### Phase 13 — Packaging & Deployment

- [ ] `Dockerfile`: multi-stage, minimal runtime image, no shell in the
      final stage if feasible (mirroring the `ubi-micro` precedent from `miso`)
- [ ] `docker-compose.yml` (or reuse `miso`'s `cloudflared` sidecar pattern)
      wiring sqlgate + `cloudflared` together
- [ ] Wire Cloudflare Access policy for the tunnel hostname before go-live
- [ ] Document the required GitHub-adjacent-free deployment steps in README:
      env vars, DB role creation SQL, Cloudflare Access policy setup
- [ ] `make docker-build` / `make docker-up` / `make docker-down` verified
      end to end
- [ ] Verify container has no shell in the runtime stage (or document why one
      was kept, if it was)

---

## Key Implementation Notes for Agent

**Query hash must be recomputed, never trusted from client state.** Always
re-hash `requests.query_text` fetched fresh from Postgres at execute time —
never trust a hash passed in from a form field or a prior in-memory value.

**Preview and execute must use physically different DB roles/credentials.**
Do not reuse one connection pool for both paths, even for convenience during
early development — this is a security boundary, not an optimization detail.

**The LIMIT wrap happens in Rust, not by asking Polars to `.limit(5)` after
a full table scan.** Polars' lazy evaluation helps with local processing
once rows are returned, but the actual bytes pulled over the wire from
Postgres/MySQL must already be limited by the wrapped SQL — otherwise a
"preview" of a huge table is exactly as expensive as running it for real.

**Approval TTL expiry should be checked lazily on read, not solely via a
background timer**, so that an expired-but-unswept request can never be
executed even if a sweep job hasn't run yet — check `expires_at` again at
the top of the execute handler regardless of `requests.status`.

**CF Access header trust is only as strong as the tunnel-origin check.**
If the shared-secret-header check in `auth/cf_access.rs` is ever removed
or weakened, the entire auth model collapses to "trust any header the
client sends" — treat that check as load-bearing, not incidental.

**Concurrency model for the raw `std::net` server should be decided once
and documented**, not left implicit. Thread-per-connection is the simplest
correct option for a small internal tool's expected load; only move to a
more complex model (thread pool with a queue, or `mio`-based non-blocking
I/O) if load testing in Phase 12 actually shows a need.
