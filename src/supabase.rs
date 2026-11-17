//! The rcad.app Supabase account, shared by every deployed product.
//!
//! One account system: the Supabase project at [`DEFAULT_URL`]. On the web,
//! `web-console` runs the GitHub OAuth + PKCE flow and stores the session in
//! localStorage under [`SESSION_KEY`]; `creator` web and `ocad` web are served
//! from the same origin and read the same entry. On the desktop,
//! [`SupabaseAccount`] runs the same provider's authorization-code + PKCE flow
//! against a loopback redirect ([`Config`]), so the signed-in user and the
//! access token are the same account on both platforms.
//!
//! The session JSON shape is web-console's (it is the writer). Native builds
//! have no localStorage: [`load`] returns `None` and [`store`]/[`clear`] do
//! nothing, so a desktop product never mistakes a web session for its own
//! credential store.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// The Supabase project every deployed product signs in against.
pub const DEFAULT_URL: &str = "https://vdixmifkbcefcegnhzjv.supabase.co";
/// The OAuth provider the site uses.
pub const DEFAULT_PROVIDER: &str = "github";

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

    #[test]
    fn the_native_sign_in_url_carries_the_pkce_parameters() {
        let request = SupabaseAccount::new(Config {
            url: "https://project.test".to_string(),
            provider: "github".to_string(),
            redirect_port: 0,
            redirect_path: "/callback".to_string(),
            ..Config::default()
        })
        .begin()
        .expect("the sign-in starts");
        assert!(request.user_code.is_none(), "the loopback redirect has no code");
        let url = request.url;
        assert!(url.starts_with("https://project.test/auth/v1/authorize?"), "{url}");
        for pair in [
            "provider=github",
            "code_challenge_method=S256",
            "redirect_to=http%3A%2F%2F127.0.0.1%3A",
            "code_challenge=",
            "state=",
        ] {
            assert!(url.contains(pair), "{url} is missing {pair}");
        }
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_desktop_session_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("rcad-supabase-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("session.json");
        let account = Account {
            id: "u1".to_string(),
            name: "Alice".to_string(),
            email: Some("u1@example.test".to_string()),
            access_token: "token".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: Some(now_unix() + 3600),
        };
        save_session(&path, &account).expect("the session saves");
        let restored = SupabaseAccount::new(Config {
            session_path: Some(path.clone()),
            ..Config::default()
        })
        .account()
        .expect("the session restores");
        assert_eq!(restored, account);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_desktop_flow_completes_through_the_loopback_redirect() {
        use std::io::{Read, Write};
        use std::net::{Ipv4Addr, TcpListener};

        // A mock Supabase token endpoint.
        let mock = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("the mock binds");
        let mock_port = mock.local_addr().expect("the mock address").port();
        let mock_thread = std::thread::spawn(move || {
            let (mut stream, _) = mock.accept().expect("the mock accepts");
            let mut buf = [0u8; 8192];
            let read = stream.read(&mut buf).expect("the mock reads");
            let request = String::from_utf8_lossy(&buf[..read]).to_string();
            assert!(
                request.starts_with("POST /auth/v1/token?grant_type=pkce"),
                "{request}"
            );
            assert!(request.contains("apikey: test-key"), "{request}");
            assert!(request.contains("auth_code=abc"), "{request}");
            let body = concat!(
                r#"{"access_token":"tok-1","refresh_token":"ref-1","expires_in":3600,"#,
                r#""user":{"id":"u1","email":"u1@example.test","user_metadata":{"name":"Alice"}}}"#,
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("the mock replies");
        });

        let dir = std::env::temp_dir().join(format!("rcad-supabase-flow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let account = SupabaseAccount::new(Config {
            url: format!("http://127.0.0.1:{mock_port}"),
            provider: "github".to_string(),
            anon_key: "test-key".to_string(),
            redirect_port: 0,
            redirect_path: "/callback".to_string(),
            session_path: Some(dir.join("session.json")),
        });
        let request = account.begin().expect("the sign-in starts");
        let query = request.url.split_once('?').expect("the query").1;
        let params: std::collections::HashMap<String, String> =
            form_urlencoded::parse(query.as_bytes()).into_owned().collect();
        let redirect_to = params["redirect_to"].clone();
        let state = params["state"].clone();

        // The browser comes back to the loopback redirect.
        let host = redirect_to
            .strip_prefix("http://")
            .and_then(|rest| rest.split('/').next())
            .expect("the redirect host")
            .to_string();
        std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(&host).expect("the redirect connects");
            let request = format!(
                "GET /callback?code=abc&state={state} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(request.as_bytes()).expect("the redirect sends");
        });

        let outcome = loop {
            match account.poll().expect("the poll") {
                Poll::Pending | Poll::SlowDown => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                other => break other,
            }
        };
        mock_thread.join().expect("the mock finishes");
        match outcome {
            Poll::Success(signed_in) => {
                assert_eq!(signed_in.id, "u1");
                assert_eq!(signed_in.name, "Alice");
                assert_eq!(account.access_token().as_deref(), Some("tok-1"));
            }
            other => panic!("the sign-in completed: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_account_is_expired_at_its_deadline() {
        let mut account = Account {
            id: "u1".to_string(),
            name: "Alice".to_string(),
            email: None,
            access_token: "token".to_string(),
            refresh_token: None,
            expires_at: Some(now_unix() + 60),
        };
        assert!(!account.is_expired());
        account.expires_at = Some(now_unix() - 1);
        assert!(account.is_expired());
    }
}

/// A signed-in Supabase account: the identity the product shows plus the
/// tokens its services authenticate. It is also the persisted session shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds when the access token stops working, when known.
    pub expires_at: Option<i64>,
}

impl Account {
    /// Whether the access token has reached its expiry.
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|expires_at| now_unix() >= expires_at)
    }
}

