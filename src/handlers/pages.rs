use askama::Template;
use axum::extract::State;
use axum::response::{IntoResponse, Redirect};
use axum::Form;
use axum_extra::extract::cookie::PrivateCookieJar;
use mongodb::bson::doc;
use serde::Deserialize;

use crate::auth::{
    make_flash_cookie, make_session_cookie, session_removal_cookie, take_flash, verify_password,
    AuthUser, OptionalAuthUser, SessionData,
};
use crate::models::{get_object_id, get_str, Flash};
use crate::state::AppState;
use crate::util::render;

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
}

#[derive(Template)]
#[template(path = "instructions.html")]
struct InstructionsTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
}

/// `GET /` - redirects to `/review` if logged in, otherwise to `/login`
/// (the `AuthUser` extractor itself redirects to `/login` on failure).
pub async fn index(_user: AuthUser) -> Redirect {
    Redirect::to(&format!("{}/review", crate::BASE_PATH))
}

pub async fn login_get(
    OptionalAuthUser(session): OptionalAuthUser,
    jar: PrivateCookieJar,
) -> impl IntoResponse {
    if session.is_some() {
        return (jar, Redirect::to(&format!("{}/review", crate::BASE_PATH))).into_response();
    }
    let (jar, flash) = take_flash(jar);
    let flashes = flash
        .map(|(category, message)| vec![Flash { category, message }])
        .unwrap_or_default();
    let tmpl = LoginTemplate {
        logged_in: false,
        username: String::new(),
        flashes,
    };
    (jar, render(tmpl)).into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

pub async fn login_post(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let user_doc = state
        .users
        .find_one(doc! { "username": &form.username })
        .await
        .ok()
        .flatten();

    if let Some(doc) = user_doc {
        let hash = get_str(&doc, "password").unwrap_or_default();
        if verify_password(&form.password, &hash) {
            let username = get_str(&doc, "username").unwrap_or(form.username);
            let user_id = get_object_id(&doc).map(|o| o.to_hex()).unwrap_or_default();
            let session = SessionData { user_id, username };
            let jar = jar
                .add(make_session_cookie(&session))
                .add(make_flash_cookie("success", "Login successful!"));
            return (jar, Redirect::to(&format!("{}/review", crate::BASE_PATH))).into_response();
        }
    }

    let flashes = vec![Flash {
        category: "error".to_string(),
        message: "Invalid username or password".to_string(),
    }];
    let tmpl = LoginTemplate {
        logged_in: false,
        username: String::new(),
        flashes,
    };
    render(tmpl).into_response()
}

pub async fn logout(jar: PrivateCookieJar) -> impl IntoResponse {
    let jar = jar
        .remove(session_removal_cookie())
        .add(make_flash_cookie("success", "Logged out successfully"));
    (jar, Redirect::to(&format!("{}/login", crate::BASE_PATH)))
}

pub async fn instructions(AuthUser(session): AuthUser, jar: PrivateCookieJar) -> impl IntoResponse {
    let (jar, flash) = take_flash(jar);
    let flashes = flash
        .map(|(category, message)| vec![Flash { category, message }])
        .unwrap_or_default();
    let tmpl = InstructionsTemplate {
        logged_in: true,
        username: session.username,
        flashes,
    };
    (jar, render(tmpl))
}
