use axum::extract::FromRef;
use axum_extra::extract::cookie::Key;
use mongodb::bson::Document;
use mongodb::{Client, Collection};

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
    /// Base domain (from `FQDN` env var) file dl/watch links are built against, e.g. `fcdn.example.com`.
    pub fqdn: String,
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
) -> AppState {
    let files_db = client.database("F2LxBot");
    AppState {
        files: files_db.collection::<Document>("file"),
        bot_users: files_db.collection::<Document>("users"),
        users,
        cookie_key,
        access_key,
        fqdn,
    }
}
