mod auth;
mod handlers;
mod models;
mod query_filters;
mod state;
mod util;

use axum::routing::{get, post};
use axum::Router;
use mongodb::bson::doc;
use mongodb::{Client, IndexModel};
use std::net::SocketAddr;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use auth::{derive_cookie_key, hash_password};
use state::{build_state, AppState};

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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let mongodb_uri = std::env::var("MONGODB_URI").expect("MONGODB_URI must be set");
    let secret_key = std::env::var("SECRET_KEY")
        .expect("SECRET_KEY must be set (generate one with `openssl rand -hex 32`)");

    let client = Client::with_uri_str(&mongodb_uri)
        .await
        .expect("failed to connect to MongoDB");
    let cookie_key = derive_cookie_key(&secret_key);
    let app_state = build_state(&client, cookie_key);

    create_default_users(&app_state).await;

    let index = IndexModel::builder().keys(doc! { "is_public": 1 }).build();
    if let Err(err) = app_state.files.create_index(index).await {
        tracing::warn!("failed to create is_public index: {err}");
    }

    let app = Router::new()
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
