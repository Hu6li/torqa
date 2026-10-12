//! Uploading rides to training platforms (R29): a rider's accounts, connected with what the
//! platform asks for, and rides uploaded in the background, each remembering where it went.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use torqa_storage::accounts::{self, StoredAccount};
use torqa_storage::rides;
pub use torqa_upload::{Activity, ActivityUploader, Credentials, Field, UploadError, Uploaded};

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
}

/// Upload work done in the background, for the frame loop to keep.
pub(crate) enum Done {
    Connected {
        rider: String,
        platform: &'static str,
        result: Result<torqa_upload::Account, UploadError>,
    },
    Uploaded(UploadJob),
}

/// A finished upload.
pub(crate) struct UploadJob {
    rider: String,
    platform: &'static str,
    fit: PathBuf,
    /// The credentials after the upload, if they changed on the way.
    renewed: Option<Credentials>,
    result: Result<Uploaded, UploadError>,
}

impl App {
    /// Takes `platforms` as those rides can be uploaded to instead of [`platforms`], e.g.
    /// stand-ins in tests.
    pub fn set_upload_platforms(&mut self, platforms: Vec<Arc<dyn ActivityUploader>>) {
        self.uploaders = platforms;
    }

    /// Every platform with rider `rider`'s account on it, in the order they are offered.
    #[must_use]
    pub fn accounts(&self, rider: &str) -> Vec<AccountStatus> {
        let stored = accounts::load(&self.data_dir, rider).unwrap_or_else(|error| {
            tracing::warn!(%error, "cannot read the rider's accounts");
            accounts::Accounts::new()
        });
        self.uploaders
            .iter()
            .map(|uploader| AccountStatus {
                platform: uploader.id(),
                name: uploader.name(),
                fields: uploader.fields(),
                help: uploader.help(),
                help_url: uploader.help_url(),
                label: stored.get(uploader.id()).map(|a| a.label.clone()),
            })
            .collect()
    }

    /// Connects rider `rider`'s account on `platform` with the fields they `entered`, in the
    /// background: reports [`AppEvent::AccountConnected`] or [`AppEvent::AccountFailed`].
    ///
    /// # Errors
    /// [`AppError::UnknownPlatform`] if there is no such platform.
    pub fn connect_account(
        &mut self,
        rider: &str,
        platform: &str,
        entered: Credentials,
    ) -> Result<(), AppError> {
        let uploader = self.uploader(platform)?;
        let tx = self.jobs_tx.clone();
        let rider = rider.to_owned();
        self.runtime.spawn(async move {
            let result = uploader.connect(entered).await;
            let _ = tx.send(JobResult::Upload(Box::new(Done::Connected {
                rider,
                platform: uploader.id(),
                result,
            })));
        });
        Ok(())
    }

    /// Forgets rider `rider`'s account on `platform`.
    ///
    /// # Errors
    /// [`AppError::Storage`] if the accounts cannot be read or written.
    pub fn disconnect_account(&mut self, rider: &str, platform: &str) -> Result<(), AppError> {
        self.change_accounts(rider, |accounts| {
            accounts.remove(platform);
        })
    }

    /// Uploads the active rider's ride `fit` to `platform` as `title` (R50), in the
    /// background: reports [`AppEvent::UploadFinished`] or [`AppEvent::UploadFailed`]. An
    /// empty title falls back to the ride's name or route.
    ///
    /// # Errors
    /// [`AppError::UnknownPlatform`] if there is no such platform, [`AppError::NotConnected`]
    /// if the rider has no account there, [`AppError::Storage`] if the ride cannot be read.
    pub fn upload_ride(&mut self, fit: &Path, platform: &str, title: &str) -> Result<(), AppError> {
        let uploader = self.uploader(platform)?;
        let rider = self.profile.id.clone();
        let mut credentials = accounts::load(&self.data_dir, &rider)
            .map_err(|e| AppError::Storage(format!("cannot read the accounts: {e}")))?
            .remove(uploader.id())
            .ok_or(AppError::NotConnected(uploader.name()))?
            .credentials;
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
        let activity = Activity {
            fit: bytes,
            name,
            external_id: fit
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
        };
        let tx = self.jobs_tx.clone();
        let fit = fit.to_owned();
        self.runtime.spawn(async move {
            let before = credentials.clone();
            let result = uploader.upload(&mut credentials, &activity).await;
            let _ = tx.send(JobResult::Upload(Box::new(Done::Uploaded(UploadJob {
                rider,
                platform: uploader.id(),
                fit,
                renewed: (credentials != before).then_some(credentials),
                result,
            }))));
        });
        Ok(())
    }

