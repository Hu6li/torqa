//! Content-addressed, bounded reliable transfers, independent of voice and state datagrams.
//!
//! A verified download is still untrusted application input. [`Received::validate`] must
//! succeed before it can be published in a library. Callers own one transfer task per peer;
//! dropping a future cancels its streams and removes its unique temporary file. A course may
//! arrive as the sender's own copy, under its own identity: its validator checks it is the
//! selected course by its key (#435).

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use quinn::{RecvStream, SendStream};
use tokio::fs::File;
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
use tokio::sync::watch;
use tokio::time::{Instant, timeout_at};

use crate::activity::{ActivityError, ContentId, ContentKind, Input};
use crate::crypto::{random, sha256_provider};
use crate::link::Connection;

mod publication;

const MAGIC: &[u8; 4] = b"OQC1";
const HEADER: usize = 45;
const CHUNK: usize = 16 * 1024;
/// Content preparation/transfer budget, independent of the one-minute connection deadline.
pub const TRANSFER_WINDOW: Duration = Duration::from_mins(15);
/// Maximum time without a completed read or write while transferring.
pub const IDLE_WINDOW: Duration = Duration::from_secs(30);

/// A transfer failed; peer-supplied text is never included in these diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    /// Invalid identity or advertised length.
    #[error(transparent)]
    Activity(#[from] ActivityError),
    /// Local storage failure, including insufficient free space.
    #[error("The shared activity could not be read or stored: {0}")]
    Io(#[from] io::Error),
    /// A read/write/whole-transfer deadline expired.
    #[error("The activity transfer timed out. Retry the download.")]
    Timeout,
    /// The authenticated transport closed or a stream was reset.
    #[error("The activity transfer was interrupted. Retry the download.")]
    Interrupted,
    /// Content did not match the selected size/hash, or the response carried extra bytes.
    #[error("The downloaded activity does not match the selected content.")]
    Mismatch,
    /// Application validation rejected the complete file; it must not reach the library.
    #[error("The shared course or workout is invalid or unsupported.")]
    Validation,
}

/// Coalesced progress: one value per transfer, with no accumulated per-chunk event queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Bytes successfully stored in the temporary file.
    pub received: u64,
    /// Expected complete size from the activity selection.
    pub total: u64,
}

fn end(deadline: Instant) -> Instant {
    deadline.min(Instant::now() + IDLE_WINDOW)
}

fn header(id: ContentId) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    id.write(&mut bytes);
    bytes
}
fn identity(bytes: &[u8; HEADER]) -> Result<ContentId, ContentError> {
    if &bytes[..4] != MAGIC {
        return Err(ContentError::Mismatch);
    }
    Ok(ContentId::read(&mut Input(&bytes[4..]))?)
}

async fn hash(
    file: &mut File,
    kind: ContentKind,
    deadline: Instant,
) -> Result<ContentId, ContentError> {
    let metadata = timeout_at(end(deadline), file.metadata())
        .await
        .map_err(|_| ContentError::Timeout)??;
    if !metadata.is_file() {
        return Err(ContentError::Validation);
    }
    let bytes = metadata.len();
    ContentId::new(kind, bytes, [0; 32])?;
    let mut hasher = sha256_provider().start();
    let mut chunk = vec![0; CHUNK];
    let mut read = 0;
    loop {
        let count = timeout_at(end(deadline), file.read(&mut chunk))
            .await
            .map_err(|_| ContentError::Timeout)??;
        if count == 0 {
            break;
        }
        read += u64::try_from(count).map_err(|_| ContentError::Mismatch)?;
        if read > bytes {
            return Err(ContentError::Mismatch);
        }
        hasher.update(&chunk[..count]);
    }
    if read != bytes {
        return Err(ContentError::Mismatch);
    }
    Ok(ContentId::new(
        kind,
        bytes,
        hasher
            .finish()
            .as_ref()
            .try_into()
            .map_err(|_| ContentError::Mismatch)?,
    )?)
}

/// An open regular file; its identity was calculated in chunks without loading it into RAM.
pub struct Source {
    file: File,
    id: ContentId,
}
impl Source {
    /// Opens and hashes local content without retaining its potentially unsafe display name.
    ///
    /// # Errors
    /// Returns [`ContentError`] for oversized/empty content, I/O failure or timeout.
    pub async fn open(path: &Path, kind: ContentKind) -> Result<Self, ContentError> {
        let deadline = Instant::now() + TRANSFER_WINDOW;
        let mut file = timeout_at(end(deadline), File::open(path))
            .await
            .map_err(|_| ContentError::Timeout)??;
        let id = hash(&mut file, kind, deadline).await?;
        timeout_at(end(deadline), file.rewind())
            .await
            .map_err(|_| ContentError::Timeout)??;
        Ok(Self { file, id })
    }
    /// The identity to put in a host-selected activity.
    #[must_use]
    pub const fn id(&self) -> ContentId {
        self.id
    }
}

