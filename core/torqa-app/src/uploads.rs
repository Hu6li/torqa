//! Uploading rides to training platforms (R29): a rider's accounts, connected with what the
//! platform asks for, and rides uploaded in the background, each remembering where it went.
//! Rides the rider keeps go up by themselves to the accounts set to upload automatically
//! (R60).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use torqa_storage::accounts::{self, StoredAccount};
use torqa_storage::rides;
pub use torqa_upload::{
    Activity, ActivityUploader, Browser, Credentials, Field, UploadError, Uploaded,
};

use crate::{App, AppError, AppEvent, JobResult};

/// Every platform rides can be uploaded to, in the order they are offered.
#[must_use]
pub fn platforms() -> Vec<Arc<dyn ActivityUploader>> {
    torqa_upload::platforms()
}

/// A platform and the rider's account on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStatus {
    /// The platform's id.
    pub platform: &'static str,
    /// Its name.
    pub name: &'static str,
    /// What the rider types in to connect.
    pub fields: &'static [Field],
    /// How to come by it, in English.
    pub help: &'static str,
    /// The page the help refers to.
    pub help_url: &'static str,
    /// Who is connected, if anyone.
    pub label: Option<String>,
    /// Rides the rider keeps go up without asking (R60).
    pub auto_upload: bool,
}

/// What the page the browser comes back to after logging in to a platform says, in the
/// rider's language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginPages {
    /// Once the account is connected.
    pub connected: String,
    /// When the login failed or was declined.
    pub failed: String,
}

/// The platforms and the uploads under way.
pub(crate) struct UploadState {
    platforms: Vec<Arc<dyn ActivityUploader>>,
    /// One upload at a time. Each reads the account's credentials when its turn comes and
    /// stores renewed ones before the next starts: platforms that renew tokens (Strava) take
    /// only the newest, so two renewals of one account must not overlap.
    queue: Arc<tokio::sync::Mutex<()>>,
    /// Held while a rider's accounts file is read, changed and written, by the frame loop and
    /// uploads alike.
    accounts: Arc<Mutex<()>>,
    /// Rides on their way to a platform, so none goes up twice.
    in_flight: HashSet<(PathBuf, &'static str)>,
    /// The account being connected, to give up on when the rider cancels.
    connecting: Option<tokio::task::JoinHandle<()>>,
}

impl UploadState {
    pub(crate) fn new(platforms: Vec<Arc<dyn ActivityUploader>>) -> Self {
        Self {
            platforms,
            queue: Arc::default(),
            accounts: Arc::default(),
            in_flight: HashSet::new(),
            connecting: None,
        }
    }
}

/// Upload work done in the background, for the frame loop to keep.
pub(crate) enum Done {
    Connected {
        rider: String,
        platform: &'static str,
        result: Result<torqa_upload::Account, UploadError>,
    },
    Uploaded {
        platform: &'static str,
        fit: PathBuf,
        result: Result<Uploaded, UploadError>,
    },
    /// A platform's login, to show in the rider's browser.
    OpenUrl(String),
}

impl App {
    /// Takes `platforms` as those rides can be uploaded to instead of [`platforms`], e.g.
    /// stand-ins in tests.
    pub fn set_upload_platforms(&mut self, platforms: Vec<Arc<dyn ActivityUploader>>) {
        self.uploads.platforms = platforms;
    }

    /// Every platform with rider `rider`'s account on it, in the order they are offered.
    #[must_use]
    pub fn accounts(&self, rider: &str) -> Vec<AccountStatus> {
        let stored = accounts::load(&self.data_dir, rider).unwrap_or_else(|error| {
            tracing::warn!(%error, "cannot read the rider's accounts");
            accounts::Accounts::new()
        });
        self.uploads
            .platforms
            .iter()
            .map(|uploader| {
                let account = stored.get(uploader.id());
                AccountStatus {
                    platform: uploader.id(),
                    name: uploader.name(),
                    fields: uploader.fields(),
                    help: uploader.help(),
                    help_url: uploader.help_url(),
                    label: account.map(|a| a.label.clone()),
                    auto_upload: account.is_some_and(|a| a.auto_upload),
                }
            })
            .collect()
    }

