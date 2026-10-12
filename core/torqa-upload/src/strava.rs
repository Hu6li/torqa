//! Strava (R29). Strava lets an app in only with OAuth, and every token request needs the
//! app's client secret, which open-source Torqa cannot keep. So each rider connects through a
//! Strava API application of their own: its Client ID and Client Secret, then a login in the
//! browser (ADR 0013). Rides go up as FIT files on a trainer; Strava processes them and then
//! names the activity.

use std::time::Duration;

use reqwest::StatusCode;

use crate::oauth::{self, Tokens};
use crate::{
    Account, Activity, ActivityUploader, BoxFuture, Browser, Credentials, Field, UploadError,
    Uploaded, cannot_reach, client, describe, multipart, query,
};

const NAME: &str = "Strava";
const SITE: &str = "https://www.strava.com";
/// Strava's API is to move to its own host in 2027 (`api-v3.strava.com`); the current one
/// keeps working until then.
const API: &str = "/api/v3";
const CLIENT_ID: &str = "client_id";
const CLIENT_SECRET: &str = "client_secret";
const ACCESS_TOKEN: &str = "access_token";
const REFRESH_TOKEN: &str = "refresh_token";
const EXPIRES_AT: &str = "expires_at";
/// Uploading activities is all Torqa asks for.
const SCOPE: &str = "activity:write";
/// How often to ask whether Strava has processed an upload, and how many times.
const POLL: Duration = Duration::from_secs(2);
const POLLS: u32 = 30;

// Labels and help are translated by the front end.
// i18n-begin
const FIELDS: &[Field] = &[
    Field {
        key: CLIENT_ID,
        label: "Client ID",
        secret: false,
    },
    Field {
        key: CLIENT_SECRET,
        label: "Client Secret",
        secret: true,
    },
];
// One line: the translation template takes texts line by line.
const HELP: &str = "Needs a Strava subscription. Create an API application on Strava with Authorization Callback Domain localhost, then copy its Client ID and Client Secret here.";
/// Strava switched the rider's application off: logging in still works, uploads do not.
const INACTIVE: &str = "Strava has switched off your API application: the account that owns it needs a Strava subscription. Subscribe, reactivate the application in Strava's API settings, then upload again.";
// i18n-end

/// Uploads to Strava.
#[derive(Debug, Clone)]
pub struct Strava {
    site: String,
    poll: Duration,
    login_time: Duration,
}

impl Default for Strava {
    fn default() -> Self {
        Self::new()
    }
}

impl Strava {
    /// Strava itself.
    #[must_use]
    pub fn new() -> Self {
        Self::with_site(SITE)
    }

    /// Strava at another address, e.g. a stand-in for tests.
    #[must_use]
    pub fn with_site(site: &str) -> Self {
        Self {
            site: site.trim_end_matches('/').to_owned(),
            poll: POLL,
            login_time: oauth::LOGIN_TIME,
        }
    }

    fn token_url(&self) -> String {
        format!("{}/oauth/token", self.site)
    }