/// Complete, hash-checked bytes that have not yet passed format validation.
pub struct Received {
    path: PathBuf,
    id: ContentId,
}
impl Drop for Received {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
impl Received {
    /// Local temporary file for validators; never display a peer-supplied filename here.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Its verified identity.
    #[must_use]
    pub const fn id(&self) -> ContentId {
        self.id
    }
    /// Applies the application's bounded course/workout validator off the async worker.
    ///
    /// # Errors
    /// Returns [`ContentError::Validation`] if the validator rejects or its worker fails.
    pub async fn validate(
        self,
        validator: impl FnOnce(&Path) -> bool + Send + 'static,
    ) -> Result<Validated, ContentError> {
        // Move the guard into the blocking task: cancelling the await cannot unlink a file
        // while its validator is reading it, or leave a completed result orphaned.
        tokio::task::spawn_blocking(move || {
            if !validator(&self.path) {
                return Err(ContentError::Validation);
            }
            Ok(Validated(self))
        })
        .await
        .map_err(|_| ContentError::Validation)?
    }
}

/// Hash-derived names publication tries in turn, the first without a number.
const NAMES: usize = 8;

fn names(id: ContentId) -> impl Iterator<Item = String> {
    let first = id.basename();
    (1..=NAMES).map(move |number| match first.split_once('.') {
        Some((hash, extension)) if number > 1 => format!("{hash}-{number}.{extension}"),
        _ => first.clone(),
    })
}

/// Format-validated content eligible for atomic publication.
pub struct Validated(Received);
impl Validated {
    /// Retains content under a hash-derived name without replacing any existing file.
    /// A file of that name with other bytes, such as a course the rider has renamed or ridden
    /// with map detail since receiving it, keeps its name and contents: this content takes the
    /// next free `-2`, `-3`, … name instead (#435). The staging directory must be on the
    /// library's file system. Android/Linux storage that rejects hard links uses an atomic
    /// no-replace rename with the same publication guarantees.
    ///
    /// # Errors
    /// Returns [`ContentError`] for I/O failure, or [`ContentError::Mismatch`] once every name
    /// holds different bytes.
    pub async fn publish(self, library: &Path) -> Result<PathBuf, ContentError> {
        let library = library.to_owned();
        let deadline = Instant::now() + TRANSFER_WINDOW;
        // Own the guard through disk work, including if the caller cancels. All read
        // handles close before cleanup attempts to remove its path on Windows.
        timeout_at(
            deadline,
            tokio::task::spawn_blocking(move || {
                verify(&self.0.path, self.0.id, deadline)?;
                for name in names(self.0.id) {
                    if Instant::now() >= deadline {
                        return Err(ContentError::Timeout);
                    }
                    let destination = library.join(name);
                    match publication::retain(&self.0.path, &destination) {
                        Ok(()) => return Ok(destination),
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                            match verify(&destination, self.0.id, deadline) {
                                Ok(()) => return Ok(destination),
                                Err(ContentError::Mismatch) => {}
                                Err(error) => return Err(error),
                            }
                        }
                        Err(error) => {
                            return Err(io::Error::new(
                                error.kind(),
                                format!(
                                    "publishing verified activity to {}: {error}",
                                    destination.display()
                                ),
                            )
                            .into());
                        }
                    }
                }
                Err(ContentError::Mismatch)
            }),
        )
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Validation)?
    }
}

fn verify(path: &Path, id: ContentId, deadline: Instant) -> Result<(), ContentError> {
    use std::io::Read as _;
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Err(ContentError::Mismatch);
    }
    let mut file = std::fs::File::open(path)?;
    if file.metadata()?.len() != id.bytes() {
        return Err(ContentError::Mismatch);
    }
    let mut hash = sha256_provider().start();
    let mut chunk = [0; CHUNK];
    let mut total = 0;
    loop {
        if Instant::now() >= deadline {
            return Err(ContentError::Timeout);
        }
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > id.bytes() {
            return Err(ContentError::Mismatch);
        }
        hash.update(&chunk[..count]);
    }
    if total != id.bytes() || hash.finish().as_ref() != id.hash() {
        return Err(ContentError::Mismatch);
    }
    Ok(())
}