    /// Connects rider `rider`'s account on `platform` with the fields they `entered`, in the
    /// background: reports [`AppEvent::AccountConnected`] or [`AppEvent::AccountFailed`].
    /// Platforms the rider logs in to report [`AppEvent::OpenUrl`] with the login first, and
    /// their page afterwards says `pages`. Connecting another account gives up on this one.
    ///
    /// # Errors
    /// [`AppError::UnknownPlatform`] if there is no such platform.
    pub fn connect_account(
        &mut self,
        rider: &str,
        platform: &str,
        entered: Credentials,
        pages: LoginPages,
    ) -> Result<(), AppError> {
        let uploader = self.uploader(platform)?;
        self.cancel_connect();
        let tx = self.jobs_tx.clone();
        let opener = self.jobs_tx.clone();
        let browser = Browser {
            open: Box::new(move |url| {
                let _ = opener.send(JobResult::Upload(Box::new(Done::OpenUrl(url.to_owned()))));
            }),
            connected_page: pages.connected,
            failed_page: pages.failed,
        };
        let rider = rider.to_owned();
        self.uploads.connecting = Some(self.runtime.spawn(async move {
            let result = uploader.connect(entered, browser).await;
            let _ = tx.send(JobResult::Upload(Box::new(Done::Connected {
                rider,
                platform: uploader.id(),
                result,
            })));
        }));
        Ok(())
    }

    /// Gives up on the account being connected, e.g. a login the rider no longer wants to
    /// finish; nothing is reported for it.
    pub fn cancel_connect(&mut self) {
        if let Some(connecting) = self.uploads.connecting.take() {
            connecting.abort();
        }
    }

    /// Forgets rider `rider`'s account on `platform` and, in the background, withdraws
    /// Torqa's access where the platform keeps a grant for it.
    ///
    /// # Errors
    /// [`AppError::Storage`] if the accounts cannot be read or written.
    pub fn disconnect_account(&mut self, rider: &str, platform: &str) -> Result<(), AppError> {
        let mut removed = None;
        self.change_accounts(rider, |accounts| {
            removed = accounts.remove(platform);
        })?;
        if let (Some(account), Ok(uploader)) = (removed, self.uploader(platform)) {
            self.revoke(uploader, account.credentials);
        }
        Ok(())
    }

    /// Withdraws Torqa's access to all of rider `rider`'s accounts in the background, as the
    /// rider is deleted.
    pub(crate) fn revoke_accounts(&mut self, rider: &str) {
        let stored = accounts::load(&self.data_dir, rider).unwrap_or_default();
        for (platform, account) in stored {
            if let Ok(uploader) = self.uploader(&platform) {
                self.revoke(uploader, account.credentials);
            }
        }
    }

    fn revoke(&self, uploader: Arc<dyn ActivityUploader>, credentials: Credentials) {
        self.runtime.spawn(async move {
            uploader.disconnect(&credentials).await;
        });
    }

    /// Whether rides rider `rider` keeps go up to `platform` without asking (R60).
    ///
    /// # Errors
    /// [`AppError::NotConnected`] if the rider has no account there, [`AppError::Storage`] if
    /// the accounts cannot be read or written.
    pub fn set_auto_upload(
        &mut self,
        rider: &str,
        platform: &str,
        on: bool,
    ) -> Result<(), AppError> {
        let uploader = self.uploader(platform)?;
        let mut connected = false;
        self.change_accounts(rider, |accounts| {
            if let Some(account) = accounts.get_mut(uploader.id()) {
                account.auto_upload = on;
                connected = true;
            }
        })?;
        if connected {
            Ok(())
        } else {
            Err(AppError::NotConnected(uploader.name()))
        }
    }

    /// Uploads the active rider's ride `fit` to `platform` as `title` (R50), in the
    /// background: reports [`AppEvent::UploadFinished`] or [`AppEvent::UploadFailed`]. An
    /// empty title falls back to the ride's name or route. A ride already on its way there is
    /// not sent again.
    ///
    /// # Errors
    /// [`AppError::UnknownPlatform`] if there is no such platform, [`AppError::NotConnected`]
    /// if the rider has no account there, [`AppError::Storage`] if the ride cannot be read.
    pub fn upload_ride(&mut self, fit: &Path, platform: &str, title: &str) -> Result<(), AppError> {
        let uploader = self.uploader(platform)?;
        let connected = accounts::load(&self.data_dir, &self.profile.id)
            .map_err(|e| AppError::Storage(format!("cannot read the accounts: {e}")))?
            .contains_key(uploader.id());
        if !connected {
            return Err(AppError::NotConnected(uploader.name()));
        }
        if self
            .uploads
            .in_flight
            .contains(&(fit.to_owned(), uploader.id()))
        {
            return Ok(());
        }
        let activity = activity(fit, title)?;
        self.start_upload(uploader, fit, activity);
        Ok(())
    }

