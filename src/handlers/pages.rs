use askama::Template;
use axum::extract::State;
use axum::response::{IntoResponse, Redirect};
use axum::Form;
use axum_extra::extract::cookie::PrivateCookieJar;
use serde::Deserialize;

use crate::auth::{
    constant_time_eq, make_flash_cookie, make_session_cookie, session_removal_cookie, take_flash,
    AuthUser, OptionalAuthUser, SessionData,
};
use crate::models::Flash;
use crate::state::AppState;
use crate::util::render;

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
    show_plgb_nav: bool,
    show_tgfs_nav: bool,
}

#[derive(Template)]
#[template(path = "instructions.html")]
struct InstructionsTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
    show_plgb_nav: bool,
    show_tgfs_nav: bool,
}

#[derive(Template)]
#[template(path = "instructions-tgfs.html")]
struct InstructionsTgfsTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
    show_plgb_nav: bool,
    show_tgfs_nav: bool,
}

#[derive(Template)]
#[template(path = "home.html")]
struct HomeTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
    show_plgb_nav: bool,
    show_tgfs_nav: bool,
}

/// `GET /` - redirects to `/home` if logged in, otherwise to `/login`
/// (the `AuthUser` extractor itself redirects to `/login` on failure).
pub async fn index(_user: AuthUser) -> Redirect {
    Redirect::to(&format!("{}/home", crate::BASE_PATH))
}

pub async fn home(AuthUser(session): AuthUser, jar: PrivateCookieJar) -> impl IntoResponse {
    let (jar, flash) = take_flash(jar);
    let flashes = flash
        .map(|(category, message)| vec![Flash { category, message }])
        .unwrap_or_default();
    let tmpl = HomeTemplate {
        logged_in: true,
        username: session.username,
        flashes,
        show_plgb_nav: false,
        show_tgfs_nav: false,
    };
    (jar, render(tmpl))
}

/// `GET /tgfs` - kept around for old bookmarks/links; the home page now links
/// straight to `/review-tgfs`.
pub async fn tgfs() -> Redirect {
    Redirect::to(&format!("{}/review-tgfs", crate::BASE_PATH))
}

pub async fn login_get(
    OptionalAuthUser(session): OptionalAuthUser,
    jar: PrivateCookieJar,
) -> impl IntoResponse {
    if session.is_some() {
        return (jar, Redirect::to(&format!("{}/home", crate::BASE_PATH))).into_response();
    }
    let (jar, flash) = take_flash(jar);
    let flashes = flash
        .map(|(category, message)| vec![Flash { category, message }])
        .unwrap_or_default();
    let tmpl = LoginTemplate {
        logged_in: false,
        username: String::new(),
        flashes,
        show_plgb_nav: false,
        show_tgfs_nav: false,
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
    let matched = state
        .users
        .iter()
        .find(|(username, password)| {
            constant_time_eq(username, &form.username) && constant_time_eq(password, &form.password)
        });

    if let Some((username, _)) = matched {
        let session = SessionData {
            username: username.clone(),
        };
        let jar = jar
            .add(make_session_cookie(&session))
            .add(make_flash_cookie("success", "Login successful!"));
        return (jar, Redirect::to(&format!("{}/home", crate::BASE_PATH))).into_response();
    }

    let flashes = vec![Flash {
        category: "error".to_string(),
        message: "Invalid username or password".to_string(),
    }];
    let tmpl = LoginTemplate {
        logged_in: false,
        username: String::new(),
        flashes,
        show_plgb_nav: false,
        show_tgfs_nav: false,
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
        show_plgb_nav: true,
        show_tgfs_nav: false,
    };
    (jar, render(tmpl))
}

pub async fn instructions_tgfs(
    AuthUser(session): AuthUser,
    jar: PrivateCookieJar,
) -> impl IntoResponse {
    let (jar, flash) = take_flash(jar);
    let flashes = flash
        .map(|(category, message)| vec![Flash { category, message }])
        .unwrap_or_default();
    let tmpl = InstructionsTgfsTemplate {
        logged_in: true,
        username: session.username,
        flashes,
        show_plgb_nav: false,
        show_tgfs_nav: true,
    };
    (jar, render(tmpl))
}
