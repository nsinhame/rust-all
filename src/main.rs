mod auth;
mod handlers;
mod models;
mod query_filters;
mod state;
mod util;

use axum::extract::{Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use axum_extra::extract::cookie::{Cookie, PrivateCookieJar};
use mongodb::bson::doc;
use mongodb::{Client, IndexModel};
use std::collections::HashMap;
use std::net::SocketAddr;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use auth::{derive_cookie_key, hash_password};
use state::{build_state, AppState};

/// URL prefix the whole app is served under, e.g. `https://host/link-review/review`.
pub const BASE_PATH: &str = "/link-review";

/// Name of the cookie that remembers a successful `?key=` check.
const ACCESS_COOKIE: &str = "link_access";

/// Ensures the two predefined reviewer accounts exist, matching the
/// behaviour of the original Flask app's `create_default_users`.
async fn create_default_users(state: &AppState) {
    let default_users = [("nik", "harekrishna"), ("prdp", "harekrishna")];
    for (username, password) in default_users {
        let existing = state
            .users
            .find_one(doc! { "username": username })
            .await
            .ok()
            .flatten();
        if existing.is_none() {
            let hashed = hash_password(password);
            let user_doc = doc! {
                "username": username,
                "password": hashed,
                "created_at": mongodb::bson::DateTime::now(),
            };
            match state.users.insert_one(user_doc).await {
                Ok(_) => tracing::info!("created default user: {username}"),
                Err(err) => tracing::error!("failed to create default user {username}: {err}"),
            }
        }
    }
}

/// Gate for everything under [`BASE_PATH`]: requires a valid `link_access` cookie, or a
/// `?key=` query parameter matching `ACCESS_KEY`, in which case the cookie is then granted.
async fn require_access_key(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Query(params): Query<HashMap<String, String>>,
    request: Request,
    next: Next,
) -> impl IntoResponse {
    let has_valid_cookie = jar
        .get(ACCESS_COOKIE)
        .map(|c| c.value() == state.access_key)
        .unwrap_or(false);
    if has_valid_cookie {
        return next.run(request).await.into_response();
    }

    if params.get("key") == Some(&state.access_key) {
        let response = next.run(request).await;
        let jar = PrivateCookieJar::new(state.cookie_key.clone()).add(
            Cookie::build((ACCESS_COOKIE, state.access_key.clone()))
                .path(BASE_PATH)
                .http_only(true),
        );
        return (jar, response).into_response();
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let mongodb_uri = std::env::var("MONGODB_URI").expect("MONGODB_URI must be set");
    let access_key = std::env::var("ACCESS_KEY")
        .expect("ACCESS_KEY must be set (this is the `?key=` value required to reach the app)");

    let client = Client::with_uri_str(&mongodb_uri)
        .await
        .expect("failed to connect to MongoDB");
    // Cookie encryption is derived from ACCESS_KEY so only one secret needs managing.
    let cookie_key = derive_cookie_key(&access_key);
    let app_state = build_state(&client, cookie_key, access_key);

    create_default_users(&app_state).await;

    let index = IndexModel::builder().keys(doc! { "is_public": 1 }).build();
    if let Err(err) = app_state.files.create_index(index).await {
        tracing::warn!("failed to create is_public index: {err}");
    }

    let protected = Router::new()
        .route("/", get(handlers::pages::index))
        .route(
            "/login",
            get(handlers::pages::login_get).post(handlers::pages::login_post),
        )
        .route("/logout", get(handlers::pages::logout))
        .route("/instructions", get(handlers::pages::instructions))
        .route("/review", get(handlers::review::review))
        .route("/submit", post(handlers::review::submit))
        .route("/done", get(handlers::done::done))
        .route("/stats", get(handlers::stats::stats))
        .nest_service("/static", ServeDir::new("static"))
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            require_access_key,
        ));

    let app = Router::new()
        .nest(BASE_PATH, protected)
        .layer(TraceLayer::new_for_http())
        .with_state(app_state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!("listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind port");
    axum::serve(listener, app).await.expect("server error");
}
