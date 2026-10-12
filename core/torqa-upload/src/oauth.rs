//! Logging the rider in on a platform's website (OAuth 2.0 authorization code, RFC 6749)
//! without a server of Torqa's: for the login only, Torqa listens on the loopback address at
//! a free port, and the platform sends the browser back there (RFC 8252). The `state` ties
//! the answer to the request. Access tokens are renewed shortly before they expire, and
//! disconnecting revokes the grant (RFC 7009).

use std::collections::BTreeMap;
use std::future::Future;
use std::net::Ipv4Addr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::{Browser, UploadError, cannot_reach, client, decode, describe, query, random_text};

/// How long the rider has to log in.
pub const LOGIN_TIME: Duration = Duration::from_mins(5);
/// How long a connection to the loopback address may take to send its request. Browsers open
/// connections ahead of time that may never send one.
const REQUEST_TIME: Duration = Duration::from_secs(3);
/// Access tokens are renewed when they have less than this left.
const RENEW_BEFORE: Duration = Duration::from_mins(5);
/// The path the browser comes back to.
const CALLBACK: &str = "/torqa";

/// What a login came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// The authorization code, to trade for tokens.
    pub code: String,
    /// The address the browser came back to; the token request names it again.
    pub redirect_uri: String,
    /// The scopes the rider granted, as the platform lists them; may be fewer than asked for.
    pub scope: String,
}

/// An access token, the token that renews it, and when the access token expires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    /// Sent with every request.
    pub access: String,
    /// Trades for a new access token.
    pub refresh: String,
    /// When the access token expires, in seconds since 1970.
    pub expires_at: u64,
}

impl Tokens {
    /// Whether the access token is to be renewed before using it.
    #[must_use]
    pub fn expiring(&self) -> bool {
        now() + RENEW_BEFORE.as_secs() >= self.expires_at
    }
}

/// Logs the rider in at `authorize` (the platform's login with its own parameters, e.g. the
/// client id and scope; the redirect and the state are added here) in the rider's browser,
/// within `within`. `finish` then trades the [`Grant`] for an account while the browser waits,
/// so that the page it shows says how it really ended.
///
/// # Errors
/// [`UploadError::Rejected`] if the login was declined, came back for another request or not
/// at all in time; whatever `finish` fails with.
pub async fn log_in<T, F>(
    authorize: &str,
    browser: &Browser,
    within: Duration,
    finish: impl FnOnce(Grant) -> F,
) -> Result<T, UploadError>
where
    F: Future<Output = Result<T, UploadError>>,
{
    let cannot_wait = |e: std::io::Error| {
        UploadError::Rejected(format!("Cannot wait for the login to come back: {e}"))
    };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(cannot_wait)?;
    // An address rather than `localhost`, which may lead the browser to IPv6 first.
    let redirect_uri = format!(
        "http://127.0.0.1:{}{CALLBACK}",
        listener.local_addr().map_err(cannot_wait)?.port()
    );
    let state = random_text();
    let separator = if authorize.contains('?') { '&' } else { '?' };
    (browser.open)(&format!(
        "{authorize}{separator}{}",
        query(&[("redirect_uri", &redirect_uri), ("state", &state)])
    ));
    let (mut socket, answer) = tokio::time::timeout(within, come_back(&listener, &state))
        .await
        .map_err(|_| {
            UploadError::Rejected("The login was not finished in time: connect again.".to_owned())
        })?
        .map_err(cannot_wait)?;
    let result = match answer {
        Ok((code, scope)) => {
            finish(Grant {
                code,
                redirect_uri,
                scope,
            })
            .await
        }
        Err(error) => Err(error),
    };
    let page = match &result {
        Ok(_) => page(&browser.connected_page, ""),
        Err(error) => page(&browser.failed_page, &error.to_string()),
    };
    // The browser may have gone; the rider sees the outcome in Torqa anyway.
    let _ = reply(&mut socket, "200 OK", "text/html; charset=utf-8", &page).await;
    result
}

/// Waits for the browser to come back to the callback with the answer to the login with
/// `state`: the connection to answer on, and the code with the granted scope, or why not.
async fn come_back(
    listener: &TcpListener,
    state: &str,
) -> std::io::Result<(TcpStream, Result<(String, String), UploadError>)> {
    loop {
        let (mut socket, _) = listener.accept().await?;
        let Ok(Some(target)) =
            tokio::time::timeout(REQUEST_TIME, request_target(&mut socket)).await
        else {
            continue;
        };
        let (path, parameters) = target.split_once('?').unwrap_or((&target, ""));
        if path != CALLBACK {
            // E.g. the browser asking for an icon.
            let _ = reply(&mut socket, "404 Not Found", "text/plain", "").await;
            continue;
        }
        let parameters: BTreeMap<String, String> = parameters
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .map(|(name, value)| (decode(name), decode(value)))
            .collect();
        let get = |name: &str| parameters.get(name).map(String::as_str);
        let answer = if get("state") != Some(state) {
            Err(UploadError::Rejected(
                "The login came back for another request: connect again.".to_owned(),
            ))
        } else if let Some(error) = get("error") {
            Err(UploadError::Rejected(if error == "access_denied" {
                "You did not let Torqa in.".to_owned()
            } else {
                format!("The login failed ({error}).")
            }))
        } else if let Some(code) = get("code") {
            Ok((code.to_owned(), get("scope").unwrap_or_default().to_owned()))
        } else {
            Err(UploadError::Rejected(
                "The login came back without its code: connect again.".to_owned(),
            ))
        };
        return Ok((socket, answer));
    }
}