    /// The application's id and secret in `credentials`.
    fn application(credentials: &Credentials) -> Result<(String, String), UploadError> {
        let get = |key: &str| {
            credentials
                .get(key)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        get(CLIENT_ID).zip(get(CLIENT_SECRET)).ok_or_else(|| {
            UploadError::Unauthorized(
                "Enter your Strava application's Client ID and Client Secret.".to_owned(),
            )
        })
    }

    /// Trades the login's `grant` for the account.
    async fn account(
        &self,
        id: &str,
        secret: &str,
        grant: oauth::Grant,
    ) -> Result<Account, UploadError> {
        if !grant.scope.split([',', ' ']).any(|scope| scope == SCOPE) {
            return Err(UploadError::Rejected(
                "Torqa may not upload activities: connect again and leave that box ticked."
                    .to_owned(),
            ));
        }
        let (tokens, answer) = oauth::request_tokens(
            NAME,
            &self.token_url(),
            &[
                ("client_id", id),
                ("client_secret", secret),
                ("code", &grant.code),
                ("grant_type", "authorization_code"),
            ],
            "",
        )
        .await?;
        let athlete = answer.get("athlete");
        let text = |key: &str| {
            athlete
                .and_then(|a| a.get(key))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
        };
        let full_name = [text("firstname"), text("lastname")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        let label = if full_name.is_empty() {
            text("username").unwrap_or(NAME).to_owned()
        } else {
            full_name
        };
        let mut credentials = Credentials::from([
            (CLIENT_ID.to_owned(), id.to_owned()),
            (CLIENT_SECRET.to_owned(), secret.to_owned()),
        ]);
        store(&mut credentials, &tokens);
        Ok(Account { label, credentials })
    }

    /// The access token in `credentials`, renewed first if it expires soon.
    async fn access_token(&self, credentials: &mut Credentials) -> Result<String, UploadError> {
        let (id, secret) = Self::application(credentials)?;
        let get = |key: &str| credentials.get(key).cloned().unwrap_or_default();
        let tokens = Tokens {
            access: get(ACCESS_TOKEN),
            refresh: get(REFRESH_TOKEN),
            expires_at: get(EXPIRES_AT).parse().unwrap_or(0),
        };
        if !tokens.expiring() {
            return Ok(tokens.access);
        }
        let (renewed, _) = oauth::request_tokens(
            NAME,
            &self.token_url(),
            &[
                ("client_id", &id),
                ("client_secret", &secret),
                ("grant_type", "refresh_token"),
                ("refresh_token", &tokens.refresh),
            ],
            &tokens.refresh,
        )
        .await?;
        // Strava takes only the newest refresh token from now on.
        store(credentials, &renewed);
        Ok(renewed.access)
    }

    async fn send(&self, token: &str, activity: &Activity) -> Result<Uploaded, UploadError> {
        let (content_type, body) = multipart(
            &[
                ("data_type", "fit"),
                ("name", &activity.name),
                ("external_id", &activity.external_id),
                // Every Torqa ride is on a trainer.
                ("trainer", "1"),
            ],
            (
                "file",
                &format!("{}.fit", activity.external_id),
                &activity.fit,
            ),
        );
        let answer = client()?
            .post(format!("{}{API}/uploads", self.site))
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await
            .map_err(|e| cannot_reach(NAME, &e))?;
        let mut upload = answered(answer).await?;
        for _ in 0..POLLS {
            if let Some(uploaded) = self.processed(&upload)? {
                return Ok(uploaded);
            }
            tokio::time::sleep(self.poll).await;
            let id = upload
                .get("id_str")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    upload
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .map(|id| id.to_string())
                })
                .ok_or_else(|| {
                    UploadError::Rejected(format!("{NAME} took the ride but named no upload."))
                })?;
            let answer = client()?
                .get(format!("{}{API}/uploads/{id}", self.site))
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| cannot_reach(NAME, &e))?;
            upload = answered(answer).await?;
        }
        Err(UploadError::Rejected(format!(
            "{NAME} is still processing the ride: upload it again in a minute to link it."
        )))
    }

    /// The activity a processed upload became, `None` while Strava is still at it.
    fn processed(&self, upload: &serde_json::Value) -> Result<Option<Uploaded>, UploadError> {
        let activity = upload
            .get("activity_id")
            .and_then(serde_json::Value::as_u64)
            .map(|id| id.to_string());
        let error = upload
            .get("error")
            .and_then(serde_json::Value::as_str)
            .filter(|e| !e.is_empty());
        let id = match (activity, error) {
            (Some(id), _) => id,
            // A ride Strava has already, e.g. uploaded by hand: link that one.
            (None, Some(error)) => duplicate_of(error).ok_or_else(|| {
                UploadError::Rejected(format!("{NAME} could not take the ride: {error}"))
            })?,
            (None, None) => return Ok(None),
        };
        Ok(Some(Uploaded {
            url: format!("{}/activities/{id}", self.site),
            id,
        }))
    }
}

/// Keeps `tokens` in `credentials`.
fn store(credentials: &mut Credentials, tokens: &Tokens) {
    credentials.insert(ACCESS_TOKEN.to_owned(), tokens.access.clone());
    credentials.insert(REFRESH_TOKEN.to_owned(), tokens.refresh.clone());
    credentials.insert(EXPIRES_AT.to_owned(), tokens.expires_at.to_string());
}

