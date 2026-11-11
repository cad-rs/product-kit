//! OAuth 2.0 / OIDC mechanics shared by the frameworks' account backends:
//! the device-flow grant's request shapes and its error vocabulary.
//!
//! Shared by workbench's device-flow client and nanobot's OAuth2 provider. The
//! transport (workbench's injectable sync HTTP vs nanobot's async reqwest), the
//! endpoint discovery, and the token/session ownership stay in each framework.

/// RFC 8628 grant type for the token endpoint.
pub const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// RFC 8628 §3.1 device authorization request form.
pub fn device_authorization_form(
    client_id: &str,
    scopes: &[&str],
    audience: Option<&str>,
) -> Vec<(String, String)> {
    let mut form = vec![("client_id".to_string(), client_id.to_string())];
    if !scopes.is_empty() {
        form.push(("scope".to_string(), scopes.join(" ")));
    }
    if let Some(audience) = audience {
        form.push(("audience".to_string(), audience.to_string()));
    }
    form
}

/// RFC 8628 §3.4 token poll form.
pub fn token_poll_form(device_code: &str, client_id: &str) -> Vec<(String, String)> {
    vec![
        ("grant_type".to_string(), DEVICE_CODE_GRANT.to_string()),
        ("device_code".to_string(), device_code.to_string()),
        ("client_id".to_string(), client_id.to_string()),
    ]
}

/// The RFC 8628 §3.5 error vocabulary, product-neutral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollError {
    /// Keep polling at the same interval.
    Pending,
    /// Slow down: add five seconds to the interval.
    SlowDown,
    /// The user refused.
    Denied,
    /// The device code expired.
    Expired,
    /// Anything else the provider invented.
    Unknown(String),
}

/// Classify the `error` member of a token-endpoint answer.
pub fn classify_poll_error(error: &str) -> PollError {
    match error {
        "authorization_pending" => PollError::Pending,
        "slow_down" => PollError::SlowDown,
        "access_denied" => PollError::Denied,
        "expired_token" => PollError::Expired,
        other => PollError::Unknown(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_device_authorization_form_carries_what_the_provider_needs() {
        let form = device_authorization_form("client-1", &["openid", "email"], Some("aud"));
        assert_eq!(
            form,
            vec![
                ("client_id".to_string(), "client-1".to_string()),
                ("scope".to_string(), "openid email".to_string()),
                ("audience".to_string(), "aud".to_string()),
            ]
        );
        let bare = device_authorization_form("client-1", &[], None);
        assert_eq!(bare, vec![("client_id".to_string(), "client-1".to_string())]);
    }

    #[test]
    fn the_token_poll_form_names_the_device_code_grant() {
        let form = token_poll_form("dev-1", "client-1");
        assert_eq!(
            form,
            vec![
                ("grant_type".to_string(), DEVICE_CODE_GRANT.to_string()),
                ("device_code".to_string(), "dev-1".to_string()),
                ("client_id".to_string(), "client-1".to_string()),
            ]
        );
    }

    #[test]
    fn the_error_vocabulary_covers_the_rfc_and_the_unknown() {
        assert_eq!(classify_poll_error("authorization_pending"), PollError::Pending);
        assert_eq!(classify_poll_error("slow_down"), PollError::SlowDown);
        assert_eq!(classify_poll_error("access_denied"), PollError::Denied);
        assert_eq!(classify_poll_error("expired_token"), PollError::Expired);
        assert_eq!(
            classify_poll_error("something_new"),
            PollError::Unknown("something_new".to_string())
        );
    }
}