/// What the user does to sign in: open the URL (the loopback redirect has
/// nothing to type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInRequest {
    pub url: String,
    pub user_code: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

/// One sign-in poll ([`SupabaseAccount::poll`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Poll {
    /// Not finished yet; poll again after the interval.
    Pending,
    /// The provider asked for a slower cadence; pending.
    SlowDown,
    /// Signed in.
    Success(Account),
    /// The user refused.
    Denied,
    /// The sign-in request timed out.
    Expired,
    /// The flow ended with a message.
    Failed(String),
}

/// The Supabase endpoints and the loopback redirect the desktop flow uses.
#[derive(Clone, Debug)]
pub struct Config {
    /// The project URL (`https://<ref>.supabase.co`).
    pub url: String,
    /// The OAuth provider (`github`).
    pub provider: String,
    /// The anon key, sent as the `apikey` header on the token endpoint.
    pub anon_key: String,
    /// The loopback port; 0 takes any free one.
    pub redirect_port: u16,
    /// The path of the redirect on the loopback port.
    pub redirect_path: String,
    /// Where the desktop keeps the session between runs; `None` keeps it in
    /// memory only.
    #[cfg(not(target_family = "wasm"))]
    pub session_path: Option<std::path::PathBuf>,
}

impl Default for Config {
    /// The deployment's project: the runtime environment first (a desktop
    /// build reads it without a rebuild), then the compile-time override,
    /// then the live project.
    fn default() -> Self {
        Self {
            url: std::env::var("SUPABASE_URL")
                .ok()
                .or_else(|| option_env!("SUPABASE_URL").map(str::to_string))
                .unwrap_or_else(|| DEFAULT_URL.to_string()),
            provider: DEFAULT_PROVIDER.to_string(),
            anon_key: std::env::var("SUPABASE_ANON_KEY")
                .ok()
                .or_else(|| option_env!("SUPABASE_ANON_KEY").map(str::to_string))
                .unwrap_or_default(),
            redirect_port: 0,
            redirect_path: "/callback".to_string(),
            #[cfg(not(target_family = "wasm"))]
            session_path: default_session_path(),
        }
    }
}