    fn uploader(&self, platform: &str) -> Result<Arc<dyn ActivityUploader>, AppError> {
        self.uploaders
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
        let storage = |e: torqa_storage::profiles::ProfileError| {
            AppError::Storage(format!("cannot save the accounts: {e}"))
        };
        let mut stored = accounts::load(&self.data_dir, rider).map_err(storage)?;
        change(&mut stored);
        accounts::save(&self.data_dir, rider, &stored).map_err(storage)
    }

    /// Keeps what upload work in the background did, and tells what came of it.
    pub(crate) fn upload_work_done(&self, done: Done) -> AppEvent {
        match done {
            Done::Connected {
                rider,
                platform,
                result,
            } => self.account_connected(rider, platform, result),
            Done::Uploaded(job) => self.upload_done(job),
        }
    }

    /// Keeps a connected account.
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
        let stored = StoredAccount {
            label: account.label,
            credentials: account.credentials,
        };
        match self.change_accounts(&rider, |accounts| {
            accounts.insert(platform.to_owned(), stored);
        }) {
            Ok(()) => AppEvent::AccountConnected {
                rider,
                platform: platform.to_owned(),
                label,
            },
            Err(error) => failed(error.to_string()),
        }
    }

    /// Keeps what changed in an upload: renewed credentials and where the ride went.
    fn upload_done(&self, job: UploadJob) -> AppEvent {
        let UploadJob {
            rider,
            platform,
            fit,
            renewed,
            result,
        } = job;
        if let Some(credentials) = renewed
            && let Err(error) = self.change_accounts(&rider, |accounts| {
                if let Some(account) = accounts.get_mut(platform) {
                    account.credentials = credentials;
                }
            })
        {
            tracing::warn!(%error, "cannot keep the renewed credentials");
        }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::time::UNIX_EPOCH;

    use torqa_domain::recording::RideSummary;
    use torqa_domain::upload::BoxFuture;
    use torqa_storage::profiles;
    use torqa_storage::rides::RideRecord;
    use torqa_upload::Account;

    use super::*;
    use crate::tests::{run_until, temp_dir};

    /// A platform that takes the key `good` and keeps what it is sent; every upload renews a
    /// token, as OAuth platforms do.
    #[derive(Default)]
    struct Fake {
        received: Mutex<Vec<Activity>>,
    }

    impl ActivityUploader for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }

        fn name(&self) -> &'static str {
            "Fake"
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

        fn connect(&self, entered: Credentials) -> BoxFuture<'_, Result<Account, UploadError>> {
            Box::pin(async move {
                if entered.get("key").is_some_and(|k| k == "good") {
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
                let mut received = self.received.lock().unwrap();
                received.push(activity.clone());
                credentials.insert("token".to_owned(), format!("renewed {}", received.len()));
                Ok(Uploaded {
                    id: "a1".to_owned(),
                    url: "https://fake.example/a1".to_owned(),
                })
            })
        }
    }

    fn app(name: &str) -> (App, Arc<Fake>, PathBuf) {
        let dir = temp_dir(name);
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let fake = Arc::new(Fake::default());
        app.set_upload_platforms(vec![fake.clone()]);
        (app, fake, dir)
    }

    /// A ride of the active rider, named `name`.
    fn ride(app: &App, name: Option<&str>) -> PathBuf {
        let dir = profiles::rides_dir(&app.data_dir, &app.profile().id);
        std::fs::create_dir_all(&dir).unwrap();
        let fit = dir.join("torqa-20261012-071500.fit");
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
            uploads: BTreeMap::new(),
        };
        rides::save(&fit, &record).unwrap();
        fit
    }

    fn connect(app: &mut App, key: &str) -> AppEvent {
        let rider = app.profile().id.clone();
        let entered = Credentials::from([("key".to_owned(), key.to_owned())]);
        app.connect_account(&rider, "fake", entered).unwrap();
        run_until(app, |e| {
            matches!(
                e,
                AppEvent::AccountConnected { .. } | AppEvent::AccountFailed { .. }
            )
        })
        .pop()
        .unwrap()
    }

    fn upload(app: &mut App, fit: &Path, title: &str) -> AppEvent {
        app.upload_ride(fit, "fake", title).unwrap();
        run_until(app, |e| {
            matches!(
                e,
                AppEvent::UploadFinished { .. } | AppEvent::UploadFailed { .. }
            )
        })
        .pop()
        .unwrap()
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
        assert_eq!(
            platforms().iter().map(|p| p.id()).collect::<Vec<_>>(),
            ["intervals"],
            "intervals.icu is offered in the app"
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
        let sent = fake.received.lock().unwrap()[0].clone();
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

        assert_eq!(fake.received.lock().unwrap()[0].name, "Morning loop");
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
}
