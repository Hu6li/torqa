//! intervals.icu (R29): the rider's personal API key, sent with Basic auth as the user
//! `API_KEY`, as its API documents for riders' own data. Its OAuth would need a client secret,
//! which an open-source app cannot keep (ADR 0013).

use reqwest::StatusCode;

use crate::{
    Account, Activity, ActivityUploader, BoxFuture, Browser, Credentials, Field, UploadError,
    Uploaded, cannot_reach, client, describe, multipart, query,
};

const NAME: &str = "intervals.icu";
const SITE: &str = "https://intervals.icu";
/// The key the API key is stored under.
const API_KEY: &str = "api_key";
// Labels and help are translated by the front end.
// i18n-begin
const FIELDS: &[Field] = &[Field {
    key: API_KEY,
    label: "API key",
    secret: true,
}];
const HELP: &str = "Find your API key on intervals.icu under Settings, Developer Settings.";
// i18n-end

/// Uploads to intervals.icu.
#[derive(Debug, Clone)]
pub struct IntervalsIcu {
    site: String,
}

impl Default for IntervalsIcu {
    fn default() -> Self {
        Self::new()
    }
}

impl IntervalsIcu {
    /// intervals.icu itself.
    #[must_use]
    pub fn new() -> Self {
        Self::with_site(SITE)
    }

    /// intervals.icu at another address, e.g. a stand-in for tests.
    #[must_use]
    pub fn with_site(site: &str) -> Self {
        Self {
            site: site.trim_end_matches('/').to_owned(),
        }
    }

    /// The rider's name for the account with `key`; checks the key on the way.
    async fn athlete(&self, key: &str) -> Result<String, UploadError> {
        let answer = client()?
            .get(format!("{}/api/v1/athlete/0", self.site))
            .basic_auth("API_KEY", Some(key))
            .send()
            .await
            .map_err(|e| cannot_reach(NAME, &e))?;
        let json = answered(answer).await?;
        let text = |key: &str| {
            json.get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
        };
        let full_name = [text("firstname"), text("lastname")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        Ok(text("name")
            .map(ToOwned::to_owned)
            .or_else(|| (!full_name.is_empty()).then_some(full_name))
            .or_else(|| text("id").map(ToOwned::to_owned))
            .unwrap_or_else(|| NAME.to_owned()))
    }

    async fn send(&self, key: &str, activity: &Activity) -> Result<Uploaded, UploadError> {
        let (content_type, body) = multipart(
            &[],
            (
                "file",
                &format!("{}.fit", activity.external_id),
                &activity.fit,
            ),
        );
        let answer = client()?
            .post(format!(
                "{}/api/v1/athlete/0/activities?{}",
                self.site,
                query(&[
                    ("name", &activity.name),
                    ("external_id", &activity.external_id)
                ])
            ))
            .basic_auth("API_KEY", Some(key))
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await
            .map_err(|e| cannot_reach(NAME, &e))?;
        // 201 for a new activity, 200 for one intervals.icu has already (it compares the
        // files): either way the ride is there.
        let json = answered(answer).await?;
        let id = json
            .get("activities")
            .and_then(serde_json::Value::as_array)
            .and_then(|activities| activities.first())
            .unwrap_or(&json)
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                UploadError::Rejected(format!("{NAME} took the ride but named no activity."))
            })?;
        Ok(Uploaded {
            id: id.to_owned(),
            url: format!("{}/activities/{id}", self.site),
        })
    }
}

/// The JSON of a successful answer, or what went wrong.
async fn answered(answer: reqwest::Response) -> Result<serde_json::Value, UploadError> {
    let status = answer.status();
    let text = answer.text().await.map_err(|e| cannot_reach(NAME, &e))?;
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(UploadError::Unauthorized(
            format!("{NAME} does not accept the API key: copy it again from its settings."),
        )),
        status if status.is_success() => serde_json::from_str(&text).map_err(|e| {
            UploadError::Rejected(format!("{NAME} answered something unexpected ({e})."))
        }),
        status => Err(UploadError::Rejected(format!(
            "{NAME} refused: {} ({status}).",
            describe(&text)
        ))),
    }
}

/// The API key in `credentials`, if there is one.
fn api_key(credentials: &Credentials) -> Result<String, UploadError> {
    credentials
        .get(API_KEY)
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .ok_or_else(|| UploadError::Unauthorized(format!("Enter your {NAME} API key.")))
}