/// The activity a duplicate upload's error names, e.g. "… duplicate of activity 123".
fn duplicate_of(error: &str) -> Option<String> {
    let after = &error[error.find("duplicate of")? + "duplicate of".len()..];
    let digits: String = after
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then_some(digits)
}

/// Whether Strava's answer says the application is switched off, as it does for applications
/// whose owner has no Strava subscription:
/// `{"errors":[{"resource":"Application","field":"Status","code":"Inactive"}]}`.
fn inactive(answer: &str) -> bool {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(answer) else {
        return false;
    };
    json.get("errors")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .any(|error| {
            let part = |key: &str| error.get(key).and_then(serde_json::Value::as_str);
            part("resource") == Some("Application")
                && part("field") == Some("Status")
                && part("code") == Some("Inactive")
        })
}

/// The JSON of a successful answer, or what went wrong.
async fn answered(answer: reqwest::Response) -> Result<serde_json::Value, UploadError> {
    let status = answer.status();
    let text = answer.text().await.map_err(|e| cannot_reach(NAME, &e))?;
    match status {
        StatusCode::UNAUTHORIZED => Err(UploadError::Unauthorized(format!(
            "{NAME} no longer lets Torqa in: {}.",
            describe(&text)
        ))),
        // Connecting again would not help: the login works, the application is off.
        StatusCode::FORBIDDEN if inactive(&text) => Err(UploadError::Rejected(INACTIVE.to_owned())),
        StatusCode::TOO_MANY_REQUESTS => Err(UploadError::Rejected(format!(
            "{NAME} takes no more uploads for a while: try again later."
        ))),
        status if status.is_success() => serde_json::from_str(&text).map_err(|e| {
            UploadError::Rejected(format!("{NAME} answered something unexpected ({e})."))
        }),
        status => Err(UploadError::Rejected(format!(
            "{NAME} refused: {} ({status}).",
            describe(&text)
        ))),
    }
}

