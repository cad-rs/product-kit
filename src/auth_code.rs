//! The browser sign-in redirect's loopback HTTP mechanics and the PKCE
//! helpers.
//!
//! Shared by the workbench framework's desktop client and the nanobot
//! harness's auth-code flow; it replaced their two copies. The transport
//! (workbench's injectable sync HTTP vs nanobot's async reqwest), the web
//! callback variant and the token/session ownership stay in each framework.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest as _, Sha256};

/// RFC 7636 S256 code challenge for a PKCE verifier.
pub fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// What the query of a redirect says about the sign-in with `state`.
#[derive(Debug, PartialEq)]
pub enum Callback {
    /// Not this sign-in's: only the browser it was sent to knows the state.
    Forged,
    /// The sign-in ended at the page without a code.
    Over(CallbackEnd),
    /// The page authorized; here is the code.
    Code(String),
}

/// How a sign-in ended at the page without a code.
#[derive(Debug, PartialEq)]
pub enum CallbackEnd {
    /// The user refused (`error=access_denied`).
    Denied,
    /// The page returned some other error.
    Failed(String),
    /// Neither `code` nor `error`.
    NoCode,
}

/// Parse a redirect query against the `state` this sign-in started with.
pub fn read_callback(query: &str, state: &str) -> Callback {
    let param = |name: &str| {
        form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if param("state").as_deref() != Some(state) {
        return Callback::Forged;
    }
    match (param("error"), param("code")) {
        (Some(error), _) if error == "access_denied" => {
            Callback::Over(CallbackEnd::Denied)
        }
        (Some(error), _) => Callback::Over(CallbackEnd::Failed(format!(
            "The sign-in page returned an error: {error}"
        ))),
        (None, Some(code)) => Callback::Code(code),
        (None, None) => Callback::Over(CallbackEnd::NoCode),
    }
}

/// The query string of a `GET` for `path`; None for any other request. The
/// whole header is read, because closing a socket with unread data can reset
/// the connection before the browser sees the response.
#[cfg(not(target_family = "wasm"))]
pub fn read_callback_query(stream: &mut std::net::TcpStream, path: &str) -> Option<String> {
    use std::io::{BufRead, BufReader, Read as _};
    use std::time::Duration;

    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut reader = BufReader::new(stream.take(16 * 1024));
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let mut line = String::new();
    while matches!(reader.read_line(&mut line), Ok(n) if n > 0) && line.trim_end() != "" {
        line.clear();
    }
    let target = request_line.strip_prefix("GET ")?.split_whitespace().next()?;
    let (target_path, query) = target.split_once('?')?;
    (target_path == path).then(|| query.to_string())
}

/// Write the fixed sign-in reply. `message` is always fixed text: nothing from
/// the request reaches the page.
#[cfg(not(target_family = "wasm"))]
pub fn respond(stream: &mut std::net::TcpStream, status: &str, message: &str) {
    use std::io::Write as _;

    let body = format!("<!doctype html><meta charset=utf-8><title>Sign-in</title><p>{message}</p>");
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirect_is_read_only_if_it_carries_this_sign_ins_state() {
        assert_eq!(read_callback("code=c&state=other", "s"), Callback::Forged);
        assert_eq!(read_callback("code=c", "s"), Callback::Forged);
        assert_eq!(
            read_callback("code=a%2Fb&state=s", "s"),
            Callback::Code("a/b".to_string())
        );
        assert_eq!(
            read_callback("error=access_denied&state=s", "s"),
            Callback::Over(CallbackEnd::Denied)
        );
        assert!(matches!(
            read_callback("error=server_error&state=s", "s"),
            Callback::Over(CallbackEnd::Failed(detail)) if detail.contains("server_error")
        ));
        assert_eq!(
            read_callback("state=s", "s"),
            Callback::Over(CallbackEnd::NoCode)
        );
    }

    /// RFC 7636 Appendix B.
    #[test]
    fn the_challenge_matches_the_rfc_example() {
        assert_eq!(
            code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
