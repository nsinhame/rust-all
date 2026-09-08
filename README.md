# File Review System (Rust)

A Rust rewrite of the original Flask file-review app, built with [Axum](https://github.com/tokio-rs/axum),
[Askama](https://askama.rs) templates and the official [MongoDB driver](https://www.mongodb.com/docs/drivers/rust/).
Designed to run comfortably within Koyeb's free tier (single small instance, low memory).

## Features

The whole app is served under the `/link-review` prefix, and every request to it requires a
shared secret (see "Access gate" below) — e.g. `https://your-host/link-review?key=<ACCESS_KEY>`.

Functionally equivalent to the original app:

- Session-based login for reviewers configured via `LINK_REVIEW_USER{n}` / `LINK_REVIEW_USER{n}_PASS` env vars.
- `/link-review/review-plgb` — 10 random unreviewed files with search (user id include/exclude, file name, forwarded-from, size range, Mongo `_id`) and live stats sidebar.
- `/link-review/submit` — bulk accept/reject, optional rename, optional "special hash" tagging.
- `/link-review/done-plgb` — paginated list of reviewed files with the same filters plus reviewer/status.
- `/link-review/stats-plgb` — dashboard with global + per-reviewer comparison (top 2 configured users).
- `/link-review/instructions-plgb` — static help page (search tips + 50 regex rename examples).

A second, independent review section covers the **TGFS** (telethon-plgb) multi-bot/multi-database
tgfilestream bot, joining its separate "index" (`user_files`) and "blob" (`files`) Mongo clusters
in-process since they can't be `$lookup`-joined server-side (see
`/memories/repo/tgfs-review-integration.md` for the full architecture writeup):

- `/link-review/review-tgfs` — random pending (never-reviewed) links, filterable by user id, **bot id**, file name, forwarded-from, size, date, and Telegram file id. Every card shows which bot generated it.
- `/link-review/submit-tgfs` — bulk accept (unrestrict)/reject (restrict)/delete/delete+warn, optional rename (kept in sync on both the index and blob doc). Delete+warn increments the uploader's `warns` count (banning is handled separately by the telethon-plgb bot itself, not by this tool).
- `/link-review/done-tgfs` — paginated list of already-reviewed links with the same filters plus reviewer/status.
- `/link-review/stats-tgfs` — dashboard with global + per-reviewer comparison, plus a per-bot "links generated" breakdown.
- `/link-review/instructions-tgfs` — static help page covering the multi-bot/multi-DB differences from PLGB.

## Project layout

The old Flask app lives in `link-allow-page/` (gitignored, kept locally for reference only).
Everything below lives at the repository root:

```
├── Cargo.toml
├── Dockerfile
├── src/
│   ├── main.rs               # entrypoint, router, default users, index creation, TGFS Mongo connect
│   ├── state.rs               # AppState (Mongo collections, cookie key) + TgfsState
│   ├── auth.rs                 # session/flash cookies, password hashing
│   ├── models.rs               # BSON helpers + view models used by PLGB templates
│   ├── query_filters.rs        # shared search-filter parsing (PLGB review/done)
│   ├── util.rs                 # formatting helpers, template render helper
│   ├── tgfs_models.rs           # TGFS view models (index+blob doc join into one card)
│   ├── tgfs_query_filters.rs    # TGFS search-filter parsing (adds bot id, file id)
│   ├── tgfs_join.rs              # cross-cluster index/blob join + sampling helpers
│   ├── tgfs_token.rs              # HMAC dl/watch link signing (matches the Python bot)
│   └── handlers/
│       ├── pages.rs          # /, /login, /logout, /instructions-plgb, /instructions-tgfs
│       ├── review.rs         # /review-plgb, /submit
│       ├── done.rs           # /done-plgb
│       ├── stats.rs          # /stats-plgb
│       ├── tgfs_review.rs    # /review-tgfs, /submit-tgfs
│       ├── tgfs_done.rs      # /done-tgfs
│       └── tgfs_stats.rs     # /stats-tgfs
├── templates/             # Askama (Jinja-like) templates, compiled into the binary
└── static/style.css       # unchanged from the original app
```

All routes above are relative to the `/link-review` prefix defined once as `BASE_PATH` in
`src/main.rs`, and referenced from templates via Askama's `{{ crate::BASE_PATH }}`.

## Local development

Requires Rust 1.88+ (`rustup update`) and a reachable MongoDB instance.

```bash
cp .env.example .env
# edit .env with your PLGB_MONGODB_URI and a real ACCESS_KEY value

cargo run
```

The server listens on `http://localhost:8000` by default (override with `PORT`). Visit
`http://localhost:8000/link-review?key=<ACCESS_KEY>` to reach it.

## Access gate

Every request under `/link-review` is gated by a shared secret, independent of the
username/password login:

- First visit must include `?key=<ACCESS_KEY>` (matching the `ACCESS_KEY` env var).
- On success, an encrypted cookie is granted so you don't need to keep passing `?key=` on
  every click while browsing.
- Any request without a valid key or cookie gets a plain 404 (not 403), so the endpoint's
  existence isn't revealed to random scanners/bots hitting the bare domain.

## Reviewer accounts

There is no user database — usernames/passwords are configured entirely via env vars, read
once at startup:

```
LINK_REVIEW_USER1=nik
LINK_REVIEW_USER1_PASS=some-password
LINK_REVIEW_USER2=prdp
LINK_REVIEW_USER2_PASS=another-password
```

Numbering must start at `1` with no gaps; the app stops scanning at the first missing pair.
At least one pair must be set or the app refuses to start. To change a password or add/remove
a reviewer, edit the env vars and restart — no database migration needed.

## Deploying to Koyeb (free tier)

1. Push this repo to GitHub (the old `link-allow-page/` folder is gitignored and won't be included).
2. In Koyeb, create a new **Web Service** from that repo, build method **Dockerfile**.
3. Set these environment variables/secrets in the Koyeb service:
   - `PLGB_MONGODB_URI` — your MongoDB Atlas (or other) connection string for the PLGB (WebStreamer) bot.
   - `ACCESS_KEY` — a long random string (`openssl rand -hex 32`); this is the `?key=` value
     visitors must supply, and it also encrypts/signs session & flash cookies.
   - `LINK_REVIEW_USER1` / `LINK_REVIEW_USER1_PASS` (and `_USER2`, `_USER3`, ... as needed) —
     reviewer login credentials, see "Reviewer accounts" above.
   - `PLGB_FQDN` — base domain used to build each PLGB file's DL/Watch links, e.g. `fcdn.example.com`
     (no scheme/path); change this whenever the CDN host changes, no redeploy of code needed.
   - `TGFS_MONGODB_URI` — the telethon-plgb bot's primary MongoDB connection string.
   - `TGFS_MONGODB_DBNAME` — defaults to `TGFS`, matching the bot's own default.
   - `TGFS_MONGODB_INDEX_URI1`, `TGFS_MONGODB_INDEX_URI2`, ... — optional, only needed if the bot
     is configured with more than one `MONGODB_INDEX_URI*` cluster; falls back to the primary URI.
   - `TGFS_MONGODB_BLOB_URI1`, `TGFS_MONGODB_BLOB_URI2`, ... — same, for `MONGODB_BLOB_URI*` clusters.
   - `TGFS_PUBLIC_URL` — the bot's own `PUBLIC_URL` (e.g. `https://tgfs.example.com`), used to build
     working signed DL/Watch links.
   - Koyeb automatically injects `PORT`; the app already reads it.
   - Note: the TGFS bot must have run at least once before this app starts, so it has generated
     its `link.secret` in `app_config` — this app reads (but never creates) that secret.
4. Pick the free instance size (1 instance, smallest plan) and deploy.
5. Once live, open `https://<your-app>.koyeb.app/link-review?key=<ACCESS_KEY>` and log in with
   one of the configured `LINK_REVIEW_USER{n}` / `LINK_REVIEW_USER{n}_PASS` pairs.

### Notes on state

- Sessions are stored in an encrypted (private) cookie, not server memory — this plays well with
  Koyeb's free tier, which may sleep/restart the single instance between requests.
- Flash messages (login/logout notices) use a short-lived cookie of their own.
- The MongoDB `is_public` index is created automatically on startup, mirroring the original app.

## Differences from the Flask version

- No server-side session store; the whole session/flash mechanism is cookie-based (see `src/auth.rs`).
- No user database: reviewer accounts are plain username/password pairs from env vars (see
  "Reviewer accounts" above), compared with a constant-time check instead of Argon2/bcrypt hashing.
- Templates are compiled into the binary at build time (Askama), so only `static/` needs to ship in the
  final Docker image.