/// The per-user file the desktop session is kept in (owner-only after the
/// first save), when the platform has a home directory.
#[cfg(all(not(target_family = "wasm"), target_os = "macos"))]
fn default_session_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        std::path::PathBuf::from(home)
            .join("Library/Application Support/rcad/supabase-session.json"),
    )
}

#[cfg(all(not(target_family = "wasm"), target_os = "windows"))]
fn default_session_path() -> Option<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(std::path::PathBuf::from(appdata).join("rcad/supabase-session.json"))
}

#[cfg(all(
    not(target_family = "wasm"),
    not(any(target_os = "macos", target_os = "windows"))
))]
fn default_session_path() -> Option<std::path::PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(std::path::PathBuf::from(xdg).join("rcad/supabase-session.json"));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        std::path::PathBuf::from(home).join(".config/rcad/supabase-session.json"),
    )
}

/// The shared Supabase account capability. Clones share one state, so a
/// product's shell and its per-session harness observe the same sign-in.
#[derive(Clone, Debug)]
pub struct SupabaseAccount {
    config: Config,
    /// The desktop sign-in/session state. Web builds keep the session in
    /// localStorage (see [`load`]), so nothing reads this there.
    #[allow(dead_code)]
    state: Arc<Mutex<State>>,
}

#[derive(Debug)]
enum State {
    Idle,
    #[cfg(not(target_family = "wasm"))]
    Waiting(Pending),
    #[cfg(not(target_family = "wasm"))]
    SignedIn(Account),
}

impl Default for State {
    fn default() -> Self {
        Self::Idle
    }
}

/// The desktop sign-in waiting for the browser's loopback redirect.
#[cfg(not(target_family = "wasm"))]
#[derive(Debug)]
struct Pending {
    listener: std::net::TcpListener,
    state: String,
    verifier: String,
    deadline: std::time::Instant,
}

