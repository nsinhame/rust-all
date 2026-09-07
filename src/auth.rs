use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::FromRef;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Redirect;
use axum_extra::extract::cookie::{Cookie, Key, PrivateCookieJar, SameSite};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Data stored (encrypted) inside the `session` cookie.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionData {
    pub user_id: String,
    pub username: String,
}

/// Extractor that requires a valid session cookie; redirects to `/login` otherwise.
pub struct AuthUser(pub SessionData);

impl<S> FromRequestParts<S> for AuthUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let jar = PrivateCookieJar::<Key>::from_headers(&parts.headers, app_state.cookie_key.clone());
        let login_path = format!("{}/login", crate::BASE_PATH);
        let cookie = jar.get("session").ok_or_else(|| Redirect::to(&login_path))?;
        let data = serde_json::from_str::<SessionData>(cookie.value())
            .map_err(|_| Redirect::to(&login_path))?;
        Ok(AuthUser(data))
    }
}

pub fn make_session_cookie(data: &SessionData) -> Cookie<'static> {
    let value = serde_json::to_string(data).expect("SessionData always serializes");
    Cookie::build(("session", value))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .into()
}

pub fn session_removal_cookie() -> Cookie<'static> {
    Cookie::build("session").path("/").into()
}

#[derive(Serialize, Deserialize)]
struct FlashMsg {
    category: String,
    message: String,
}

/// Builds a one-shot cookie used to carry a flash message across a redirect.
pub fn make_flash_cookie(category: &str, message: &str) -> Cookie<'static> {
    let payload = serde_json::to_string(&FlashMsg {
        category: category.to_string(),
        message: message.to_string(),
    })
    .expect("FlashMsg always serializes");
    Cookie::build(("flash", payload))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .into()
}

/// Reads and clears the flash cookie (if any), returning the updated jar.
pub fn take_flash(jar: PrivateCookieJar) -> (PrivateCookieJar, Option<(String, String)>) {
    match jar.get("flash") {
        Some(cookie) => {
            let value = cookie.value().to_string();
            let jar = jar.remove(Cookie::build("flash").path("/"));
            let parsed = serde_json::from_str::<FlashMsg>(&value)
                .ok()
                .map(|f| (f.category, f.message));
            (jar, parsed)
        }
        None => (jar, None),
    }
}

pub fn hash_password(password: &str) -> String {
    // `hash_password` (the `getrandom` feature is enabled by default) generates
    // a fresh random salt internally, so no explicit salt is needed here.
    Argon2::default()
        .hash_password(password.as_bytes())
        .expect("argon2 hashing should not fail")
        .to_string()
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Derives a cookie signing/encryption key from an arbitrary-length secret string.
pub fn derive_cookie_key(secret: &str) -> Key {
    Key::derive_from(secret.as_bytes())
}

/// Like [`AuthUser`] but never fails; used by pages (e.g. `/login`) that need
/// to know whether a session exists without forcing a redirect.
pub struct OptionalAuthUser(pub Option<SessionData>);

impl<S> FromRequestParts<S> for OptionalAuthUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let jar = PrivateCookieJar::<Key>::from_headers(&parts.headers, app_state.cookie_key.clone());
        let data = jar
            .get("session")
            .and_then(|c| serde_json::from_str::<SessionData>(c.value()).ok());
        Ok(OptionalAuthUser(data))
    }
}
