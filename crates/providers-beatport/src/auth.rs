//! Beatport OAuth, the only part of this provider that is not a plain GET.
//!
//! Beatport has no self-serve developer tier: API credentials are issued to
//! approved partners. What is left for an ordinary user is the client the store
//! ships for its own API documentation page — a public OAuth client, usable with
//! the *authorization-code* grant against the user's own account. So TagRex
//! never holds a Beatport password: the shell opens
//! [`authorize_url`] in a window, the user signs in on Beatport's own page, and
//! the redirect carries back a one-time code that [`exchange_code`] trades for a
//! token.
//!
//! The client id is not published as a constant anywhere, so it is read from the
//! documentation page's own scripts ([`fetch_client_id`]). That is the fragile
//! part of the arrangement and the reason this whole crate is isolated: when it
//! breaks, it breaks alone.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tagrex_core::provider::ProviderError;

use crate::{status_to_error, API_BASE, USER_AGENT};

/// Where Beatport sends the browser back with the authorization code. It has to
/// be one the public client already has registered, so it is theirs, not ours —
/// the shell watches its own window for a navigation to this prefix instead of
/// listening on a local port.
pub const REDIRECT_URI: &str = "https://api.beatport.com/v4/auth/o/post-message/";

/// Refresh this many seconds before the token actually expires, so a request
/// never starts with a token that dies mid-flight.
const EXPIRY_BUFFER_SECS: u64 = 60;

/// An access token plus what is needed to renew it. Persisted by the shell in
/// the OS app-config directory — the same place the Discogs token lives, and
/// never in the repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatportToken {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds. Absolute rather than a duration so it survives being
    /// written to disk and read back in another session.
    pub expires_at: u64,
}

impl BeatportToken {
    /// Whether the token is expired, or close enough that it should be renewed
    /// before the next request.
    pub fn is_expired(&self) -> bool {
        now_unix() + EXPIRY_BUFFER_SECS >= self.expires_at
    }

    fn from_response(body: &str) -> Result<Self, ProviderError> {
        let value: serde_json::Value = serde_json::from_str(body)
            .map_err(|err| ProviderError::Other(format!("malformed token response: {err}")))?;
        let string = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
        };
        let (Some(access_token), Some(refresh_token)) =
            (string("access_token"), string("refresh_token"))
        else {
            // Beatport answers a rejected grant with a JSON error object, which
            // is far more useful to the user than "missing field".
            let detail = string("error_description")
                .or_else(|| string("error"))
                .unwrap_or_else(|| "no access token in response".to_string());
            return Err(ProviderError::Auth(detail));
        };
        let expires_in = value
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600);
        Ok(Self {
            access_token,
            refresh_token,
            expires_at: now_unix() + expires_in,
        })
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// Build the agent the auth calls use. Separate from the provider's own agent
/// because authorizing happens before there is a token to build a provider
/// with.
pub fn agent(proxy: Option<&str>) -> Result<ureq::Agent, ProviderError> {
    let mut builder = ureq::Agent::config_builder().http_status_as_error(false);
    if let Some(proxy) = proxy.filter(|p| !p.trim().is_empty()) {
        let proxy = ureq::Proxy::new(proxy.trim())
            .map_err(|err| ProviderError::Network(format!("invalid proxy: {err}")))?;
        builder = builder.proxy(Some(proxy));
    }
    Ok(ureq::Agent::new_with_config(builder.build()))
}

/// The URL to open in a window for the user to sign in. On success Beatport
/// redirects to [`REDIRECT_URI`] with `?code=…`.
pub fn authorize_url(client_id: &str) -> String {
    format!(
        "{API_BASE}/auth/o/authorize/?response_type=code&client_id={client_id}&redirect_uri={REDIRECT_URI}"
    )
}

/// Pull the authorization code out of the URL the window was redirected to.
/// `None` for any other URL, which is what the navigation handler sees for every
/// step of the login itself.
pub fn code_from_redirect(url: &str) -> Option<String> {
    if !url.starts_with(REDIRECT_URI) {
        return None;
    }
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "code" && !value.is_empty()).then(|| value.to_string())
    })
}

/// Read the public client id out of the API documentation page's scripts.
///
/// Two steps, because the id lives in a hashed bundle whose name changes: fetch
/// the page, then each script it references, until one carries the id.
pub fn fetch_client_id(agent: &ureq::Agent) -> Result<String, ProviderError> {
    let html = get_text(agent, &format!("{API_BASE}/docs/"))?;
    let mut last_error = None;
    for src in script_sources(&html) {
        let url = if src.starts_with("http") {
            src.clone()
        } else {
            format!("https://api.beatport.com{src}")
        };
        match get_text(agent, &url) {
            Ok(js) => {
                if let Some(id) = client_id_in(&js) {
                    return Ok(id);
                }
            }
            Err(err) => last_error = Some(err),
        }
    }
    Err(ProviderError::Auth(match last_error {
        Some(err) => format!("could not read the Beatport client id ({err})"),
        None => "could not read the Beatport client id".to_string(),
    }))
}

/// Trade the one-time code from the redirect for a token.
pub fn exchange_code(
    agent: &ureq::Agent,
    client_id: &str,
    code: &str,
) -> Result<BeatportToken, ProviderError> {
    post_token(
        agent,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", client_id),
        ],
    )
}

/// Renew an expired token without involving the user.
pub fn refresh(
    agent: &ureq::Agent,
    client_id: &str,
    refresh_token: &str,
) -> Result<BeatportToken, ProviderError> {
    post_token(
        agent,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ],
    )
}