impl Default for SupabaseAccount {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl SupabaseAccount {
    pub fn new(config: Config) -> Self {
        // A desktop build comes back signed in from the persisted session.
        #[cfg(not(target_family = "wasm"))]
        let state = match restore_session(config.session_path.as_deref()) {
            Some(account) => State::SignedIn(account),
            None => State::default(),
        };
        #[cfg(target_family = "wasm")]
        let state = State::default();
        Self {
            config,
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Starts the platform's sign-in and returns what the user acts on.
    pub fn begin(&self) -> Result<SignInRequest, String> {
        #[cfg(target_family = "wasm")]
        {
            Ok(SignInRequest {
                // The console's login page returns to this app (same origin).
                url: login_url(&current_path()),
                user_code: None,
                expires_in: 300,
                interval: 1,
            })
        }
        #[cfg(not(target_family = "wasm"))]
        {
            let (request, pending) = self.begin_desktop()?;
            *lock(&self.state) = State::Waiting(pending);
            Ok(request)
        }
    }

    /// One check on the sign-in started by [`SupabaseAccount::begin`].
    pub fn poll(&self) -> Result<Poll, String> {
        #[cfg(target_family = "wasm")]
        {
            return Ok(match load() {
                Some(session) if session.is_expired() => Poll::Expired,
                Some(session) => Poll::Success(account_from_session(&session)),
                None => Poll::Pending,
            });
        }
        #[cfg(not(target_family = "wasm"))]
        {
            self.poll_desktop()
        }
    }

    /// The signed-in account, when there is a non-expired one.
    pub fn account(&self) -> Option<Account> {
        #[cfg(target_family = "wasm")]
        let account = load()
            .map(|session| account_from_session(&session))
            .filter(|account| !account.is_expired());
        #[cfg(not(target_family = "wasm"))]
        let account = match &*lock(&self.state) {
            State::SignedIn(account) if !account.is_expired() => Some(account.clone()),
            _ => None,
        };
        account
    }

    /// The bearer token for the site's own services while signed in.
    pub fn access_token(&self) -> Option<String> {
        self.account().map(|account| account.access_token)
    }

    /// Ends the session.
    pub fn logout(&self) {
        #[cfg(target_family = "wasm")]
        clear();
        #[cfg(not(target_family = "wasm"))]
        {
            if let Some(path) = &self.config.session_path {
                let _ = std::fs::remove_file(path);
            }
            *lock(&self.state) = State::Idle;
        }
    }

    /// Opens `url` in the system browser (the desktop sign-in hand-off).
    #[cfg(not(target_family = "wasm"))]
    pub fn open_url(url: &str) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        let (program, args) = ("open", vec![url.to_string()]);
        #[cfg(target_os = "windows")]
        let (program, args) = (
            "cmd",
            vec!["/C".to_string(), "start".to_string(), String::new(), url.to_string()],
        );
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let (program, args) = ("xdg-open", vec![url.to_string()]);
        std::process::Command::new(program)
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open {url}: {e}"))
    }

    /// Binds the loopback port and builds Supabase's authorize URL.
    #[cfg(not(target_family = "wasm"))]
    fn begin_desktop(&self) -> Result<(SignInRequest, Pending), String> {
        use std::net::{Ipv4Addr, TcpListener};
        use std::time::{Duration, Instant};

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, self.config.redirect_port))
            .and_then(|listener| listener.set_nonblocking(true).map(|()| listener))
            .map_err(|e| format!("could not listen for the browser: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("could not listen for the browser: {e}"))?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}{}", self.config.redirect_path);
        let verifier = random_token()?;
        let state = random_token()?;
        let challenge = crate::auth_code::code_challenge(&verifier);

        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("provider", &self.config.provider);
        query.append_pair("redirect_to", &redirect_uri);
        query.append_pair("state", &state);
        query.append_pair("code_challenge", &challenge);
        query.append_pair("code_challenge_method", "S256");
        let url = format!(
            "{}/auth/v1/authorize?{}",
            self.config.url.trim_end_matches('/'),
            query.finish()
        );
        Ok((
            SignInRequest {
                url,
                user_code: None,
                expires_in: 300,
                interval: 1,
            },
            Pending {
                listener,
                state,
                verifier,
                deadline: Instant::now() + Duration::from_secs(300),
            },
        ))
    }

    /// One loopback poll: read the redirect, trade the code for tokens.
    #[cfg(not(target_family = "wasm"))]
    fn poll_desktop(&self) -> Result<Poll, String> {
        use crate::auth_code::{read_callback, read_callback_query, respond, Callback, CallbackEnd};
        use std::io::ErrorKind;
        use std::time::Instant;

        let mut state = lock(&self.state);
        let pending = match std::mem::take(&mut *state) {
            State::Waiting(pending) => pending,
            other => {
                *state = other;
                return Ok(Poll::Pending);
            }
        };
        if Instant::now() >= pending.deadline {
            return Ok(Poll::Expired);
        }
        let mut stream = match pending.listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                *state = State::Waiting(pending);
                return Ok(Poll::Pending);
            }
            Err(e) => return Err(format!("could not read the browser's reply: {e}")),
        };
        let Some(query) = read_callback_query(&mut stream, &self.config.redirect_path) else {
            respond(&mut stream, "404 Not Found", "Not found.");
            *state = State::Waiting(pending);
            return Ok(Poll::Pending);
        };
        match read_callback(&query, &pending.state) {
            Callback::Forged => {
                respond(
                    &mut stream,
                    "400 Bad Request",
                    "This sign-in request is not valid.",
                );
                *state = State::Waiting(pending);
                Ok(Poll::Pending)
            }
            Callback::Over(end) => {
                respond(
                    &mut stream,
                    "200 OK",
                    "Sign-in did not complete. You can close this tab.",
                );
                match end {
                    CallbackEnd::Denied => Ok(Poll::Denied),
                    CallbackEnd::Failed(message) => Ok(Poll::Failed(message)),
                    CallbackEnd::NoCode => Ok(Poll::Failed(
                        "the sign-in page returned no code".to_string(),
                    )),
                }
            }
            Callback::Code(code) => {
                let outcome = exchange(&self.config, &code, &pending.verifier);
                match outcome {
                    Ok(account) => {
                        respond(
                            &mut stream,
                            "200 OK",
                            "Signed in. You can close this tab.",
                        );
                        persist_session(&self.config, &account);
                        *state = State::SignedIn(account.clone());
                        Ok(Poll::Success(account))
                    }
                    Err(error) => {
                        respond(
                            &mut stream,
                            "200 OK",
                            "Sign-in failed. Return to the app and try again.",
                        );
                        Ok(Poll::Failed(error))
                    }
                }
            }
        }
    }
}