// Field order closes the handle before deleting its path, including on Windows. A disk
// operation owns this entire guard, so cancelling its await cannot unlink an open file.
struct Writer {
    file: std::fs::File,
    received: Received,
}
impl Writer {
    async fn write(
        mut self,
        chunk: Vec<u8>,
        count: usize,
        deadline: Instant,
    ) -> Result<(Self, Vec<u8>), ContentError> {
        use std::io::Write as _;
        timeout_at(
            end(deadline),
            tokio::task::spawn_blocking(move || {
                self.file.write_all(&chunk[..count])?;
                Ok((self, chunk))
            }),
        )
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Validation)?
    }
    async fn finish(self, deadline: Instant) -> Result<Received, ContentError> {
        timeout_at(
            end(deadline),
            tokio::task::spawn_blocking(move || {
                self.file.sync_all()?;
                drop(self.file);
                Ok(self.received)
            }),
        )
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Validation)?
    }
}

async fn temporary(
    directory: &Path,
    id: ContentId,
    deadline: Instant,
) -> Result<Writer, ContentError> {
    // Create the guard in the same blocking task as the file. If cancellation races creation,
    // dropping that task's result still removes the newly created temporary file.
    let directory = directory.to_owned();
    timeout_at(
        end(deadline),
        tokio::task::spawn_blocking(move || {
            let token = random::<16>().map_err(|_| ContentError::Validation)?;
            let mut suffix = String::with_capacity(32);
            for byte in token {
                let _ = write!(suffix, "{byte:02x}");
            }
            let path = directory.join(format!(".orqwa-{suffix}.part"));
            let file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;
            Ok(Writer {
                file,
                received: Received { path, id },
            })
        }),
    )
    .await
    .map_err(|_| ContentError::Timeout)?
    .map_err(|_| ContentError::Validation)?
}

/// Receives the requested content, then requires a clean stream end. A workout must be
/// exactly the requested one. A course may come as the sender's own copy of it, renamed or
/// with another map picture (#435): other bytes, which must match the identity the stream
/// announces and [`Received::id`] reports, and whose course key the validator must check.
/// The temporary directory is local configuration and must already exist.
///
/// # Errors
/// Returns [`ContentError`] for invalid framing, length/hash mismatch, timeout or storage failure.
pub async fn receive(
    recv: &mut RecvStream,
    expected: ContentId,
    directory: &Path,
    progress: Option<&watch::Sender<Progress>>,
) -> Result<Received, ContentError> {
    let deadline = Instant::now() + TRANSFER_WINDOW;
    let mut bytes = [0; HEADER];
    timeout_at(end(deadline), recv.read_exact(&mut bytes))
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Interrupted)?;
    let id = identity(&bytes)?;
    if id != expected && (id.kind(), expected.kind()) != (ContentKind::Course, ContentKind::Course)
    {
        return Err(ContentError::Mismatch);
    }
    let mut writer = temporary(directory, id, deadline).await?;
    let mut hasher = sha256_provider().start();
    let mut chunk = vec![0; CHUNK];
    let mut total = 0;
    if let Some(progress) = progress {
        progress.send_replace(Progress {
            received: 0,
            total: id.bytes(),
        });
    }
    while total < id.bytes() {
        let count = usize::try_from((id.bytes() - total).min(CHUNK as u64))
            .map_err(|_| ContentError::Mismatch)?;
        timeout_at(end(deadline), recv.read_exact(&mut chunk[..count]))
            .await
            .map_err(|_| ContentError::Timeout)?
            .map_err(|_| ContentError::Interrupted)?;
        hasher.update(&chunk[..count]);
        (writer, chunk) = writer.write(chunk, count, deadline).await?;
        total += count as u64;
        if let Some(progress) = progress {
            progress.send_replace(Progress {
                received: total,
                total: id.bytes(),
            });
        }
    }
    let mut extra = [0];
    if timeout_at(end(deadline), recv.read(&mut extra))
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Interrupted)?
        .is_some()
    {
        return Err(ContentError::Mismatch);
    }
    if hasher.finish().as_ref() != id.hash() {
        return Err(ContentError::Mismatch);
    }
    writer.finish(deadline).await
}

