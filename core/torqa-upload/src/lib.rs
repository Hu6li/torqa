//! Uploading rides to training platforms (R29), each behind [`ActivityUploader`]:
//! intervals.icu ([`intervals::IntervalsIcu`]) first. Adding a platform means an uploader and
//! an entry in [`platforms`]; storage, the app layer and the interface take it as it is.

pub mod intervals;

use std::fmt::Write as _;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::Arc;
use std::time::Duration;

pub use torqa_domain::upload::{
    Account, Activity, ActivityUploader, BoxFuture, Credentials, Field, UploadError, Uploaded,
};

/// Every platform rides can be uploaded to, in the order they are offered to the rider.
#[must_use]
pub fn platforms() -> Vec<Arc<dyn ActivityUploader>> {
    vec![Arc::new(intervals::IntervalsIcu::new())]
}

/// The HTTP client of the uploaders: a ride's FIT file is small, a slow platform is not.
fn client() -> Result<reqwest::Client, UploadError> {
    reqwest::Client::builder()
        .user_agent(concat!(
            "Torqa/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/bossm8/torqa)"
        ))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| UploadError::Unreachable(format!("No HTTP client: {e}")))
}

/// `platform` cannot be reached.
fn cannot_reach(platform: &str, error: &reqwest::Error) -> UploadError {
    UploadError::Unreachable(format!(
        "Cannot reach {platform}: check the internet connection ({error})."
    ))
}

/// What a platform's error answer says, short: the usual JSON fields where it has them.
fn describe(answer: &str) -> String {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(answer) else {
        return answer.trim().chars().take(200).collect();
    };
    let text = |key: &str| json.get(key).and_then(serde_json::Value::as_str);
    text("error_description")
        .or_else(|| text("message"))
        .or_else(|| text("error"))
        .unwrap_or_default()
        .to_owned()
}

/// A `multipart/form-data` body with one file: its content type (with the boundary) and bytes.
/// Written here rather than with reqwest's `multipart` feature, which would bring in more
/// crates for one small body.
fn multipart(field: &str, filename: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    // Long and random enough not to occur in a FIT file by chance.
    let boundary = format!("torqa-{}", random_text());
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; \
         filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// A URL query of `pairs`, percent-encoded.
fn query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `text` percent-encoded for a URL's query.
fn encode(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// 32 random hex digits. The standard library seeds its hashers from the system's random
/// source, which is random enough here and needs no further crate.
fn random_text() -> String {
    let half = || RandomState::new().build_hasher().finish();
    format!("{:016x}{:016x}", half(), half())
}

#[cfg(test)]
pub(crate) mod testing {
    //! A stand-in for a platform's server.

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    /// Answers one request after the other with `answers` (status line and JSON body); returns
    /// its address and, once done, the requests as received.
    pub async fn serve(answers: Vec<(&'static str, String)>) -> (String, Served) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let site = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, body) in answers {
                let (mut socket, _) = listener.accept().await.unwrap();
                requests.push(read_request(&mut socket).await);
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
            requests
        });
        (site, task)
    }

    /// The requests a [`serve`] received, raw.
    pub type Served = tokio::task::JoinHandle<Vec<Vec<u8>>>;

    /// One HTTP request, with its body.
    async fn read_request(socket: &mut TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let read = socket.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..read]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|l| l.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
            if read == 0 {
                break;
            }
        }
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_bodies_carry_the_file_between_boundaries() {
        let (content_type, body) = multipart("file", "r.fit", &[0, 1, 2]);

        let boundary = content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with(&format!("--{boundary}\r\n")));
        assert!(text.contains("name=\"file\"; filename=\"r.fit\""));
        assert!(body.windows(3).any(|w| w == [0, 1, 2]));
        assert!(text.ends_with(&format!("\r\n--{boundary}--\r\n")));
    }

    #[test]
    fn boundaries_differ_between_uploads() {
        assert_ne!(multipart("f", "a", b"").0, multipart("f", "a", b"").0);
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(
            query(&[("name", "Zone 2 & more/ä"), ("external_id", "torqa-1")]),
            "name=Zone%202%20%26%20more%2F%C3%A4&external_id=torqa-1"
        );
    }

    #[test]
    fn error_answers_are_told_short() {
        assert_eq!(
            describe(r#"{"error":"invalid_grant","error_description":"Code expired"}"#),
            "Code expired"
        );
        assert_eq!(
            describe(r#"{"status":403,"error":"Forbidden"}"#),
            "Forbidden"
        );
        assert_eq!(describe(" Bad Gateway\n"), "Bad Gateway");
    }
}
