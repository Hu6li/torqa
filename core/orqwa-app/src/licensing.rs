//! Sold worlds on this device (ADR 0043, R293): which worlds it may open, activating one with a
//! licence key and releasing it again. The device secret and activations live in the device
//! folder ([`crate::paths::device_dir`]), never in the data folder that riders sync. Only
//! activating and releasing reach the network; opening an activated world never does.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use orqwa_licence::LicenceError;
use orqwa_licence::activation::{Activation, ActivationFile};
use orqwa_licence::device::{self, Device};
use orqwa_licence::protocol::{
    ActivateRequest, ActivateResponse, Refusal, Refused, ReleaseRequest,
};
use orqwa_licence::stream::ContentKey;
use orqwa_licence::trust::{Signed, Trust};
use orqwa_storage::world::WorldFile;
use tracing::warn;

/// The activation server. A placeholder until it runs: activating fails until then.
pub const SERVER: &str = "https://activate.orqwa.invalid";
const ACTIVATIONS: &str = "activations";
const REVOCATIONS: &str = "revocations.json";
/// The largest answer the server sends: an activation with two certificates and a list.
const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;

/// Why activating or releasing failed.
#[derive(Debug, thiserror::Error)]
pub enum LicensingError {
    /// The device folder lies in the data folder, which may sync to other devices, or there is
    /// none.
    #[error("the device folder {0} lies in the data folder")]
    InDataFolder(PathBuf),
    /// The server could not be reached.
    #[error("could not reach the activation server: {0}")]
    Unreachable(String),
    /// The server refused.
    #[error("the activation server refused: {0:?}")]
    Refused(Refusal),
    /// The server's answer made no sense.
    #[error("the activation server answered unexpectedly: {0}")]
    Server(String),
    /// The activation does not hold.
    #[error(transparent)]
    Licence(#[from] LicenceError),
    /// Reading or writing the device folder.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// This device's licensing: cheap to clone, shared with jobs.
#[derive(Clone)]
pub struct Licensing {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    usable: bool,
    server: String,
    machine: Mutex<Option<Vec<u8>>>,
    trust: RwLock<Trust>,
}

impl std::fmt::Debug for Licensing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Licensing")
            .field("dir", &self.inner.dir)
            .finish_non_exhaustive()
    }
}

impl Licensing {
    /// Licensing kept in `dir` ([`crate::paths::device_dir`]); refused without one, or if it
    /// lies in `data_dir`.
    #[must_use]
    pub fn new(dir: Option<PathBuf>, data_dir: &Path) -> Self {
        let Some(dir) = dir else {
            warn!("no device folder: no activations");
            return Self::with_server(PathBuf::new(), data_dir, SERVER, false);
        };
        Self::with_server(dir, data_dir, SERVER, true)
    }

    pub(crate) fn with_server(dir: PathBuf, data_dir: &Path, server: &str, usable: bool) -> Self {
        let usable = usable && crate::paths::outside(&dir, data_dir);
        if !usable {
            warn!(dir = %dir.display(), "the device folder lies in the data folder: no activations");
        }
        let mut trust = Trust::builtin();
        for signed in stored_revocations(&dir) {
            if let Err(error) = trust.accept_revocations(&signed) {
                warn!(%error, "ignoring stored withdrawals no root signed");
            }
        }
        Self {
            inner: Arc::new(Inner {
                dir,
                usable,
                server: server.trim_end_matches('/').to_owned(),
                machine: Mutex::new(None),
                trust: RwLock::new(trust),
            }),
        }
    }

    /// The operating system's id for this device, where ORQWA cannot read it itself (Android,
    /// through Java). An empty one is ignored.
    pub fn set_machine_id(&self, id: Vec<u8>) {
        if !id.is_empty() {
            *lock(&self.inner.machine) = Some(id);
        }
    }

    /// The machine id activations are bound to; none, and nothing is activated or opened, as an
    /// activation not bound to the machine would open wherever its folder is copied.
    fn machine(&self) -> Result<Vec<u8>, LicensingError> {
        lock(&self.inner.machine)
            .clone()
            .or_else(device::machine_id)
            .ok_or(LicensingError::Licence(LicenceError::NoMachine))
    }