    /// The rider keeps the ride `fit` (Done on its summary, R42): it goes up as `title` to
    /// every account of theirs set to upload automatically that does not have it yet (R60).
    /// Returns those platforms.
    pub fn keep_ride(&mut self, fit: &Path, title: &str) -> Vec<&'static str> {
        let stored = accounts::load(&self.data_dir, &self.profile.id).unwrap_or_else(|error| {
            tracing::warn!(%error, "cannot read the rider's accounts");
            accounts::Accounts::new()
        });
        let uploaded = rides::load(fit).map(|r| r.uploads).unwrap_or_default();
        let due: Vec<Arc<dyn ActivityUploader>> = self
            .uploads
            .platforms
            .iter()
            .filter(|u| stored.get(u.id()).is_some_and(|a| a.auto_upload))
            .filter(|u| !uploaded.contains_key(u.id()))
            .filter(|u| !self.uploads.in_flight.contains(&(fit.to_owned(), u.id())))
            .cloned()
            .collect();
        if due.is_empty() {
            return Vec::new();
        }
        let activity = match activity(fit, title) {
            Ok(activity) => activity,
            Err(error) => {
                tracing::warn!(%error, "cannot upload the ride");
                return Vec::new();
            }
        };
        due.into_iter()
            .map(|uploader| {
                let platform = uploader.id();
                self.start_upload(uploader, fit, activity.clone());
                platform
            })
            .collect()
    }

    /// The platforms the ride `fit` is on its way to.
    #[must_use]
    pub fn uploading(&self, fit: &Path) -> Vec<&'static str> {
        self.uploads
            .platforms
            .iter()
            .map(|u| u.id())
            .filter(|&id| self.uploads.in_flight.contains(&(fit.to_owned(), id)))
            .collect()
    }

    /// Queues the upload of `activity`, the ride `fit`, to the active rider's account on
    /// `uploader`'s platform.
    fn start_upload(
        &mut self,
        uploader: Arc<dyn ActivityUploader>,
        fit: &Path,
        activity: Activity,
    ) {
        let platform = uploader.id();
        self.uploads.in_flight.insert((fit.to_owned(), platform));
        let tx = self.jobs_tx.clone();
        let queue = Arc::clone(&self.uploads.queue);
        let lock = Arc::clone(&self.uploads.accounts);
        let data_dir = self.data_dir.clone();
        let rider = self.profile.id.clone();
        let fit = fit.to_owned();
        self.runtime.spawn(async move {
            let _turn = queue.lock().await;
            let result = upload_in_turn(&data_dir, &lock, &rider, &*uploader, &activity).await;
            let _ = tx.send(JobResult::Upload(Box::new(Done::Uploaded {
                platform,
                fit,
                result,
            })));
        });
    }

    fn uploader(&self, platform: &str) -> Result<Arc<dyn ActivityUploader>, AppError> {
        self.uploads
            .platforms
            .iter()
            .find(|u| u.id() == platform)
            .cloned()
            .ok_or(AppError::UnknownPlatform)
    }

    fn change_accounts(
        &self,
        rider: &str,
        change: impl FnOnce(&mut accounts::Accounts),
    ) -> Result<(), AppError> {
        change_accounts(&self.data_dir, &self.uploads.accounts, rider, change)
    }

    /// Keeps what upload work in the background did, and tells what came of it.
    pub(crate) fn upload_work_done(&mut self, done: Done) -> AppEvent {
        match done {
            Done::Connected {
                rider,
                platform,
                result,
            } => self.account_connected(rider, platform, result),
            Done::Uploaded {
                platform,
                fit,
                result,
            } => {
                self.uploads.in_flight.remove(&(fit.clone(), platform));
                self.upload_done(platform, fit, result)
            }
            Done::OpenUrl(url) => AppEvent::OpenUrl(url),
        }
    }

    /// Keeps a connected account; one connected again keeps its setting for automatic uploads.
    fn account_connected(
        &self,
        rider: String,
        platform: &'static str,
        result: Result<torqa_upload::Account, UploadError>,
    ) -> AppEvent {
        let failed = |message: String| AppEvent::AccountFailed {
            rider: rider.clone(),
            platform: platform.to_owned(),
            message,
        };
        let account = match result {
            Ok(account) => account,
            Err(error) => return failed(error.to_string()),
        };
        let label = account.label.clone();
        match self.change_accounts(&rider, |accounts| {
            let auto_upload = accounts.get(platform).is_some_and(|a| a.auto_upload);
            accounts.insert(
                platform.to_owned(),
                StoredAccount {
                    label: account.label,
                    auto_upload,
                    credentials: account.credentials,
                },
            );
        }) {
            Ok(()) => AppEvent::AccountConnected {
                rider,
                platform: platform.to_owned(),
                label,
            },
            Err(error) => failed(error.to_string()),
        }
    }

    /// Remembers where the ride went.
    fn upload_done(
        &self,
        platform: &'static str,
        fit: PathBuf,
        result: Result<Uploaded, UploadError>,
    ) -> AppEvent {
        match result {
            Ok(uploaded) => {
                let url = uploaded.url.clone();
                let record = rides::load(&fit).or_else(|_| self.rebuild_metadata(&fit));
                match record {
                    Ok(mut record) => {
                        record.uploads.insert(
                            platform.to_owned(),
                            rides::Upload {
                                id: uploaded.id,
                                url: uploaded.url,
                            },
                        );
                        if let Err(error) = rides::save(&fit, &record) {
                            tracing::warn!(%error, "cannot remember where the ride went");
                        }
                    }
                    Err(error) => tracing::warn!(%error, "cannot remember where the ride went"),
                }
                AppEvent::UploadFinished {
                    fit,
                    platform: platform.to_owned(),
                    url,
                }
            }
            Err(error) => AppEvent::UploadFailed {
                fit,
                platform: platform.to_owned(),
                reconnect: matches!(error, UploadError::Unauthorized(_)),
                message: error.to_string(),
            },
        }
    }
}