impl ActivityUploader for Strava {
    fn id(&self) -> &'static str {
        "strava"
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
        "https://www.strava.com/settings/api"
    }

    fn connect(
        &self,
        entered: Credentials,
        browser: Browser,
    ) -> BoxFuture<'_, Result<Account, UploadError>> {
        Box::pin(async move {
            let (id, secret) = Self::application(&entered)?;
            let authorize = format!(
                "{}/oauth/authorize?{}",
                self.site,
                query(&[
                    ("client_id", &id),
                    ("response_type", "code"),
                    ("approval_prompt", "auto"),
                    ("scope", SCOPE),
                ])
            );
            oauth::log_in(&authorize, &browser, self.login_time, |grant| {
                self.account(&id, &secret, grant)
            })
            .await
        })
    }

    fn upload<'a>(
        &'a self,
        credentials: &'a mut Credentials,
        activity: &'a Activity,
    ) -> BoxFuture<'a, Result<Uploaded, UploadError>> {
        Box::pin(async move {
            let token = self.access_token(credentials).await?;
            self.send(&token, activity).await
        })
    }

    fn disconnect<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let (Ok((id, secret)), Some(token)) = (
                Self::application(credentials),
                credentials.get(REFRESH_TOKEN),
            ) else {
                return;
            };
            oauth::revoke(
                NAME,
                &format!("{}/oauth/revoke", self.site),
                &id,
                &secret,
                token,
            )
            .await;
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tokio::sync::oneshot;

    use super::*;
    use crate::decode;
    use crate::testing::serve;

    const CONNECTED: &str = "Torqa is connected to Strava.";
    const FAILED: &str = "Torqa could not connect.";

    /// Strava at `site`, quick to poll and to give up on a login.
    fn strava(site: &str) -> Strava {
        Strava {
            poll: Duration::ZERO,
            login_time: Duration::from_secs(5),
            ..Strava::with_site(site)
        }
    }

    fn application() -> Credentials {
        Credentials::from([
            (CLIENT_ID.to_owned(), "123".to_owned()),
            (CLIENT_SECRET.to_owned(), " s3cret ".to_owned()),
        ])
    }

    /// The account after a login, its access token expiring in `seconds`.
    fn connected(seconds: u64) -> Credentials {
        let mut credentials = application();
        credentials.insert(CLIENT_SECRET.to_owned(), "s3cret".to_owned());
        store(
            &mut credentials,
            &Tokens {
                access: "a1".to_owned(),
                refresh: "r1".to_owned(),
                expires_at: oauth::now() + seconds,
            },
        );
        credentials
    }

    /// A browser that, shown Strava's login, comes straight back with `answer` (a query,
    /// `STATE` standing for the state Torqa sent; `None` never comes back). Hands back the
    /// login's address and the page the browser then showed.
    fn browser(answer: Option<&'static str>) -> (Browser, oneshot::Receiver<(String, String)>) {
        let (tx, rx) = oneshot::channel();
        let tx = Mutex::new(Some(tx));
        let open = move |url: &str| {
            let url = url.to_owned();
            let parameter = |name: &str| {
                url.split(['?', '&'])
                    .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
                    .map(decode)
                    .unwrap()
            };
            let (redirect, state) = (parameter("redirect_uri"), parameter("state"));
            let tx = tx.lock().unwrap().take().unwrap();
            let Some(answer) = answer else {
                drop(tx);
                return;
            };
            tokio::spawn(async move {
                let back = format!("{redirect}?{}", answer.replace("STATE", &state));
                let page = reqwest::get(back).await.unwrap().text().await.unwrap();
                let _ = tx.send((url, page));
            });
        };
        let browser = Browser {
            open: Box::new(open),
            connected_page: CONNECTED.to_owned(),
            failed_page: FAILED.to_owned(),
        };
        (browser, rx)
    }

    fn text(request: &[u8]) -> String {
        String::from_utf8_lossy(request).into_owned()
    }

    #[tokio::test]
    async fn connecting_logs_in_through_the_riders_own_application() {
        let (site, served) = serve(vec![(
            "200 OK",
            r#"{"token_type":"Bearer","access_token":"a1","refresh_token":"r1",
                "expires_at":2000000000,"athlete":{"id":7,"firstname":"Ann","lastname":"Rider"}}"#
                .to_owned(),
        )])
        .await;
        let (browser, shown) = browser(Some("code=c0de&scope=read,activity:write&state=STATE"));

        let account = strava(&site).connect(application(), browser).await.unwrap();

        assert_eq!(account.label, "Ann Rider");
        assert_eq!(account.credentials["client_id"], "123");
        assert_eq!(account.credentials["client_secret"], "s3cret");
        assert_eq!(account.credentials["access_token"], "a1");
        assert_eq!(account.credentials["refresh_token"], "r1");
        assert_eq!(account.credentials["expires_at"], "2000000000");
        let (login, page) = shown.await.unwrap();
        assert!(login.starts_with(&format!("{site}/oauth/authorize?client_id=123&")));
        for part in [
            "response_type=code",
            "scope=activity%3Awrite",
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A",
            "&state=",
        ] {
            assert!(login.contains(part), "{part} in {login}");
        }
        assert!(page.contains(CONNECTED), "{page}");
        let exchange = text(&served.await.unwrap()[0]);
        assert!(exchange.starts_with("POST /oauth/token "));
        assert!(
            exchange.ends_with(
                "client_id=123&client_secret=s3cret&code=c0de&grant_type=authorization_code"
            ),
            "{exchange}"
        );
    }

    #[tokio::test]
    async fn a_login_that_may_not_upload_is_refused() {
        let (browser, shown) = browser(Some("code=c0de&scope=read&state=STATE"));

        let error = strava("http://127.0.0.1:9")
            .connect(application(), browser)
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Rejected(_)), "{error:?}");
        let (_, page) = shown.await.unwrap();
        assert!(page.contains(FAILED) && page.contains("upload"), "{page}");
    }

    #[tokio::test]
    async fn a_declined_login_is_refused() {
        let (browser, _shown) = browser(Some("error=access_denied&state=STATE"));

        let error = strava("http://127.0.0.1:9")
            .connect(application(), browser)
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Rejected(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_login_coming_back_for_another_request_is_refused() {
        let (browser, _shown) = browser(Some("code=c0de&scope=activity:write&state=other"));

        let error = strava("http://127.0.0.1:9")
            .connect(application(), browser)
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Rejected(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_login_not_finished_in_time_gives_up() {
        let (browser, _shown) = browser(None);
        let quick = Strava {
            login_time: Duration::from_millis(100),
            ..strava("http://127.0.0.1:9")
        };

        let error = quick.connect(application(), browser).await.unwrap_err();

        assert!(matches!(error, UploadError::Rejected(_)), "{error:?}");
    }

    #[tokio::test]
    async fn connecting_needs_the_applications_id_and_secret() {
        let (browser, _shown) = browser(None);
        let only_id = Credentials::from([(CLIENT_ID.to_owned(), "123".to_owned())]);

        let error = strava("http://127.0.0.1:9")
            .connect(only_id, browser)
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Unauthorized(_)), "{error:?}");
    }

    fn activity() -> Activity {
        Activity {
            fit: b"FIT activity".to_vec(),
            name: "Gurten & back".to_owned(),
            external_id: "torqa-20261012-071500".to_owned(),
        }
    }

    #[tokio::test]
    async fn uploads_the_fit_file_as_a_trainer_ride_and_links_the_activity() {
        let (site, served) = serve(vec![
            (
                "201 Created",
                r#"{"id":5,"id_str":"5","external_id":"torqa-20261012-071500.fit",
                    "error":null,"status":"Your activity is still being processed.",
                    "activity_id":null}"#
                    .to_owned(),
            ),
            (
                "200 OK",
                r#"{"id":5,"id_str":"5","error":null,"status":"Your activity is ready.",
                    "activity_id":777}"#
                    .to_owned(),
            ),
        ])
        .await;
        let mut credentials = connected(3600);

        let uploaded = strava(&site)
            .upload(&mut credentials, &activity())
            .await
            .unwrap();

        assert_eq!(uploaded.id, "777");
        assert_eq!(uploaded.url, format!("{site}/activities/777"));
        let requests = served.await.unwrap();
        let upload = text(&requests[0]);
        assert!(upload.starts_with("POST /api/v3/uploads "), "{upload}");
        assert!(upload.contains("authorization: Bearer a1"), "{upload}");
        for (field, value) in [
            ("data_type", "fit"),
            ("name", "Gurten & back"),
            ("external_id", "torqa-20261012-071500"),
            ("trainer", "1"),
        ] {
            assert!(
                upload.contains(&format!("name=\"{field}\"\r\n\r\n{value}\r\n")),
                "{field} in {upload}"
            );
        }
        assert!(upload.contains("FIT activity"));
        assert!(text(&requests[1]).starts_with("GET /api/v3/uploads/5 "));
    }

    #[tokio::test]
    async fn an_expiring_token_is_renewed_first_and_the_new_one_kept() {
        let (site, served) = serve(vec![
            (
                "200 OK",
                r#"{"token_type":"Bearer","access_token":"a2","refresh_token":"r2",
                    "expires_at":2000000000}"#
                    .to_owned(),
            ),
            (
                "201 Created",
                r#"{"id":5,"id_str":"5","error":null,"activity_id":777}"#.to_owned(),
            ),
        ])
        .await;
        let mut credentials = connected(60);

        strava(&site)
            .upload(&mut credentials, &activity())
            .await
            .unwrap();

        assert_eq!(credentials["access_token"], "a2");
        assert_eq!(credentials["refresh_token"], "r2");
        assert_eq!(credentials["expires_at"], "2000000000");
        let requests = served.await.unwrap();
        let renewal = text(&requests[0]);
        assert!(renewal.starts_with("POST /oauth/token "));
        assert!(
            renewal.ends_with(
                "client_id=123&client_secret=s3cret&grant_type=refresh_token&refresh_token=r1"
            ),
            "{renewal}"
        );
        assert!(text(&requests[1]).contains("authorization: Bearer a2"));
    }

    #[tokio::test]
    async fn a_ride_strava_has_already_counts_as_uploaded() {
        let (site, _served) = serve(vec![(
            "201 Created",
            r#"{"id":5,"id_str":"5","activity_id":null,
                "error":"torqa-20261012-071500.fit duplicate of activity 4242"}"#
                .to_owned(),
        )])
        .await;

        let uploaded = strava(&site)
            .upload(&mut connected(3600), &activity())
            .await
            .unwrap();

        assert_eq!(uploaded.id, "4242");
    }

    #[tokio::test]
    async fn a_file_strava_cannot_read_says_why() {
        let (site, _served) = serve(vec![(
            "201 Created",
            r#"{"id":5,"id_str":"5","activity_id":null,"error":"Unable to parse file"}"#.to_owned(),
        )])
        .await;

        let error = strava(&site)
            .upload(&mut connected(3600), &activity())
            .await
            .unwrap_err();

        assert!(
            matches!(&error, UploadError::Rejected(why) if why.contains("Unable to parse file")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_withdrawn_grant_asks_to_connect_again() {
        let (site, _served) = serve(vec![(
            "400 Bad Request",
            r#"{"message":"Bad Request","errors":[{"resource":"RefreshToken",
                "field":"refresh_token","code":"invalid"}]}"#
                .to_owned(),
        )])
        .await;

        let error = strava(&site)
            .upload(&mut connected(60), &activity())
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Unauthorized(_)), "{error:?}");
    }

    #[tokio::test]
    async fn an_upload_refused_for_its_token_asks_to_connect_again() {
        let (site, _served) = serve(vec![(
            "401 Unauthorized",
            r#"{"message":"Authorization Error"}"#.to_owned(),
        )])
        .await;

        let error = strava(&site)
            .upload(&mut connected(3600), &activity())
            .await
            .unwrap_err();

        assert!(matches!(error, UploadError::Unauthorized(_)), "{error:?}");
    }

    #[tokio::test]
    async fn an_application_switched_off_for_want_of_a_subscription_says_so() {
        let (site, _served) = serve(vec![(
            "403 Forbidden",
            r#"{"message":"Forbidden","errors":[{"resource":"Application","field":"Status",
                "code":"Inactive"}]}"#
                .to_owned(),
        )])
        .await;

        let error = strava(&site)
            .upload(&mut connected(3600), &activity())
            .await
            .unwrap_err();

        assert!(
            matches!(&error, UploadError::Rejected(why) if why.contains("subscription")),
            "not a reason to connect again, but to subscribe: {error:?}"
        );
    }

    #[tokio::test]
    async fn other_refusals_say_why_without_asking_to_connect_again() {
        let (site, _served) = serve(vec![(
            "403 Forbidden",
            r#"{"message":"Forbidden","errors":[{"resource":"Athlete","field":"access",
                "code":"forbidden"}]}"#
                .to_owned(),
        )])
        .await;

        let error = strava(&site)
            .upload(&mut connected(3600), &activity())
            .await
            .unwrap_err();

        assert!(
            matches!(&error, UploadError::Rejected(why) if why.contains("Forbidden")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn disconnecting_revokes_the_grant() {
        let (site, served) = serve(vec![("200 OK", String::new())]).await;

        strava(&site).disconnect(&connected(3600)).await;

        let revoke = text(&served.await.unwrap()[0]);
        assert!(revoke.starts_with("POST /oauth/revoke "));
        // "123:s3cret" in Base64.
        assert!(
            revoke.contains("authorization: Basic MTIzOnMzY3JldA=="),
            "{revoke}"
        );
        assert!(revoke.ends_with("token=r1&token_type_hint=refresh_token"));
    }

    #[test]
    fn duplicates_name_the_activity_strava_has() {
        assert_eq!(
            duplicate_of("a.fit duplicate of activity 4242").as_deref(),
            Some("4242")
        );
        assert_eq!(
            duplicate_of("a.fit duplicate of <a href='/activities/99'>activity</a>").as_deref(),
            Some("99")
        );
        assert_eq!(duplicate_of("Unable to parse file"), None);
    }
}