/// The account a token belongs to — shown in settings so "signed in" names who,
/// and used to verify a stored token still works.
pub fn account_username(agent: &ureq::Agent, access_token: &str) -> Result<String, ProviderError> {
    let mut response = agent
        .get(&format!("{API_BASE}/my/account/"))
        .header("Authorization", &format!("Bearer {access_token}"))
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|err| ProviderError::Network(err.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(status_to_error(status, None));
    }
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|err| ProviderError::Network(err.to_string()))?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|err| ProviderError::Other(format!("malformed account response: {err}")))?;
    Ok(value
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string())
}

/// Both grants are the same request with different parameters. Beatport's token
/// endpoint takes them in the query string, not a form body.
fn post_token(
    agent: &ureq::Agent,
    params: &[(&str, &str)],
) -> Result<BeatportToken, ProviderError> {
    let mut request = agent
        .post(&format!("{API_BASE}/auth/o/token/"))
        .header("User-Agent", USER_AGENT);
    for (key, value) in params {
        request = request.query(*key, *value);
    }
    let mut response = request
        .send_empty()
        .map_err(|err| ProviderError::Network(err.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|err| ProviderError::Network(err.to_string()))?;
    if !(200..300).contains(&status) {
        // The body usually explains why far better than the status does, so try
        // it first and fall back to the plain status mapping.
        return Err(match BeatportToken::from_response(&body) {
            Err(ProviderError::Auth(detail)) => ProviderError::Auth(detail),
            _ => status_to_error(status, None),
        });
    }
    BeatportToken::from_response(&body)
}

fn get_text(agent: &ureq::Agent, url: &str) -> Result<String, ProviderError> {
    let mut response = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|err| ProviderError::Network(err.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(status_to_error(status, None));
    }
    response
        .body_mut()
        .read_to_string()
        .map_err(|err| ProviderError::Network(err.to_string()))
}

/// Every `src="…js"` in a page, in document order.
fn script_sources(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("src=") {
        rest = &rest[at + 4..];
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let Some(end) = rest[1..].find(quote) else {
            break;
        };
        let src = &rest[1..=end];
        if src.ends_with(".js") {
            out.push(src.to_string());
        }
        rest = &rest[end + 1..];
    }
    out
}

/// The id as the documentation bundle spells it: `API_CLIENT_ID: '…'`.
fn client_id_in(js: &str) -> Option<String> {
    let at = js.find("API_CLIENT_ID")?;
    let rest = &js[at..];
    let open = rest.find(['\'', '"'])?;
    let quote = rest[open..].chars().next()?;
    let end = rest[open + 1..].find(quote)?;
    let id = &rest[open + 1..open + 1 + end];
    (!id.is_empty()).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_code_out_of_the_redirect() {
        assert_eq!(
            code_from_redirect(&format!("{REDIRECT_URI}?code=abc123&state=x")),
            Some("abc123".to_string())
        );
        assert_eq!(
            code_from_redirect(&format!("{REDIRECT_URI}?state=x&code=abc123")),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn ignores_every_url_that_is_not_the_redirect() {
        // The login pages the user walks through, and the redirect itself when
        // it carries an error instead of a code.
        assert_eq!(code_from_redirect("https://www.beatport.com/account"), None);
        assert_eq!(code_from_redirect(&authorize_url("client")), None);
        assert_eq!(
            code_from_redirect(&format!("{REDIRECT_URI}?error=access_denied")),
            None
        );
        assert_eq!(code_from_redirect(&format!("{REDIRECT_URI}?code=")), None);
    }

    /// The one part of this crate that cannot be proven with a fixture: the
    /// client id is read out of a page that can change under us at any time, and
    /// when it does, signing in stops working. Ignored so CI stays offline —
    /// run it by hand (`cargo test -p tagrex-providers-beatport -- --ignored`)
    /// when sign-in starts failing, to tell "they moved the id" apart from
    /// "the account or the grant is the problem".
    #[test]
    #[ignore = "hits the live documentation page"]
    fn reads_the_client_id_off_the_live_docs_page() {
        let agent = agent(None).unwrap();
        let id = fetch_client_id(&agent).expect("no client id on the docs page");
        assert!(!id.is_empty());
    }

    #[test]
    fn finds_the_client_id_in_a_bundle() {
        assert_eq!(
            client_id_in("window.ui=SwaggerUIBundle({API_CLIENT_ID: 'abcDEF123',url:'x'})"),
            Some("abcDEF123".to_string())
        );
        assert_eq!(client_id_in("nothing to see"), None);
    }

    #[test]
    fn collects_script_sources_and_skips_other_assets() {
        let html = r#"<link href="x.css"><script src="/static/main.9f8.js"></script>
                      <img src="logo.png"><script src='/static/other.js'></script>"#;
        assert_eq!(
            script_sources(html),
            vec![
                "/static/main.9f8.js".to_string(),
                "/static/other.js".to_string()
            ]
        );
    }

    #[test]
    fn a_token_response_without_a_token_is_an_auth_error() {
        let err = BeatportToken::from_response(
            r#"{"error":"invalid_grant","error_description":"Authorization code is invalid"}"#,
        )
        .unwrap_err();
        assert!(matches!(err, ProviderError::Auth(detail) if detail.contains("invalid")));
    }

    #[test]
    fn a_fresh_token_is_not_expired_and_a_past_one_is() {
        let token = BeatportToken::from_response(
            r#"{"access_token":"a","refresh_token":"r","expires_in":3600}"#,
        )
        .unwrap();
        assert!(!token.is_expired());
        let stale = BeatportToken {
            expires_at: now_unix(),
            ..token
        };
        assert!(stale.is_expired());
    }
}
