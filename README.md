# sqlgate

Web-based SQL query preview and approval gateway. Submit a query, get a Polars-backed row-limited preview, request approval, then execute — all through an HTMX + Alpine.js frontend behind Cloudflare Access.

## Architecture

```
Browser ──▶ cloudflared ──▶ sqlgate (Rust, std::net) ──▶ Postgres (persistence)
                │                        │
        Cloudflare Access         ┌──────┴──────┐
        (identity + tunnel)       │  preview DB  │ (read-only role)
                                  │  execute DB  │ (read-write role)
                                  └──────────────┘
```

- **HTTP server**: hand-rolled `std::net::TcpListener` — no axum, hyper, or actix
- **Frontend**: HTMX v4 + Alpine.js v3, Tailwind CSS v4 standalone (no npm)
- **Preview engine**: Polars `LazyFrame` with `LIMIT 5` wrap — never pulls full tables
- **Persistence**: Postgres for requests, approvals, audit log
- **Auth**: delegated entirely to Cloudflare Access via shared-secret tunnel header

## Stack

| Layer | Technology |
|---|---|
| Runtime | Rust (edition 2024) |
| HTTP | `std::net::TcpListener`, hand-rolled HTTP/1.1 parser |
| Frontend | HTMX v4.0.0-beta5 + Alpine.js 3.14.9 + Tailwind CSS v4.1.18 |
| Query preview | Polars (lazy, row-limited) |
| Persistence | PostgreSQL (hand-written parameterized SQL) |
| Auth | Cloudflare Access + tunnel shared-secret header |
| Container | Multi-stage Dockerfile → `distroless/cc-debian12:nonroot` (no shell) |

Zero framework dependencies: no axum, hyper, actix-web, tokio, ORM, or async runtime.

## Quick Start

```bash
# Prerequisites: Rust toolchain, Docker

# Install toolchain + download Tailwind standalone + vendor JS
make setup

# Start databases (Postgres + MySQL targets + sqlgate persistence)
make db-up

# Build + run
make dev
DATABASE_URL="postgres://sqlgate:sqlgate@localhost:5432/sqlgate" \
CF_TUNNEL_SECRET_VALUE=supersecret \
cargo run

# Smoke test
curl http://localhost:8080/health      # → "ok"
curl http://localhost:8080/            # → full HTML page
curl -H "HX-Request: true" localhost:8080/  # → fragment only
```

## Environment Variables

See `.env.example` for the full list. Key variables:

| Variable | Default | Purpose |
|---|---|---|
| `LISTEN_ADDR` | `0.0.0.0:8080` | HTTP bind address |
| `DATABASE_URL` | — | Postgres persistence DSN |
| `CF_TUNNEL_SECRET_HEADER` | `X-CF-Tunnel-Secret` | Cloudflare tunnel shared-secret header name |
| `CF_TUNNEL_SECRET_VALUE` | — | Shared-secret value set by cloudflared |
| `TARGET_<name>_PREVIEW` | — | Preview role connection (read-only) |
| `TARGET_<name>_EXECUTE` | — | Execute role connection (read-write) |
| `APPROVAL_TTL_SECONDS` | `900` | Approval expiry (15 min) |

## Development

```bash
make help        # Show all targets
make setup       # Install tools + download Tailwind/Alpine/HTMX
make dev         # Debug build
make test        # Run all tests
make fmt         # Format code
make lint        # Clippy with -D warnings
make build       # Release build + Tailwind CSS
make clean       # Remove build artifacts
make db-up       # Start databases via docker compose
make db-down     # Stop databases (keeps volumes)
make db-reset    # Tear down and rebuild from scratch
```

## Design Tokens

Parchment/cream background with rust accents:

| Token | Hex | Usage |
|---|---|---|
| `parchment` | `#fdf6e3` | Page background |
| `parchment-dark` | `#f5e6c8` | Cards, nav |
| `rust` | `#b7410e` | Buttons, links, accents |
| `rust-light` | `#d4723a` | Hover states |
| `amber` | `#e8a84a` | Highlights |
| `ink` | `#2c1810` | Body text |
| `ink-muted` | `#6b5b4f` | Secondary text |

## Project Phases

| # | Phase | Status |
|---|---|---|
| 0 | Project Scaffolding | ✅ Done |
| 1 | Raw HTTP Server Core | ✅ Done |
| 2 | Static Assets & Tailwind Pipeline | ✅ Done |
| 3 | Base Layout & HTMX/Alpine Conventions | ✅ Done |
| 4 | Postgres Persistence Layer | ✅ Done |
| 5 | Cloudflare Access Identity Trust | ✅ Done |
| 6 | Query Submission | ✅ Done |
| 7 | Preview Engine | ✅ Done |
| 8 | Approval Workflow | ✅ Done |
| 9 | Execution Engine | ✅ Done |
| 10 | Audit Trail & History Views | ✅ Done |
| 11 | Polish | ✅ Done |
| 12 | Testing & Hardening | ✅ Done |
| 13 | Packaging & Deployment | ✅ Done |

## Deployment

### Docker Compose (full stack)

```bash
# Build and start everything: sqlgate + Postgres + MySQL targets
make docker-up

# Stop
make docker-down
```

### Cloudflare Access + Tunnel

1. Create a Cloudflare Tunnel pointing at `sqlgate:8080` in your docker network.
2. Set the tunnel shared-secret header in Cloudflare Access policy.
3. Uncomment the `cloudflared` sidecar in `docker-compose.yaml` and set `CF_TUNNEL_TOKEN`.

```yaml
# In docker-compose.yaml, uncomment:
cloudflared:
  image: cloudflare/cloudflared:latest
  command: tunnel run --token ${CF_TUNNEL_TOKEN}
```

### Required env vars for production

| Variable | Example |
|---|---|
| `DATABASE_URL` | `postgres://sqlgate:password@host:5432/sqlgate` |
| `CF_TUNNEL_SECRET_VALUE` | Shared secret set in cloudflared config |
| `TARGET_<db>_PREVIEW` | Read-only preview role connection |
| `TARGET_<db>_EXECUTE` | Read-write execute role connection |

### Database role setup

Run the init scripts from `docker/` against your target databases:

```sql
-- Preview role (read-only)
CREATE ROLE sqlgate_preview WITH LOGIN PASSWORD 'preview' NOSUPERUSER;
GRANT CONNECT ON DATABASE mydb TO sqlgate_preview;
GRANT USAGE ON SCHEMA public TO sqlgate_preview;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO sqlgate_preview;

-- Execute role (read-write)
CREATE ROLE sqlgate_execute WITH LOGIN PASSWORD 'execute' NOSUPERUSER;
GRANT CONNECT ON DATABASE mydb TO sqlgate_execute;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO sqlgate_execute;
```
## Security Model


- **Query hash verification**: re-hash `query_text` from Postgres at execute time — never trust client-supplied hash
- **Separate DB roles**: `sqlgate_preview` (read-only) vs `sqlgate_execute` (read-write) — different connections, different credentials
- **LIMIT wrap in Rust**: `SELECT * FROM (<query>) sub LIMIT 5` applied server-side before Polars touches rows
- **Approval TTL**: lazy-checked on execute, not solely via background sweep — expired approvals can never execute
- **CF Access tunnel guard**: requests without the shared-secret tunnel header are rejected, even if they forge the CF email header
- **Audit log**: append-only, no update/delete paths anywhere in the codebase

---

Built with omp + deepseek-v4-pro. Token cost so far: **$1.03**.
