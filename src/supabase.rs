//! The rcad.app Supabase session shared by the site's web apps.
//!
//! `web-console` runs the GitHub OAuth + PKCE flow and stores the resulting
//! session in localStorage under [`SESSION_KEY`]. `creator` web and `ocad` web
//! are served from the same origin, so they read the same entry: one account
//! across the three apps, and the access token is what the cloud collaboration
//! room authenticates.
//!
//! The session is a plain JSON value whose shape is web-console's (it is the
//! writer). Native builds have no localStorage: [`load`] returns `None` and
//! [`store`]/[`clear`] do nothing, so a desktop product never mistakes a web
//! session for its own credential store.

use serde::{Deserialize, Serialize};

/// The localStorage key the session is stored under.
pub const SESSION_KEY: &str = "rcad.session";

/// The authenticated browser session, kept in local storage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds when the access token stops working.
    pub expires_at: i64,
    pub user_id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// The identity the frameworks' account surfaces show. Every consumer maps it
/// onto its own account type, so the name/email fallback lives once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
}

impl Session {
    /// Whether the access token has reached its expiry.
    pub fn is_expired(&self) -> bool {
        now_unix() >= self.expires_at
    }

    /// The name to show next to the avatar: the profile name, else the email,
    /// else a generic label.
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .or(self.email.as_deref())
            .unwrap_or("用户")
    }

    /// The bearer token for the site's own services (the collaboration room).
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// The identity snapshot.
    pub fn identity(&self) -> Identity {
        Identity {
            id: self.user_id.clone(),
            name: self.display_name().to_string(),
            email: self.email.clone(),
        }
    }
}

/// Reads the shared session; `None` when there is none (or not a browser).
#[cfg(target_family = "wasm")]
pub fn load() -> Option<Session> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let raw = storage.get_item(SESSION_KEY).ok()??;
    serde_json::from_str(&raw).ok()
}

/// Reads the shared session; native builds never have one.
#[cfg(not(target_family = "wasm"))]
pub fn load() -> Option<Session> {
    None
}

/// Stores the session for the other apps on the origin to find.
#[cfg(target_family = "wasm")]
pub fn store(session: &Session) {
    let Some(storage) = web_sys::window()
        .and_then(|window| window.local_storage().ok())
        .flatten()
    else {
        return;
    };
    if let Ok(raw) = serde_json::to_string(session) {
        let _ = storage.set_item(SESSION_KEY, &raw);
    }
}

/// Stores the session; native builds have no local storage.
#[cfg(not(target_family = "wasm"))]
pub fn store(_session: &Session) {}

/// Clears the shared session (sign-out).
#[cfg(target_family = "wasm")]
pub fn clear() {
    if let Some(storage) = web_sys::window()
        .and_then(|window| window.local_storage().ok())
        .flatten()
    {
        let _ = storage.remove_item(SESSION_KEY);
    }
}

/// Clears the shared session; native builds have no local storage.
#[cfg(not(target_family = "wasm"))]
pub fn clear() {}

/// The login page of the site's console, returning to `next` (a same-origin
/// path) once the flow completes. An empty `next` lands on the console.
pub fn login_url(next: &str) -> String {
    #[cfg(target_family = "wasm")]
    let origin = web_sys::window()
        .and_then(|window| window.location().origin().ok())
        .unwrap_or_else(|| "https://rcad.app".to_string());
    #[cfg(not(target_family = "wasm"))]
    let origin = "https://rcad.app".to_string();

    if next.is_empty() {
        format!("{origin}/login")
    } else {
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("next", next);
        format!("{origin}/login?{}", query.finish())
    }
}

/// The current page's path, the default destination after a sign-in.
pub fn current_path() -> String {
    #[cfg(target_family = "wasm")]
    {
        web_sys::window()
            .and_then(|window| window.location().pathname().ok())
            .unwrap_or_else(|| "/".to_string())
    }
    #[cfg(not(target_family = "wasm"))]
    {
        "/".to_string()
    }
}

#[cfg(target_family = "wasm")]
fn now_unix() -> i64 {
    (js_sys::Date::new_0().get_time() / 1000.0) as i64
}

#[cfg(not(target_family = "wasm"))]
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            access_token: "token-1".to_string(),
            refresh_token: Some("refresh-1".to_string()),
            expires_at: now_unix() + 3600,
            user_id: "u1".to_string(),
            email: Some("u1@example.test".to_string()),
            name: Some("Alice".to_string()),
        }
    }

    #[test]
    fn a_session_round_trips_through_its_json_shape() {
        let session = session();
        let raw = serde_json::to_string(&session).expect("serializes");
        assert_eq!(serde_json::from_str::<Session>(&raw).expect("parses"), session);
    }

    #[test]
    fn the_display_name_falls_back_to_the_email_then_a_generic_label() {
        let mut session = session();
        assert_eq!(session.display_name(), "Alice");
        session.name = None;
        assert_eq!(session.display_name(), "u1@example.test");
        session.email = None;
        assert_eq!(session.display_name(), "用户");
    }

    #[test]
    fn an_expired_session_is_recognized() {
        let mut session = session();
        assert!(!session.is_expired());
        session.expires_at = now_unix() - 1;
        assert!(session.is_expired());
    }

    #[test]
    fn the_identity_carries_the_id_name_and_email() {
        let identity = session().identity();
        assert_eq!(identity.id, "u1");
        assert_eq!(identity.name, "Alice");
        assert_eq!(identity.email.as_deref(), Some("u1@example.test"));
    }

    #[test]
    fn the_login_url_carries_the_return_path() {
        let url = login_url("/creator/");
        assert!(url.ends_with("/login?next=%2Fcreator%2F"), "{url}");
        let bare = login_url("");
        assert!(bare.ends_with("/login"), "{bare}");
    }
}