/// The ride `fit` to upload as `title`; an empty title falls back to the ride's name or route.
fn activity(fit: &Path, title: &str) -> Result<Activity, AppError> {
    let bytes = std::fs::read(fit)
        .map_err(|e| AppError::Storage(format!("cannot read {}: {e}", fit.display())))?;
    let title = title.trim();
    let name = if title.is_empty() {
        rides::load(fit).ok().map_or_else(
            || "Ride".to_owned(),
            |record| record.name.unwrap_or(record.route),
        )
    } else {
        title.to_owned()
    };
    Ok(Activity {
        fit: bytes,
        name,
        external_id: fit
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
    })
}

/// Uploads `activity` with the account's credentials as they are now, and stores them again
/// if the platform renewed them on the way. Runs in the upload's turn.
async fn upload_in_turn(
    data_dir: &Path,
    lock: &Mutex<()>,
    rider: &str,
    uploader: &dyn ActivityUploader,
    activity: &Activity,
) -> Result<Uploaded, UploadError> {
    let stored = {
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        accounts::load(data_dir, rider)
    };
    let mut credentials = stored
        .map_err(|e| UploadError::Rejected(format!("Cannot read the accounts: {e}")))?
        .remove(uploader.id())
        .ok_or_else(|| UploadError::Unauthorized(format!("Connect {} again.", uploader.name())))?
        .credentials;
    let before = credentials.clone();
    let result = uploader.upload(&mut credentials, activity).await;
    if credentials != before
        && let Err(error) = change_accounts(data_dir, lock, rider, |accounts| {
            if let Some(account) = accounts.get_mut(uploader.id()) {
                account.credentials = credentials;
            }
        })
    {
        tracing::warn!(%error, "cannot keep the renewed credentials");
    }
    result
}

