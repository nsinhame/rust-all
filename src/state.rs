use axum::extract::FromRef;
use axum_extra::extract::cookie::Key;
use mongodb::bson::Document;
use mongodb::{Client, Collection};

/// telethon-plgb's multi-bot / multi-cluster-Mongo state (see `tgfs/database/mongodb/__init__.py`).
///
/// - `index_colls[i]` is cluster `i`'s `user_files` collection (one row per
///   user+bot+file "link"; this is what the review UI iterates over).
/// - `blob_colls[i]` is cluster `i`'s `files` collection, keyed by Telegram
///   `file_id`, holding the physical file's `is_restricted`/size/mime/etc.
///   An index entry's `cluster` field says which blob cluster its file lives in.
/// - Both lists fall back to a single-element `vec![primary]` when no
///   `TGFS_MONGODB_INDEX_URI*`/`TGFS_MONGODB_BLOB_URI*` are configured, mirroring
///   the Python `index_dbs or [primary]` fallback.
#[derive(Clone)]
pub struct TgfsState {
    /// Primary DB's `users` collection (`warns`, `ban_date`, `bots`, ...).
    pub primary_users: Collection<Document>,
    pub index_colls: Vec<Collection<Document>>,
    pub blob_colls: Vec<Collection<Document>>,
    /// HMAC-SHA256 key used to sign/verify dl/watch tokens (`config` doc `link.secret`).
    pub link_secret: Vec<u8>,
    /// `TGFS_PUBLIC_URL`, no trailing slash, e.g. `https://tgfs.example.com`.
    pub public_url: String,
}

/// Shared application state handed to every handler via axum's `State` extractor.
#[derive(Clone)]
pub struct AppState {
    pub files: Collection<Document>,
    /// The plgb bot's `users` collection (Telegram user id `id`, `warn_count`, etc.).
    pub bot_users: Collection<Document>,
    /// (username, password) pairs loaded from `LINK_REVIEW_USER{n}` / `LINK_REVIEW_USER{n}_PASS`.
    pub users: Vec<(String, String)>,
    pub cookie_key: Key,
    /// Shared secret required (via `?key=` or the granting cookie) to reach the app at all.
    pub access_key: String,
    /// Base domain (from `PLGB_FQDN` env var) file dl/watch links are built against, e.g. `fcdn.example.com`.
    pub fqdn: String,
    pub tgfs: TgfsState,
}

impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}

pub fn build_state(
    client: &Client,
    cookie_key: Key,
    access_key: String,
    users: Vec<(String, String)>,
    fqdn: String,
    tgfs: TgfsState,
) -> AppState {
    let files_db = client.database("F2LxBot");
    AppState {
        tgfs,
        files: files_db.collection::<Document>("file"),
        bot_users: files_db.collection::<Document>("users"),
        users,
        cookie_key,
        access_key,
        fqdn,
    }
}
