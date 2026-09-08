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

use auth::derive_cookie_key;
use state::{build_state, AppState};

/// URL prefix the whole app is served under, e.g. `https://host/link-review/review`.
pub const BASE_PATH: &str = "/link-review";

/// Name of the cookie that remembers a successful `?key=` check.
const ACCESS_COOKIE: &str = "link_access";

/// Loads reviewer credentials from `LINK_REVIEW_USER{n}` / `LINK_REVIEW_USER{n}_PASS` pairs,
/// starting at n=1 and stopping at the first missing/incomplete pair.
fn load_users_from_env() -> Vec<(String, String)> {
    let mut users = Vec::new();
    let mut n = 1;
    loop {
        let username = std::env::var(format!("LINK_REVIEW_USER{n}"));
        let password = std::env::var(format!("LINK_REVIEW_USER{n}_PASS"));
        match (username, password) {
            (Ok(username), Ok(password)) if !username.is_empty() => {
                users.push((username, password));
                n += 1;
            }
            _ => break,
        }
    }
    users
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
    let fqdn = std::env::var("FQDN")
        .expect("FQDN must be set (base domain used to build dl/watch links, e.g. fcdn.example.com)");
    let users = load_users_from_env();
    if users.is_empty() {
        panic!(
            "no reviewer accounts configured; set LINK_REVIEW_USER1 / LINK_REVIEW_USER1_PASS (and _USER2, _USER3, ... as needed)"
        );
    }

    let client = Client::with_uri_str(&mongodb_uri)
        .await
        .expect("failed to connect to MongoDB");
    // Cookie encryption is derived from ACCESS_KEY so only one secret needs managing.
    let cookie_key = derive_cookie_key(&access_key);
    let app_state = build_state(&client, cookie_key, access_key, users, fqdn);

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
        .route("/home", get(handlers::pages::home))
        .route("/tgfs", get(handlers::pages::tgfs))
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