/// Sends one previously identified source in bounded chunks; never buffers a complete course.
///
/// # Errors
/// Returns [`ContentError`] for a changed source, interrupted connection, timeout or I/O error.
pub async fn send(send: &mut SendStream, source: &mut Source) -> Result<(), ContentError> {
    let deadline = Instant::now() + TRANSFER_WINDOW;
    timeout_at(end(deadline), source.file.rewind())
        .await
        .map_err(|_| ContentError::Timeout)??;
    timeout_at(end(deadline), send.write_all(&header(source.id)))
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Interrupted)?;
    let mut remaining = source.id.bytes();
    let mut chunk = vec![0; CHUNK];
    while remaining > 0 {
        let count =
            usize::try_from(remaining.min(CHUNK as u64)).map_err(|_| ContentError::Mismatch)?;
        timeout_at(end(deadline), source.file.read_exact(&mut chunk[..count]))
            .await
            .map_err(|_| ContentError::Timeout)??;
        timeout_at(end(deadline), send.write_all(&chunk[..count]))
            .await
            .map_err(|_| ContentError::Timeout)?
            .map_err(|_| ContentError::Interrupted)?;
        remaining -= count as u64;
    }
    send.finish().map_err(|_| ContentError::Interrupted)
}

/// Opens a content request on an existing pinned connection, without discovery or extra sockets.
/// Caller limits simultaneous requests to one per peer and owns cancellation of the future.
/// A course may arrive as the sender's own copy; see [`receive`].
///
/// # Errors
/// Returns [`ContentError`] when opening, requesting, verifying or storing fails.
pub async fn fetch(
    connection: &Connection,
    id: ContentId,
    directory: &Path,
    progress: Option<&watch::Sender<Progress>>,
) -> Result<Received, ContentError> {
    let (mut send, mut recv) = timeout_at(Instant::now() + IDLE_WINDOW, connection.open_bi())
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Interrupted)?;
    timeout_at(Instant::now() + IDLE_WINDOW, send.write_all(&header(id)))
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Interrupted)?;
    send.finish().map_err(|_| ContentError::Interrupted)?;
    receive(&mut recv, id, directory, progress).await
}

/// Serves only a currently selected source on a stream accepted by the owning lobby actor.
/// Arbitrary paths, filenames and unselected hashes are never honored.
///
/// # Errors
/// Returns [`ContentError`] for malformed/unselected requests, excess request bytes or transfer failure.
pub async fn serve(
    send: &mut SendStream,
    recv: &mut RecvStream,
    source: &mut Source,
) -> Result<(), ContentError> {
    if requested(recv).await? != source.id {
        return Err(ContentError::Mismatch);
    }
    self::send(send, source).await
}