/// The token response of `POST /auth/v1/token?grant_type=pkce`.
#[cfg(not(target_family = "wasm"))]
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    user: TokenUser,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Deserialize)]
struct TokenUser {
    id: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    user_metadata: serde_json::Value,
}

/// Trades the authorization code for the account (native).
#[cfg(not(target_family = "wasm"))]
fn exchange(config: &Config, code: &str, verifier: &str) -> Result<Account, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        // The token endpoint answers 400 with a JSON `error` for a bad code;
        // keep the body readable.
        .http_status_as_error(false)
        .build()
        .new_agent();
    let url = format!(
        "{}/auth/v1/token?grant_type=pkce",
        config.url.trim_end_matches('/')
    );
    let response = agent
        .post(&url)
        .header("apikey", &config.anon_key)
        .send_form([("auth_code", code), ("code_verifier", verifier)])
        .map_err(|e| format!("the token request failed: {e}"))?;
    let status = response.status().as_u16();
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("could not read the token response: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "the token endpoint refused the code ({status}): {}",
            body.chars().take(200).collect::<String>()
        ));
    }
    let parsed: TokenResponse = serde_json::from_str(&body)
        .map_err(|e| format!("the token response is not JSON: {e}"))?;
    let name = ["name", "full_name", "preferred_username"]
        .iter()
        .find_map(|key| {
            parsed
                .user
                .user_metadata
                .get(*key)
                .and_then(|value| value.as_str())
        })
        .unwrap_or(&parsed.user.id)
        .to_string();
    Ok(Account {
        id: parsed.user.id,
        name,
        email: parsed.user.email,
        access_token: parsed.access_token,
        refresh_token: parsed.refresh_token,
        expires_at: parsed.expires_in.map(|secs| now_unix() + secs as i64),
    })
}

/// Keeps the signed-in account between runs. A write failure is not fatal:
/// the session still works for this run.
#[cfg(not(target_family = "wasm"))]
fn persist_session(config: &Config, account: &Account) {
    if let Some(path) = &config.session_path {
        let _ = save_session(path, account);
    }
}

/// Reads the persisted session, when there is one.
#[cfg(not(target_family = "wasm"))]
fn restore_session(path: Option<&std::path::Path>) -> Option<Account> {
    let path = path?;
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes the session atomically and tightens the file to its owner.
#[cfg(not(target_family = "wasm"))]
fn save_session(path: &std::path::Path, account: &Account) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(account).map_err(|e| e.to_string())?;
    crate::atomic::write(path, json.as_bytes()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Maps the shared web session onto the account.
#[cfg(target_family = "wasm")]
fn account_from_session(session: &Session) -> Account {
    Account {
        id: session.user_id.clone(),
        name: session.display_name().to_string(),
        email: session.email.clone(),
        access_token: session.access_token.clone(),
        refresh_token: session.refresh_token.clone(),
        expires_at: Some(session.expires_at),
    }
}

/// 256 random bits, URL-safe: long enough for a PKCE verifier.
#[cfg(not(target_family = "wasm"))]
fn random_token() -> Result<String, String> {
    use base64::Engine as _;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no secure randomness available: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// A lock that survives a panic elsewhere.
#[cfg(not(target_family = "wasm"))]
fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
