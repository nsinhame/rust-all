# File Review System (Rust)

A Rust rewrite of the original Flask file-review app, built with [Axum](https://github.com/tokio-rs/axum),
[Askama](https://askama.rs) templates and the official [MongoDB driver](https://www.mongodb.com/docs/drivers/rust/).
Designed to run comfortably within Koyeb's free tier (single small instance, low memory).

## Features

Functionally equivalent to the original app:

- Session-based login for the two predefined reviewers (`nik` / `prdp`), auto-created on first boot.
- `/review` — 10 random unreviewed files with search (user id include/exclude, file name, forwarded-from, size range, Mongo `_id`) and live stats sidebar.
- `/submit` — bulk accept/reject, optional rename, optional "special hash" tagging.
- `/done` — paginated list of reviewed files with the same filters plus reviewer/status.
- `/stats` — dashboard with global + per-reviewer (nik vs prdp) comparison.
- `/instructions` — static help page (search tips + 50 regex rename examples).

## Project layout

The old Flask app lives in `link-allow-page/` (gitignored, kept locally for reference only).
Everything below lives at the repository root:

```
├── Cargo.toml
├── Dockerfile
├── src/
│   ├── main.rs          # entrypoint, router, default users, index creation
│   ├── state.rs          # AppState (Mongo collections, cookie key)
│   ├── auth.rs            # session/flash cookies, password hashing
│   ├── models.rs          # BSON helpers + view models used by templates
│   ├── query_filters.rs   # shared search-filter parsing (review/done)
│   ├── util.rs            # formatting helpers, template render helper
│   └── handlers/
│       ├── pages.rs        # /, /login, /logout, /instructions
│       ├── review.rs       # /review, /submit
│       ├── done.rs         # /done
│       └── stats.rs        # /stats
├── templates/             # Askama (Jinja-like) templates, compiled into the binary
└── static/style.css       # unchanged from the original app
```

## Local development

Requires Rust 1.88+ (`rustup update`) and a reachable MongoDB instance.

```bash
cp .env.example .env
# edit .env with your MONGODB_URI and a real SECRET_KEY

cargo run
```

The server listens on `http://localhost:8000` by default (override with `PORT`).

## Deploying to Koyeb (free tier)

1. Push this repo to GitHub (the old `link-allow-page/` folder is gitignored and won't be included).
2. In Koyeb, create a new **Web Service** from that repo, build method **Dockerfile**.
3. Set these environment variables/secrets in the Koyeb service:
   - `MONGODB_URI` — your MongoDB Atlas (or other) connection string.
   - `SECRET_KEY` — a long random string (`openssl rand -hex 32`).
   - Koyeb automatically injects `PORT`; the app already reads it.
4. Pick the free instance size (1 instance, smallest plan) and deploy.
5. Once live, log in with `nik` / `harekrishna` or `prdp` / `harekrishna` (change these passwords in
   the database afterwards, or update `create_default_users` in `main.rs` before first boot).

### Notes on state

- Sessions are stored in an encrypted (private) cookie, not server memory — this plays well with
  Koyeb's free tier, which may sleep/restart the single instance between requests.
- Flash messages (login/logout notices) use a short-lived cookie of their own.
- The MongoDB `is_public` index is created automatically on startup, mirroring the original app.

## Differences from the Flask version

- No server-side session store; the whole session/flash mechanism is cookie-based (see `src/auth.rs`).
- Passwords are hashed with Argon2id instead of Werkzeug's PBKDF2 (existing users are recreated with
  Argon2 hashes on first boot if they don't already exist — this does not touch existing custom users
  you may have added directly in MongoDB with a different hash format, so re-create those manually if needed).
- Templates are compiled into the binary at build time (Askama), so only `static/` needs to ship in the
  final Docker image.