    /// The roots and the newest withdrawals world files are checked against.
    #[must_use]
    pub fn trust(&self) -> Trust {
        self.inner
            .trust
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn device(&self) -> Result<Device, LicensingError> {
        if !self.inner.usable {
            return Err(LicensingError::InDataFolder(self.inner.dir.clone()));
        }
        Ok(Device::load_or_create(&self.inner.dir, &self.machine()?)?)
    }

    fn activation_file(&self, world: &str) -> PathBuf {
        // World ids are UUIDs (docs/world-files.md): safe as file names.
        self.inner
            .dir
            .join(ACTIVATIONS)
            .join(format!("{world}.json"))
    }

    /// The activation and content key of `world` on this device, if it holds here.
    #[must_use]
    pub fn activation(&self, world: &str) -> Option<(Activation, ContentKey)> {
        let text = std::fs::read_to_string(self.activation_file(world)).ok()?;
        let opened = (|| {
            let file: ActivationFile = serde_json::from_str(&text)?;
            let device = self.device()?;
            Ok::<_, LicensingError>(file.open(&self.trust(), world, &device)?)
        })();
        opened
            .inspect_err(|error| warn!(world, %error, "the activation does not hold here"))
            .ok()
    }

    /// Unlocks a world activated on this device; returns the buyer's name.
    pub fn unlock(&self, file: &mut WorldFile) -> Option<String> {
        file.content_key()?;
        let (activation, key) = self.activation(&file.description().id)?;
        file.unlock(key)
            .inspect_err(|error| warn!(%error, "the activation's key is not the world's"))
            .ok()?;
        Some(activation.licensee)
    }

    /// Activates `world` on this device with the licence key from the purchase; returns the
    /// buyer's name. Nothing is kept unless the activation holds here.
    ///
    /// # Errors
    /// [`LicensingError`] if the device folder is unusable, the server cannot be reached or
    /// refuses, or what it sends does not hold.
    pub async fn activate(&self, world: &str, licence_key: &str) -> Result<String, LicensingError> {
        let device = self.device()?;
        let request = ActivateRequest {
            licence_key: licence_key.trim().to_owned(),
            world: world.to_owned(),
            device: device.id().to_owned(),
            device_key: device.key().bytes().to_vec(),
        };
        let answer: ActivateResponse =
            serde_json::from_slice(&self.post("/v1/activate", &request).await?)?;
        if let Some(signed) = &answer.revocations {
            self.take_revocations(signed);
        }
        let (activation, _) = answer.activation.open(&self.trust(), world, &device)?;
        let path = self.activation_file(world);
        std::fs::create_dir_all(path.parent().unwrap_or(&self.inner.dir))?;
        device::write_private(
            &path,
            serde_json::to_string_pretty(&answer.activation)?.as_bytes(),
        )?;
        Ok(activation.licensee)
    }

    /// Releases this device from every paid world on it, freeing its slot in each licence, then
    /// gives it a new identity: activations kept aside, or put back, open nothing afterwards.
    /// They are set aside first, so a freed slot never leaves a world open here; if the server
    /// cannot be reached or refuses, they are put back and the device stays as it was.
    ///
    /// # Errors
    /// [`LicensingError`] if the device folder is unusable, an activation cannot be set aside,
    /// or the server cannot be reached or refuses.
    pub async fn release(&self) -> Result<(), LicensingError> {
        let device = self.device()?;
        let kept = self.set_activations_aside()?;
        let request = ReleaseRequest {
            device: device.id().to_owned(),
            device_key: device.key().bytes().to_vec(),
        };
        match self.post("/v1/release", &request).await {
            Ok(_) | Err(LicensingError::Refused(Refusal::UnknownDevice)) => {
                Device::renew(&self.inner.dir, &self.machine()?)?;
                Ok(())
            }
            Err(error) => {
                for (path, bytes) in kept {
                    if let Err(restore) = device::write_private(&path, &bytes) {
                        warn!(%restore, path = %path.display(), "cannot put an activation back");
                    }
                }
                Err(error)
            }
        }
    }

    /// Removes every activation on this device, handing back what each held; all or none.
    fn set_activations_aside(&self) -> Result<Vec<(PathBuf, Vec<u8>)>, LicensingError> {
        let dir = self.inner.dir.join(ACTIVATIONS);
        let listing = match std::fs::read_dir(&dir) {
            Ok(listing) => listing,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut kept = Vec::new();
        for path in listing.filter_map(Result::ok).map(|entry| entry.path()) {
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let removed = std::fs::read(&path).and_then(|bytes| {
                std::fs::remove_file(&path)?;
                Ok(bytes)
            });
            match removed {
                Ok(bytes) => kept.push((path, bytes)),
                Err(error) => {
                    for (path, bytes) in kept {
                        let _ = device::write_private(&path, &bytes);
                    }
                    return Err(error.into());
                }
            }
        }
        Ok(kept)
    }

    fn take_revocations(&self, signed: &Signed) {
        let mut trust = self
            .inner
            .trust
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match trust.accept_revocations(signed) {
            Ok(true) => {
                // Every list that withdrew something is kept: they add up, each checked again
                // when ORQWA starts.
                let mut lists = stored_revocations(&self.inner.dir);
                lists.push(signed.clone());
                let kept = serde_json::to_string_pretty(&lists)
                    .map_err(std::io::Error::other)
                    .and_then(|text| {
                        device::write_private(&self.inner.dir.join(REVOCATIONS), text.as_bytes())
                    });
                if let Err(error) = kept {
                    warn!(%error, "cannot keep the newest withdrawals");
                }
            }
            Ok(false) => {}
            Err(error) => warn!(%error, "ignoring withdrawals no root signed"),
        }
    }

    async fn post<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<Vec<u8>, LicensingError> {
        let unreachable = |e: reqwest::Error| LicensingError::Unreachable(e.to_string());
        let response = client(&self.inner.server)?
            .post(format!("{}{path}", self.inner.server))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(body)?)
            .send()
            .await
            .map_err(unreachable)?;
        let status = response.status();
        let too_large = || LicensingError::Server("the answer is too large".into());
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ANSWER_BYTES as u64)
        {
            return Err(too_large());
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(unreachable)? {
            if bytes.len() + chunk.len() > MAX_ANSWER_BYTES {
                return Err(too_large());
            }
            bytes.extend_from_slice(&chunk);
        }
        if status.is_success() {
            return Ok(bytes);
        }
        match serde_json::from_slice::<Refused>(&bytes) {
            Ok(refused) => Err(LicensingError::Refused(refused.refusal)),
            Err(_) => Err(LicensingError::Server(format!("status {status}"))),
        }
    }
}

/// An HTTPS client that agrees keys only with the hybrid post-quantum X25519MLKEM768, over TLS
/// 1.3, checking the server's certificate as the operating system does (ADR 0043).
fn client(server: &str) -> Result<reqwest::Client, LicensingError> {
    use rustls_platform_verifier::BuilderVerifierExt as _;
    // No redirects: one would carry the licence key and device key to another host.
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("ORQWA/", env!("CARGO_PKG_VERSION")));
    if server.starts_with("https://") {
        let mut provider = rustls::crypto::aws_lc_rs::default_provider();
        provider.kx_groups = vec![rustls::crypto::aws_lc_rs::kx_group::X25519MLKEM768];
        let tls = |e: rustls::Error| LicensingError::Server(e.to_string());
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(tls)?
            .with_platform_verifier()
            .map_err(tls)?
            .with_no_client_auth();
        builder = builder.tls_backend_preconfigured(config).https_only(true);
    } else if !cfg!(test) {
        return Err(LicensingError::Server(
            "the activation server must use HTTPS".into(),
        ));
    }
    builder
        .build()
        .map_err(|e| LicensingError::Unreachable(e.to_string()))
}

/// The withdrawal lists this device has kept, each still to be checked.
fn stored_revocations(dir: &Path) -> Vec<Signed> {
    std::fs::read_to_string(dir.join(REVOCATIONS))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
#[path = "licensing_tests.rs"]
pub(crate) mod tests;