/// Reads rider `rider`'s accounts, applies `change` and writes them, holding `lock`.
fn change_accounts(
    data_dir: &Path,
    lock: &Mutex<()>,
    rider: &str,
    change: impl FnOnce(&mut accounts::Accounts),
) -> Result<(), AppError> {
    let storage = |e: torqa_storage::profiles::ProfileError| {
        AppError::Storage(format!("cannot save the accounts: {e}"))
    };
    let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let mut stored = accounts::load(data_dir, rider).map_err(storage)?;
    change(&mut stored);
    accounts::save(data_dir, rider, &stored).map_err(storage)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{Duration, UNIX_EPOCH};

    use torqa_domain::recording::RideSummary;
    use torqa_domain::upload::BoxFuture;
    use torqa_storage::profiles;
    use torqa_storage::rides::RideRecord;
    use torqa_upload::Account;

    use super::*;
    use crate::tests::{run_until, temp_dir};

    /// A platform that takes the key `good` and keeps what it is sent. Like Strava, it
    /// renews a token with every upload and from then on takes only the new one; and it takes
    /// its time. The key `login` logs in through the browser first; with `never` the login
    /// never ends.
    struct Fake {
        id: &'static str,
        name: &'static str,
        received: Mutex<Vec<Activity>>,
        /// The token it gave out last.
        token: Mutex<Option<String>>,
        /// The credentials of the accounts disconnected.
        revoked: Mutex<Vec<Credentials>>,
    }

    impl Fake {
        fn new(id: &'static str, name: &'static str) -> Arc<Self> {
            Arc::new(Self {
                id,
                name,
                received: Mutex::default(),
                token: Mutex::default(),
                revoked: Mutex::default(),
            })
        }

        /// Waits a little for `count` accounts to be disconnected, as that happens in the
        /// background; their credentials.
        fn revoked(&self, count: usize) -> Vec<Credentials> {
            for _ in 0..100 {
                if self.revoked.lock().unwrap().len() >= count {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            self.revoked.lock().unwrap().clone()
        }

        fn received(&self) -> Vec<Activity> {
            self.received.lock().unwrap().clone()
        }
    }

    impl ActivityUploader for Fake {
        fn id(&self) -> &'static str {
            self.id
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn fields(&self) -> &'static [Field] {
            &[Field {
                key: "key",
                label: "Key",
                secret: true,
            }]
        }

        fn help(&self) -> &'static str {
            "Ask Fake."
        }

        fn help_url(&self) -> &'static str {
            "https://fake.example"
        }

        fn connect(
            &self,
            entered: Credentials,
            browser: Browser,
        ) -> BoxFuture<'_, Result<Account, UploadError>> {
            Box::pin(async move {
                match entered.get("key").map(String::as_str) {
                    Some("never") => std::future::pending::<()>().await,
                    Some("login") => (browser.open)("https://fake.example/login"),
                    _ => {}
                }
                if entered
                    .get("key")
                    .is_some_and(|k| k == "good" || k == "login")
                {
                    Ok(Account {
                        label: "Ann on Fake".to_owned(),
                        credentials: entered,
                    })
                } else {
                    Err(UploadError::Unauthorized(
                        "Fake does not know this key.".to_owned(),
                    ))
                }
            })
        }

        fn upload<'a>(
            &'a self,
            credentials: &'a mut Credentials,
            activity: &'a Activity,
        ) -> BoxFuture<'a, Result<Uploaded, UploadError>> {
            Box::pin(async move {
                if credentials.get("key").is_none_or(|k| k != "good") {
                    return Err(UploadError::Unauthorized("Connect Fake again.".to_owned()));
                }
                let presented = credentials.get("token").cloned();
                tokio::time::sleep(Duration::from_millis(20)).await;
                let mut token = self.token.lock().unwrap();
                if token.is_some() && presented != *token {
                    return Err(UploadError::Unauthorized(
                        "Fake renewed that token already.".to_owned(),
                    ));
                }
                let mut received = self.received.lock().unwrap();
                received.push(activity.clone());
                let renewed = format!("renewed {}", received.len());
                *token = Some(renewed.clone());
                credentials.insert("token".to_owned(), renewed);
                Ok(Uploaded {
                    id: "a1".to_owned(),
                    url: "https://fake.example/a1".to_owned(),
                })
            })
        }

        fn disconnect<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, ()> {
            Box::pin(async move {
                self.revoked.lock().unwrap().push(credentials.clone());
            })
        }
    }

    fn app(name: &str) -> (App, Arc<Fake>, PathBuf) {
        let dir = temp_dir(name);
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let fake = Fake::new("fake", "Fake");
        app.set_upload_platforms(vec![fake.clone()]);
        (app, fake, dir)
    }

    /// A ride of the active rider, named `name`.
    fn ride(app: &App, name: Option<&str>) -> PathBuf {
        ride_at(app, "torqa-20261012-071500", name)
    }

    /// A ride of the active rider in `stem.fit`, named `name`.
    fn ride_at(app: &App, stem: &str, name: Option<&str>) -> PathBuf {
        let dir = profiles::rides_dir(&app.data_dir, &app.profile().id);
        std::fs::create_dir_all(&dir).unwrap();
        let fit = dir.join(format!("{stem}.fit"));
        std::fs::write(&fit, b"FIT activity").unwrap();
        let record = RideRecord {
            route: "Gurten".to_owned(),
            start: UNIX_EPOCH,
            summary: RideSummary::default(),
            route_key: None,
            route_time: None,
            climbs: Vec::new(),
            name: name.map(ToOwned::to_owned),
            ftp_estimate: None,
            rider_mass: None,
            simulated: false,
            analysis: None,
            uploads: BTreeMap::new(),
        };
        rides::save(&fit, &record).unwrap();
        fit
    }

    fn connect(app: &mut App, key: &str) -> AppEvent {
        connect_to(app, "fake", key)
    }

    fn connect_to(app: &mut App, platform: &str, key: &str) -> AppEvent {
        start_connecting(app, platform, key);
        run_until(app, |e| {
            matches!(
                e,
                AppEvent::AccountConnected { .. } | AppEvent::AccountFailed { .. }
            )
        })
        .pop()
        .unwrap()
    }

    fn start_connecting(app: &mut App, platform: &str, key: &str) {
        let rider = app.profile().id.clone();
        let entered = Credentials::from([("key".to_owned(), key.to_owned())]);
        let pages = LoginPages {
            connected: "Connected.".to_owned(),
            failed: "Not connected.".to_owned(),
        };
        app.connect_account(&rider, platform, entered, pages)
            .unwrap();
    }

    fn upload(app: &mut App, fit: &Path, title: &str) -> AppEvent {
        app.upload_ride(fit, "fake", title).unwrap();
        uploads_done(app, 1).pop().unwrap()
    }

    /// Runs the app until `count` uploads have finished or failed; those events.
    fn uploads_done(app: &mut App, count: usize) -> Vec<AppEvent> {
        let done = |e: &AppEvent| {
            matches!(
                e,
                AppEvent::UploadFinished { .. } | AppEvent::UploadFailed { .. }
            )
        };
        let mut seen = 0;
        let events = run_until(app, |e| {
            seen += usize::from(done(e));
            seen == count
        });
        events.into_iter().filter(done).collect()
    }

    fn label(app: &App, rider: &str) -> Option<String> {
        app.accounts(rider).remove(0).label
    }

    #[test]
    fn every_platform_is_offered_with_what_it_asks_for() {
        let (app, _, dir) = app("platforms");

        let accounts = app.accounts(&app.profile().id);

        assert_eq!(accounts.len(), 1);
        let fake = &accounts[0];
        assert_eq!((fake.platform, fake.name), ("fake", "Fake"));
        assert_eq!(fake.fields[0].key, "key");
        assert_eq!(fake.label, None);
        assert!(!fake.auto_upload);
        assert_eq!(
            platforms().iter().map(|p| p.id()).collect::<Vec<_>>(),
            ["intervals", "strava"],
            "intervals.icu and Strava are offered in the app"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_connected_account_belongs_to_its_rider_alone() {
        let (mut app, _, dir) = app("connect");
        let rider = app.profile().id.clone();

        let event = connect(&mut app, "good");

        assert_eq!(
            event,
            AppEvent::AccountConnected {
                rider: rider.clone(),
                platform: "fake".to_owned(),
                label: "Ann on Fake".to_owned(),
            }
        );
        assert_eq!(label(&app, &rider).as_deref(), Some("Ann on Fake"));
        assert_eq!(label(&app, "someone-else"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_refused_connection_keeps_nothing() {
        let (mut app, _, dir) = app("refused");
        let rider = app.profile().id.clone();

        let event = connect(&mut app, "wrong");

        assert!(
            matches!(&event, AppEvent::AccountFailed { message, .. } if message.contains("key")),
            "{event:?}"
        );
        assert_eq!(label(&app, &rider), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_ride_goes_up_under_its_title_and_links_its_activity() {
        let (mut app, fake, dir) = app("upload");
        connect(&mut app, "good");
        let fit = ride(&app, None);

        let event = upload(&mut app, &fit, "Gurten · Mon 12 Oct");

        assert_eq!(
            event,
            AppEvent::UploadFinished {
                fit: fit.clone(),
                platform: "fake".to_owned(),
                url: "https://fake.example/a1".to_owned(),
            }
        );
        let sent = fake.received()[0].clone();
        assert_eq!(sent.name, "Gurten · Mon 12 Oct");
        assert_eq!(sent.fit, b"FIT activity");
        assert_eq!(sent.external_id, "torqa-20261012-071500");
        let uploads = &app.history()[0].record.uploads;
        assert_eq!(uploads["fake"].url, "https://fake.example/a1");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_ride_without_a_title_goes_up_under_its_name() {
        let (mut app, fake, dir) = app("untitled");
        connect(&mut app, "good");
        let fit = ride(&app, Some("Morning loop"));

        upload(&mut app, &fit, " ");

        assert_eq!(fake.received()[0].name, "Morning loop");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn credentials_renewed_on_the_way_are_kept() {
        let (mut app, _, dir) = app("renewed");
        connect(&mut app, "good");
        let fit = ride(&app, None);

        upload(&mut app, &fit, "Ride");
        upload(&mut app, &fit, "Ride");

        let stored = accounts::load(&app.data_dir, &app.profile().id).unwrap();
        assert_eq!(stored["fake"].credentials["token"], "renewed 2");
        assert_eq!(stored["fake"].credentials["key"], "good");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rides_go_up_to_connected_accounts_only() {
        let (mut app, _, dir) = app("unconnected");
        let fit = ride(&app, None);

        assert!(matches!(
            app.upload_ride(&fit, "fake", "Ride"),
            Err(AppError::NotConnected("Fake"))
        ));
        assert!(matches!(
            app.upload_ride(&fit, "elsewhere", "Ride"),
            Err(AppError::UnknownPlatform)
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_account_the_platform_no_longer_accepts_asks_to_connect_again() {
        let (mut app, _, dir) = app("revoked");
        let rider = app.profile().id.clone();
        let revoked = StoredAccount {
            label: "Ann on Fake".to_owned(),
            auto_upload: false,
            credentials: Credentials::from([("key".to_owned(), "revoked".to_owned())]),
        };
        accounts::save(
            &app.data_dir,
            &rider,
            &accounts::Accounts::from([("fake".to_owned(), revoked)]),
        )
        .unwrap();
        let fit = ride(&app, None);

        let event = upload(&mut app, &fit, "Ride");

        assert!(
            matches!(
                event,
                AppEvent::UploadFailed {
                    reconnect: true,
                    ..
                }
            ),
            "{event:?}"
        );
        assert!(app.history()[0].record.uploads.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_disconnected_account_is_forgotten() {
        let (mut app, _, dir) = app("disconnect");
        let rider = app.profile().id.clone();
        connect(&mut app, "good");

        app.disconnect_account(&rider, "fake").unwrap();

        assert_eq!(label(&app, &rider), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn kept_rides_go_up_to_the_accounts_set_to_upload_them_automatically() {
        let dir = temp_dir("keep");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let (auto, asked, unconnected) = (
            Fake::new("fake", "Fake"),
            Fake::new("asked", "Asked"),
            Fake::new("unconnected", "Unconnected"),
        );
        app.set_upload_platforms(vec![auto.clone(), asked.clone(), unconnected.clone()]);
        let rider = app.profile().id.clone();
        connect_to(&mut app, "fake", "good");
        connect_to(&mut app, "asked", "good");
        app.set_auto_upload(&rider, "fake", true).unwrap();
        let fit = ride(&app, None);

        let started = app.keep_ride(&fit, "Morning loop");
        let event = uploads_done(&mut app, 1).pop().unwrap();

        assert_eq!(started, ["fake"]);
        assert!(
            matches!(event, AppEvent::UploadFinished { .. }),
            "{event:?}"
        );
        assert_eq!(auto.received()[0].name, "Morning loop");
        assert!(asked.received().is_empty() && unconnected.received().is_empty());
        let accounts = app.accounts(&rider);
        assert_eq!(
            accounts.iter().map(|a| a.auto_upload).collect::<Vec<_>>(),
            [true, false, false]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_ride_goes_to_a_platform_once_even_when_kept_while_uploading() {
        let (mut app, fake, dir) = app("once");
        let rider = app.profile().id.clone();
        connect(&mut app, "good");
        app.set_auto_upload(&rider, "fake", true).unwrap();
        let fit = ride(&app, None);

        app.upload_ride(&fit, "fake", "Ride").unwrap();
        let while_uploading = app.uploading(&fit);
        let kept_while_uploading = app.keep_ride(&fit, "Ride");
        app.upload_ride(&fit, "fake", "Ride").unwrap();
        uploads_done(&mut app, 1);
        let kept_after = app.keep_ride(&fit, "Ride");

        assert_eq!(while_uploading, ["fake"]);
        assert!(kept_while_uploading.is_empty() && kept_after.is_empty());
        assert_eq!(app.uploading(&fit), [""; 0]);
        assert_eq!(fake.received().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn uploads_take_turns_so_a_renewed_token_is_never_stale() {
        let (mut app, fake, dir) = app("turns");
        connect(&mut app, "good");
        let first = ride_at(&app, "torqa-20261012-071500", None);
        let second = ride_at(&app, "torqa-20261012-181500", None);

        app.upload_ride(&first, "fake", "First").unwrap();
        app.upload_ride(&second, "fake", "Second").unwrap();
        let events = uploads_done(&mut app, 2);

        assert!(
            events
                .iter()
                .all(|e| matches!(e, AppEvent::UploadFinished { .. })),
            "{events:?}"
        );
        assert_eq!(fake.received().len(), 2);
        let stored = accounts::load(&app.data_dir, &app.profile().id).unwrap();
        assert_eq!(stored["fake"].credentials["token"], "renewed 2");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn automatic_uploads_need_an_account_and_survive_connecting_again() {
        let (mut app, _, dir) = app("auto-setting");
        let rider = app.profile().id.clone();

        assert!(matches!(
            app.set_auto_upload(&rider, "fake", true),
            Err(AppError::NotConnected("Fake"))
        ));
        connect(&mut app, "good");
        app.set_auto_upload(&rider, "fake", true).unwrap();
        connect(&mut app, "good");

        assert!(app.accounts(&rider)[0].auto_upload);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn platforms_that_log_in_open_the_riders_browser_first() {
        let (mut app, _, dir) = app("login");

        start_connecting(&mut app, "fake", "login");
        let events = run_until(&mut app, |e| matches!(e, AppEvent::AccountConnected { .. }));

        let opened = events
            .iter()
            .position(|e| *e == AppEvent::OpenUrl("https://fake.example/login".to_owned()));
        assert!(
            opened.is_some_and(|i| i < events.len() - 1),
            "the login opens before the account is connected: {events:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_cancelled_login_connects_nothing_and_a_new_one_can_start() {
        let (mut app, _, dir) = app("cancel");
        let rider = app.profile().id.clone();

        start_connecting(&mut app, "fake", "never");
        app.cancel_connect();
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        let mut events = Vec::new();
        while std::time::Instant::now() < deadline {
            events.extend(app.update(Duration::from_millis(16)));
            std::thread::sleep(Duration::from_millis(16));
        }
        let connected_after = connect(&mut app, "good");

        assert!(
            !events.iter().any(|e| matches!(
                e,
                AppEvent::AccountConnected { .. } | AppEvent::AccountFailed { .. }
            )),
            "{events:?}"
        );
        assert!(matches!(connected_after, AppEvent::AccountConnected { .. }));
        assert_eq!(label(&app, &rider).as_deref(), Some("Ann on Fake"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disconnecting_withdraws_torqas_access() {
        let (mut app, fake, dir) = app("revoke");
        let rider = app.profile().id.clone();
        connect(&mut app, "good");

        app.disconnect_account(&rider, "fake").unwrap();

        let revoked = fake.revoked(1);
        assert_eq!(revoked.len(), 1);
        assert_eq!(revoked[0]["key"], "good");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deleting_a_rider_withdraws_torqas_access_to_their_accounts() {
        let (mut app, fake, dir) = app("delete-rider");
        let leaving = app.profile().id.clone();
        connect(&mut app, "good");
        let staying = torqa_domain::profile::Profile {
            name: "Bob".to_owned(),
            ..torqa_domain::profile::Profile::default()
        };
        app.save_profile(None, staying).unwrap();

        app.delete_profile(&leaving).unwrap();

        assert_eq!(fake.revoked(1).len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