/// The target of an HTTP request, e.g. `/torqa?code=…`; `None` if it is not one.
async fn request_target(socket: &mut TcpStream) -> Option<String> {
    let mut request = Vec::new();
    let mut buffer = [0; 2048];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 16 * 1024 {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let text = String::from_utf8_lossy(&request);
    let mut line = text.lines().next()?.split(' ');
    match (line.next(), line.next()) {
        (Some("GET"), Some(target)) => Some(target.to_owned()),
        _ => None,
    }
}

async fn reply(
    socket: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let answer = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(answer.as_bytes()).await?;
    socket.shutdown().await
}

/// The page the browser shows after the login: `text`, and `why` if it failed.
fn page(text: &str, why: &str) -> String {
    let why = if why.is_empty() {
        String::new()
    } else {
        format!("<p class=\"why\">{}</p>", escape(why))
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Torqa</title><style>\
         body{{font-family:system-ui,sans-serif;background:#111318;color:#f0f2f7;\
         display:flex;align-items:center;justify-content:center;height:100vh;margin:0}}\
         main{{max-width:34rem;padding:2rem;text-align:center;line-height:1.5}}\
         .why{{color:#a3adbd}}</style></head><body><main><h1>Torqa</h1><p>{}</p>{why}\
         </main></body></html>",
        escape(text)
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Asks `platform`'s token endpoint `url` for tokens with the form `pairs`: a code to trade
/// or a refresh token. Returns them and the whole answer, which may say more, e.g. who logged
/// in. A refresh token the answer leaves out stays `keep_refresh`.
///
/// # Errors
/// [`UploadError::Unauthorized`] if the platform refuses the grant or the client,
/// [`UploadError::Unreachable`] if it cannot be reached.
pub async fn request_tokens(
    platform: &str,
    url: &str,
    pairs: &[(&str, &str)],
    keep_refresh: &str,
) -> Result<(Tokens, serde_json::Value), UploadError> {
    let answer = client()?
        .post(url)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(query(pairs))
        .send()
        .await
        .map_err(|e| cannot_reach(platform, &e))?;
    let status = answer.status();
    let text = answer
        .text()
        .await
        .map_err(|e| cannot_reach(platform, &e))?;
    if status.is_client_error() {
        return Err(UploadError::Unauthorized(format!(
            "{platform} no longer lets Torqa in: {} ({status}).",
            describe(&text)
        )));
    }
    if !status.is_success() {
        return Err(UploadError::Rejected(format!(
            "{platform} refused: {} ({status}).",
            describe(&text)
        )));
    }
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        UploadError::Rejected(format!("{platform} answered something unexpected ({e})."))
    })?;
    let text = |key: &str| json.get(key).and_then(serde_json::Value::as_str);
    let access = text("access_token")
        .ok_or_else(|| UploadError::Rejected(format!("{platform} gave no access token.")))?;
    let expires_at = json
        .get("expires_at")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            json.get("expires_in")
                .and_then(serde_json::Value::as_u64)
                .map(|seconds| now() + seconds)
        })
        .unwrap_or(0);
    let tokens = Tokens {
        access: access.to_owned(),
        refresh: text("refresh_token").unwrap_or(keep_refresh).to_owned(),
        expires_at,
    };
    Ok((tokens, json))
}

/// Revokes `token` at `platform`'s revocation endpoint `url`, authenticating as the client
/// with `client_id` and `client_secret`; best effort.
pub async fn revoke(platform: &str, url: &str, client_id: &str, client_secret: &str, token: &str) {
    let sent = async {
        client()?
            .post(url)
            .basic_auth(client_id, Some(client_secret))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(query(&[
                ("token", token),
                ("token_type_hint", "refresh_token"),
            ]))
            .send()
            .await
            .map_err(|e| cannot_reach(platform, &e))?
            .error_for_status()
            .map_err(|e| UploadError::Rejected(e.to_string()))
    };
    if let Err(error) = sent.await {
        tracing::warn!(%error, platform, "cannot revoke Torqa's access");
    }
}

/// Seconds since 1970.
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_renewed_a_little_before_they_expire() {
        let tokens = |left: u64| Tokens {
            access: String::new(),
            refresh: String::new(),
            expires_at: now() + left,
        };

        assert!(!tokens(3600).expiring());
        assert!(tokens(60).expiring());
    }

    #[test]
    fn pages_show_the_text_and_why_safely() {
        let shown = page("Connected <b>", "a & b");

        assert!(shown.contains("<p>Connected &lt;b&gt;</p>"));
        assert!(shown.contains("<p class=\"why\">a &amp; b</p>"));
        assert!(!page("Connected", "").contains("class=\"why\""));
    }
}