/// Reads a bounded request naming content, never a local path.
///
/// # Errors
/// Returns [`ContentError`] for invalid framing, excess bytes or timeout.
pub async fn requested(recv: &mut RecvStream) -> Result<ContentId, ContentError> {
    let deadline = Instant::now() + IDLE_WINDOW;
    let mut bytes = [0; HEADER];
    timeout_at(deadline, recv.read_exact(&mut bytes))
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Interrupted)?;
    let id = identity(&bytes)?;
    let mut extra = [0];
    if timeout_at(deadline, recv.read(&mut extra))
        .await
        .map_err(|_| ContentError::Timeout)?
        .map_err(|_| ContentError::Interrupted)?
        .is_some()
    {
        return Err(ContentError::Mismatch);
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::RouteHash;
    use crate::discovery::GatherOptions;
    use crate::link::{ConnectOptions, Host, HostEvent, Joiner};
    use crate::policy::AddressPolicy;

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let token = random::<8>().unwrap();
            let path = std::env::temp_dir()
                .join(format!("orqwa-transfer-{}-{token:?}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn empty(&self) -> bool {
            std::fs::read_dir(&self.0).unwrap().next().is_none()
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    async fn pair() -> (Host, Connection, Connection) {
        let options = ConnectOptions {
            gather: GatherOptions {
                policy: AddressPolicy {
                    lan: false,
                    loopback: true,
                },
                stun_servers: vec![],
                ..GatherOptions::default()
            },
            window: Duration::from_secs(10),
        };
        let mut host = Host::open("Anna", "Test", RouteHash([7; 32]), &options)
            .await
            .unwrap();
        let joiner = Joiner::open(host.invite().clone(), RouteHash([7; 32]), &options)
            .await
            .unwrap();
        host.admit(joiner.reply()).unwrap();
        let client = joiner.connect().await.unwrap();
        let HostEvent::Joined(peer) = host.next().await.unwrap() else {
            panic!("admission expired");
        };
        (host, client, peer)
    }
    #[tokio::test]
    async fn encrypted_transfers_keep_one_progress_value_and_publish_only_validated_content() {
        let (_host, client, peer) = pair().await;
        let from = Directory::new();
        let to = Directory::new();
        let bytes = vec![42; 256 * 1024];
        let source_path = from.0.join("untrusted display name.tqc");
        std::fs::write(&source_path, &bytes).unwrap();
        let mut source = Source::open(&source_path, ContentKind::Course)
            .await
            .unwrap();
        let id = source.id();
        let (progress, latest) = watch::channel(Progress {
            received: 0,
            total: id.bytes(),
        });
        let download = fetch(&client, id, &to.0, Some(&progress));
        let upload = async {
            let (mut send, mut recv) = peer.accept_bi().await.unwrap();
            serve(&mut send, &mut recv, &mut source).await.unwrap();
        };
        let (received, ()) = tokio::join!(download, upload);
        let received = received.unwrap();
        assert_eq!(
            *latest.borrow(),
            Progress {
                received: id.bytes(),
                total: id.bytes()
            }
        );
        assert_eq!(std::fs::read(received.path()).unwrap(), bytes);
        assert!(!to.0.join(id.basename()).exists());
        let published = received
            .validate(|_| true)
            .await
            .unwrap()
            .publish(&to.0)
            .await
            .unwrap();
        assert_eq!(published, to.0.join(id.basename()));
        assert_eq!(std::fs::read(published).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(&to.0).unwrap().count(), 1);
        // Content transport leaves the existing datagram channel usable.
        client.send_datagram(&[123]).unwrap();
        assert_eq!(peer.read_datagram().await.unwrap(), vec![123]);
    }
    async fn raw(bytes: Vec<u8>, id: ContentId, to: &Path) -> Result<Received, ContentError> {
        let (_host, client, peer) = pair().await;
        let upload = async {
            let (mut send, _) = client.open_bi().await.unwrap();
            send.write_all(&bytes).await.unwrap();
            send.finish().unwrap();
            // Wait for the receiver to finish before dropping its sending connection.
            send.stopped().await.ok();
        };
        let download = async {
            let (_, mut recv) = peer.accept_bi().await.unwrap();
            receive(&mut recv, id, to, None).await
        };
        let ((), result) = tokio::join!(upload, download);
        result
    }
    #[tokio::test]
    async fn changed_truncated_extra_and_wrong_identity_bytes_never_reach_the_library() {
        let id = ContentId::of(ContentKind::Workout, b"workout").unwrap();
        for body in [b"corrupt".as_slice(), b"short", b"workout plus more"] {
            let to = Directory::new();
            let mut bytes = header(id);
            bytes.extend_from_slice(body);
            assert!(raw(bytes, id, &to.0).await.is_err());
            assert!(to.empty());
        }
        let to = Directory::new();
        let other = ContentId::of(ContentKind::Workout, b"other").unwrap();
        assert!(raw(header(other), id, &to.0).await.is_err());
        assert!(to.empty());
        let mut bytes = header(id);
        bytes.extend_from_slice(b"workout");
        let received = raw(bytes, id, &to.0).await.unwrap();
        assert!(received.validate(|_| false).await.is_err());
        assert!(to.empty());
    }
    #[tokio::test]
    async fn a_course_may_arrive_as_the_senders_own_copy_but_only_as_the_bytes_it_names() {
        let selected = ContentId::of(ContentKind::Course, b"selected course").unwrap();
        let own = ContentId::of(ContentKind::Course, b"renamed course").unwrap();
        let to = Directory::new();
        let mut bytes = header(own);
        bytes.extend_from_slice(b"renamed course");
        let received = raw(bytes, selected, &to.0).await.unwrap();
        assert_eq!(received.id(), own);
        assert_eq!(std::fs::read(received.path()).unwrap(), b"renamed course");
        drop(received);
        // Bytes other than the copy it announces, or a workout, still never arrive.
        let mut changed = header(own);
        changed.extend_from_slice(b"another course");
        assert!(raw(changed, selected, &to.0).await.is_err());
        let workout = ContentId::of(ContentKind::Workout, b"workout").unwrap();
        let mut other = header(workout);
        other.extend_from_slice(b"workout");
        assert!(raw(other, selected, &to.0).await.is_err());
        assert!(to.empty());
    }
    async fn staged(to: &Path) -> (ContentId, Validated) {
        let id = ContentId::of(ContentKind::Workout, b"workout").unwrap();
        let mut bytes = header(id);
        bytes.extend_from_slice(b"workout");
        let received = raw(bytes, id, to).await.unwrap();
        (id, received.validate(|_| true).await.unwrap())
    }
    #[tokio::test]
    async fn publication_reuses_identical_content_and_never_overwrites_changed_files() {
        let to = Directory::new();
        let (id, validated) = staged(&to.0).await;
        let destination = to.0.join(id.basename());
        std::fs::write(&destination, b"workout").unwrap();
        assert_eq!(validated.publish(&to.0).await.unwrap(), destination);
        assert_eq!(std::fs::read_dir(&to.0).unwrap().count(), 1);

        // A file the rider changed since it arrived keeps its name and bytes (#435).
        let to = Directory::new();
        let (id, validated) = staged(&to.0).await;
        let changed = to.0.join(id.basename());
        std::fs::write(&changed, b"different").unwrap();
        let published = validated.publish(&to.0).await.unwrap();
        let hash = id.basename().trim_end_matches(".oqw").to_owned();
        assert_eq!(published, to.0.join(format!("{hash}-2.oqw")));
        assert_eq!(std::fs::read(published).unwrap(), b"workout");
        assert_eq!(std::fs::read(changed).unwrap(), b"different");
        assert_eq!(std::fs::read_dir(&to.0).unwrap().count(), 2);

        // Once every name holds other bytes, nothing is replaced and publication fails.
        let to = Directory::new();
        let (_, validated) = staged(&to.0).await;
        for name in names(id) {
            std::fs::write(to.0.join(name), b"different").unwrap();
        }
        assert!(matches!(
            validated.publish(&to.0).await,
            Err(ContentError::Mismatch)
        ));
        assert_eq!(std::fs::read_dir(&to.0).unwrap().count(), NAMES);
        for name in names(id) {
            assert_eq!(std::fs::read(to.0.join(name)).unwrap(), b"different");
        }
    }
    #[tokio::test]
    async fn cancelling_a_partial_download_removes_its_unique_temporary_file() {
        let (_host, client, peer) = pair().await;
        let to = Directory::new();
        let id = ContentId::of(ContentKind::Workout, b"workout").unwrap();
        let directory = to.0.clone();
        let task = tokio::spawn(async move {
            let (_, mut recv) = peer.accept_bi().await.unwrap();
            receive(&mut recv, id, &directory, None).await
        });
        let (mut send, _) = client.open_bi().await.unwrap();
        send.write_all(&header(id)).await.unwrap();
        send.write_all(b"w").await.unwrap();
        timeout_at(Instant::now() + Duration::from_secs(2), async {
            while to.empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        let _ = task.await;
        assert!(to.empty());
    }

    #[tokio::test]
    async fn stalled_downloads_obey_the_idle_deadline_and_clean_up() {
        let (_host, client, peer) = pair().await;
        let to = Directory::new();
        let id = ContentId::of(ContentKind::Workout, b"workout").unwrap();
        let directory = to.0.clone();
        let task = tokio::spawn(async move {
            let (_, mut recv) = peer.accept_bi().await.unwrap();
            receive(&mut recv, id, &directory, None).await
        });
        let (mut send, _) = client.open_bi().await.unwrap();
        send.write_all(&header(id)).await.unwrap();
        send.write_all(b"w").await.unwrap();
        timeout_at(Instant::now() + Duration::from_secs(2), async {
            while to.empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::pause();
        tokio::time::advance(IDLE_WINDOW + Duration::from_secs(1)).await;
        let result = task.await.unwrap();
        assert!(matches!(
            result,
            Err(ContentError::Timeout | ContentError::Interrupted)
        ));
        assert!(to.empty());
    }

    #[tokio::test]
    async fn a_validator_cannot_publish_mutated_staging_bytes() {
        let to = Directory::new();
        let id = ContentId::of(ContentKind::Workout, b"workout").unwrap();
        let mut bytes = header(id);
        bytes.extend_from_slice(b"workout");
        let received = raw(bytes, id, &to.0).await.unwrap();
        let validated = received
            .validate(|path| std::fs::write(path, b"changed").is_ok())
            .await
            .unwrap();
        assert!(matches!(
            validated.publish(&to.0).await,
            Err(ContentError::Mismatch)
        ));
        assert!(to.empty());
    }
}
