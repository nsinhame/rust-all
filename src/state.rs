use axum::extract::FromRef;
use axum_extra::extract::cookie::Key;
use mongodb::bson::Document;
use mongodb::{Client, Collection};

/// Shared application state handed to every handler via axum's `State` extractor.
#[derive(Clone)]
pub struct AppState {
    pub files: Collection<Document>,
    pub users: Collection<Document>,
    pub cookie_key: Key,
}

impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}

pub fn build_state(client: &Client, cookie_key: Key) -> AppState {
    let files_db = client.database("F2LxBot");
    let auth_db = client.database("link_allow_auth");
    AppState {
        files: files_db.collection::<Document>("file"),
        users: auth_db.collection::<Document>("users"),
        cookie_key,
    }
}