impl ActivityUploader for IntervalsIcu {
    fn id(&self) -> &'static str {
        "intervals"
    }

    fn name(&self) -> &'static str {
        NAME
    }

    fn fields(&self) -> &'static [Field] {
        FIELDS
    }

    fn help(&self) -> &'static str {
        HELP
    }

    fn help_url(&self) -> &'static str {
        "https://intervals.icu/settings"
    }

    fn connect(
        &self,
        entered: Credentials,
        _browser: Browser,
    ) -> BoxFuture<'_, Result<Account, UploadError>> {
        Box::pin(async move {
            let key = api_key(&entered)?;
            let label = self.athlete(&key).await?;
            Ok(Account {
                label,
                credentials: Credentials::from([(API_KEY.to_owned(), key)]),
            })
        })
    }

    fn upload<'a>(
        &'a self,
        credentials: &'a mut Credentials,
        activity: &'a Activity,
    ) -> BoxFuture<'a, Result<Uploaded, UploadError>> {
        Box::pin(async move { self.send(&api_key(credentials)?, activity).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::serve;

    fn activity() -> Activity {
        Activity {
            fit: vec![14, 16, 0xD4, 0x08, b'.', b'F', b'I', b'T'],
            name: "Gurten & back".to_owned(),
            external_id: "torqa-20261012-071500".to_owned(),
        }
    }

    /// intervals.icu needs no login in the browser.
    fn no_browser() -> Browser {
        Browser {
            open: Box::new(|url| panic!("opened {url}")),
            connected_page: String::new(),
            failed_page: String::new(),
        }
    }

    fn with_key(key: &str) -> Credentials {
        Credentials::from([(API_KEY.to_owned(), key.to_owned())])
    }

    /// Basic auth as user `API_KEY` with the password `secret`, in Base64 (hyper writes header
    /// names in lower case).
    const AS_SECRET: &str = "authorization: Basic QVBJX0tFWTpzZWNyZXQ=";

    #[tokio::test]
    async fn connecting_checks_the_key_and_names_the_athlete() {
        let (site, served) = serve(vec![(
            "200 OK",
            r#"{"id":"i42","name":"Ann Rider"}"#.to_owned(),
        )])
        .await;

        let account = IntervalsIcu::with_site(&site)
            .connect(with_key(" secret "), no_browser())
            .await
            .unwrap();

        assert_eq!(account.label, "Ann Rider");
        assert_eq!(account.credentials, with_key("secret"));
        let request = String::from_utf8(served.await.unwrap().remove(0)).unwrap();
        assert!(request.starts_with("GET /api/v1/athlete/0 "));
        assert!(request.contains(AS_SECRET), "{request}");
    }

    #[tokio::test]
    async fn a_wrong_key_does_not_connect() {
        let (site, _served) = serve(vec![("401 Unauthorized", "{}".to_owned())]).await;

        let error = IntervalsIcu::with_site(&site)
            .connect(with_key("secret"), no_browser())
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Unauthorized(_)), "{error:?}");
    }

    #[tokio::test]
    async fn no_key_asks_for_one_without_asking_intervals_icu() {
        let error = IntervalsIcu::with_site("http://127.0.0.1:9")
            .connect(with_key("  "), no_browser())
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Unauthorized(_)), "{error:?}");
    }

    #[tokio::test]
    async fn uploads_the_fit_file_under_the_rides_name_and_links_the_activity() {
        let (site, served) = serve(vec![(
            "201 Created",
            r#"{"icu_athlete_id":"i42","id":"i777","activities":[{"icu_athlete_id":"i42","id":"i777"}]}"#
                .to_owned(),
        )])
        .await;

        let uploaded = IntervalsIcu::with_site(&site)
            .upload(&mut with_key("secret"), &activity())
            .await
            .unwrap();

        assert_eq!(uploaded.id, "i777");
        assert_eq!(uploaded.url, format!("{site}/activities/i777"));
        let request = served.await.unwrap().remove(0);
        let text = String::from_utf8_lossy(&request);
        assert!(
            text.starts_with(
                "POST /api/v1/athlete/0/activities?name=Gurten%20%26%20back\
                 &external_id=torqa-20261012-071500 "
            ),
            "{text}"
        );
        assert!(text.contains(AS_SECRET), "{text}");
        assert!(text.contains("filename=\"torqa-20261012-071500.fit\""));
        assert!(request.windows(8).any(|w| w == activity().fit.as_slice()));
    }

    #[tokio::test]
    async fn a_ride_intervals_icu_has_already_counts_as_uploaded() {
        let (site, _served) = serve(vec![(
            "200 OK",
            r#"{"icu_athlete_id":"i42","activities":[{"icu_athlete_id":"i42","id":"i500"}]}"#
                .to_owned(),
        )])
        .await;

        let uploaded = IntervalsIcu::with_site(&site)
            .upload(&mut with_key("secret"), &activity())
            .await
            .unwrap();

        assert_eq!(uploaded.id, "i500");
    }

    #[tokio::test]
    async fn a_revoked_key_asks_to_connect_again() {
        let (site, _served) = serve(vec![("403 Forbidden", "{}".to_owned())]).await;

        let error = IntervalsIcu::with_site(&site)
            .upload(&mut with_key("secret"), &activity())
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Unauthorized(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_refused_upload_says_why() {
        let (site, _served) = serve(vec![(
            "422 Unprocessable Entity",
            r#"{"error":"Unable to parse file"}"#.to_owned(),
        )])
        .await;

        let error = IntervalsIcu::with_site(&site)
            .upload(&mut with_key("secret"), &activity())
            .await
            .unwrap_err();

        assert!(
            matches!(&error, UploadError::Rejected(why) if why.contains("Unable to parse file")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn an_unreachable_platform_is_told_apart() {
        // Nothing listens on the discard port.
        let error = IntervalsIcu::with_site("http://127.0.0.1:9")
            .upload(&mut with_key("secret"), &activity())
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Unreachable(_)), "{error:?}");
    }
}
