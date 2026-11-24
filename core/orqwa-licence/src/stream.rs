//! Entries encrypted with AES-256-GCM in chunks of 64 KiB of plain text, so a part streams
//! through checks and a video can seek. An 8-byte random prefix comes first; each chunk is
//! sealed with the prefix and its number as nonce and, as associated data, whether it is the
//! last. Chunks cannot be moved, repeated or dropped, and an entry cannot be cut short. Each
//! entry has its own key, derived from the world's content key and the entry's name.

use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use serde::{Deserialize, Serialize};

use crate::keys::Sealed;
use crate::{LicenceError, derive, random};

/// Plain text per chunk.
pub const CHUNK: usize = 64 * 1024;
const TAG: usize = 16;
const PREFIX: usize = 8;
const SEALED_CHUNK: u64 = (CHUNK + TAG) as u64;

/// A world's content key: the secret its encrypted entries are derived from.
#[derive(Clone, PartialEq, Eq)]
pub struct ContentKey {
    id: String,
    key: [u8; 32],
}

impl std::fmt::Debug for ContentKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentKey")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentKeyFile {
    format: String,
    id: String,
    secret: Sealed,
}

const CONTENT_KEY_FILE: &str = "orqwa-content-key-1";

impl ContentKey {
    /// A new key named `id` (such as "lake-biel-2026-11").
    ///
    /// # Errors
    /// [`LicenceError::Random`] if no secure random bytes are available.
    pub fn generate(id: &str) -> Result<Self, LicenceError> {
        Ok(Self {
            id: id.to_owned(),
            key: random()?,
        })
    }

    /// The key from its id and 32 secret bytes.
    #[must_use]
    pub fn new(id: &str, key: [u8; 32]) -> Self {
        Self {
            id: id.to_owned(),
            key,
        }
    }

    /// Its id, which the manifest names.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn secret(&self) -> &[u8; 32] {
        &self.key
    }

    /// The key as a JSON file encrypted with `password`, as private keys are: a leaked content
    /// key opens its world for everyone.
    ///
    /// # Errors
    /// [`LicenceError::Random`] if no secure random bytes are available.
    pub fn protect(&self, password: &str) -> Result<String, LicenceError> {
        let file = ContentKeyFile {
            format: CONTENT_KEY_FILE.to_owned(),
            id: self.id.clone(),
            secret: Sealed::seal(&self.key, password, CONTENT_KEY_FILE)?,
        };
        Ok(serde_json::to_string_pretty(&file)?)
    }

    /// A key [`ContentKey::protect`] wrote.
    ///
    /// # Errors
    /// [`LicenceError::Password`] for a wrong password or a changed file.
    pub fn unprotect(text: &str, password: &str) -> Result<Self, LicenceError> {
        let file: ContentKeyFile = serde_json::from_str(text)?;
        if file.format != CONTENT_KEY_FILE {
            return Err(LicenceError::Invalid(
                "not an ORQWA content key file".into(),
            ));
        }
        let key = file
            .secret
            .open(password, CONTENT_KEY_FILE)?
            .try_into()
            .map_err(|_| LicenceError::Password)?;
        Ok(Self { id: file.id, key })
    }

    fn entry_key(&self, entry: &str) -> LessSafeKey {
        let key: [u8; 32] = derive(b"ORQWA orqwa-1 entry", &self.key, &[entry.as_bytes()]);
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key).expect("32 bytes for AES-256"))
    }
}

fn nonce(prefix: [u8; PREFIX], chunk: u32) -> Nonce {
    let mut nonce = [0; 12];
    nonce[..PREFIX].copy_from_slice(&prefix);
    nonce[PREFIX..].copy_from_slice(&chunk.to_be_bytes());
    Nonce::assume_unique_for_key(nonce)
}

fn chunk_count(plain: u64) -> u64 {
    plain.div_ceil(CHUNK as u64).max(1)
}

/// The plain size of an entry of `sealed` bytes, if that size can be one.
#[must_use]
pub fn plain_len(sealed: u64) -> Option<u64> {
    let body = sealed.checked_sub(PREFIX as u64)?;
    let chunks = body.div_ceil(SEALED_CHUNK).max(1);
    let plain = body.checked_sub(chunks * TAG as u64)?;
    (u32::try_from(chunks).is_ok() && chunk_count(plain) == chunks).then_some(plain)
}

/// Encrypts `plain` as entry `entry` of a world sealed with `key`.
///
/// # Errors
/// [`LicenceError::Random`] if no secure random bytes are available, or
/// [`LicenceError::Invalid`] for an entry beyond 2³² chunks.
pub fn encrypt(key: &ContentKey, entry: &str, plain: &[u8]) -> Result<Vec<u8>, LicenceError> {
    let mut encryptor = Encryptor::new(Vec::with_capacity(plain.len() + PREFIX + TAG), key, entry)?;
    encryptor.write_all(plain)?;
    Ok(encryptor.finish()?)
}

/// Encrypts an entry as it is written, a chunk at a time, into `out`; [`Encryptor::finish`]
/// seals the last chunk. Writes the same bytes as [`encrypt`].
pub struct Encryptor<W: Write> {
    out: W,
    key: LessSafeKey,
    prefix: [u8; PREFIX],
    pending: Vec<u8>,
    number: u32,
}

impl<W: Write> Encryptor<W> {
    /// Starts entry `entry`, writing its random prefix.
    ///
    /// # Errors
    /// [`LicenceError::Random`] if no secure random bytes are available, or a write error.
    pub fn new(mut out: W, key: &ContentKey, entry: &str) -> Result<Self, LicenceError> {
        let prefix: [u8; PREFIX] = random()?;
        out.write_all(&prefix)?;
        Ok(Self {
            out,
            key: key.entry_key(entry),
            prefix,
            pending: Vec::with_capacity(2 * CHUNK),
            number: 0,
        })
    }

    fn seal(&mut self, len: usize, last: bool) -> io::Result<()> {
        let mut chunk: Vec<u8> = self.pending.drain(..len).collect();
        self.key
            .seal_in_place_append_tag(
                nonce(self.prefix, self.number),
                Aad::from([u8::from(last)]),
                &mut chunk,
            )
            .map_err(|_| io::Error::other("could not encrypt"))?;
        self.out.write_all(&chunk)?;
        self.number = self
            .number
            .checked_add(1)
            .ok_or_else(|| io::Error::other("too large to encrypt"))?;
        Ok(())
    }

    /// Seals the last chunk (empty for an empty entry) and hands back the output.
    ///
    /// # Errors
    /// A write error.
    pub fn finish(mut self) -> io::Result<W> {
        let len = self.pending.len();
        self.seal(len, true)?;
        Ok(self.out)
    }
}

impl<W: Write> Write for Encryptor<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        // A full chunk is kept back until more follows: only finish knows the last one.
        while self.pending.len() > CHUNK {
            self.seal(CHUNK, false)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// Decrypts a whole entry.
///
/// # Errors
/// [`LicenceError::Decrypt`] if it is damaged, cut short or sealed with another key or name.
pub fn decrypt(key: &ContentKey, entry: &str, sealed: &[u8]) -> Result<Vec<u8>, LicenceError> {
    let mut reader = Decryptor::new(Cursor::new(sealed), key, entry, sealed.len() as u64)
        .map_err(|_| LicenceError::Decrypt)?;
    let mut plain = Vec::new();
    reader
        .read_to_end(&mut plain)
        .map_err(|_| LicenceError::Decrypt)?;
    Ok(plain)
}

/// Reads an encrypted entry's plain text, seeking by chunk. Every chunk is authenticated
/// before any of its bytes are handed out.
pub struct Decryptor<R> {
    inner: R,
    key: LessSafeKey,
    prefix: [u8; PREFIX],
    sealed_len: u64,
    plain_len: u64,
    chunks: u64,
    position: u64,
    chunk: Option<(u64, Vec<u8>)>,
}

fn damaged() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, LicenceError::Decrypt)
}

impl<R: Read + Seek> Decryptor<R> {
    /// Reads the entry of `sealed_len` bytes that starts at `inner`'s position 0.
    ///
    /// # Errors
    /// If the size cannot be an encrypted entry's, or reading fails.
    pub fn new(mut inner: R, key: &ContentKey, entry: &str, sealed_len: u64) -> io::Result<Self> {
        let plain_len = plain_len(sealed_len).ok_or_else(damaged)?;
        let mut prefix = [0; PREFIX];
        inner.seek(SeekFrom::Start(0))?;
        inner.read_exact(&mut prefix)?;
        let mut decryptor = Self {
            inner,
            key: key.entry_key(entry),
            prefix,
            sealed_len,
            plain_len,
            chunks: chunk_count(plain_len),
            position: 0,
            chunk: None,
        };
        // An empty entry is one empty chunk, which no read would open: open it here, so a
        // wrong key or a forged entry fails for it too.
        if plain_len == 0 {
            decryptor.load(0)?;
        }
        Ok(decryptor)
    }

    /// The plain text's length.
    #[must_use]
    pub fn plain_len(&self) -> u64 {
        self.plain_len
    }

    fn load(&mut self, index: u64) -> io::Result<()> {
        if self.chunk.as_ref().is_some_and(|(i, _)| *i == index) {
            return Ok(());
        }
        let start = PREFIX as u64 + index * SEALED_CHUNK;
        let len = SEALED_CHUNK.min(self.sealed_len - start);
        let mut buffer = vec![0; usize::try_from(len).map_err(|_| damaged())?];
        self.inner.seek(SeekFrom::Start(start))?;
        self.inner.read_exact(&mut buffer)?;
        let number = u32::try_from(index).map_err(|_| damaged())?;
        let last = index + 1 == self.chunks;
        let plain_len = self
            .key
            .open_in_place(
                nonce(self.prefix, number),
                Aad::from([u8::from(last)]),
                &mut buffer,
            )
            .map_err(|_| damaged())?
            .len();
        buffer.truncate(plain_len);
        self.chunk = Some((index, buffer));
        Ok(())
    }
}

impl<R: Read + Seek> Read for Decryptor<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.plain_len || buf.is_empty() {
            return Ok(0);
        }
        let index = self.position / CHUNK as u64;
        self.load(index)?;
        let (_, chunk) = self.chunk.as_ref().expect("loaded above");
        let offset = usize::try_from(self.position % CHUNK as u64).expect("below CHUNK");
        let n = buf.len().min(chunk.len() - offset);
        buf[..n].copy_from_slice(&chunk[offset..offset + n]);
        self.position += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for Decryptor<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(by) => self.plain_len.checked_add_signed(by),
            SeekFrom::Current(by) => self.position.checked_add_signed(by),
        };
        self.position = target
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"))?;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ContentKey {
        ContentKey::new("k", [3; 32])
    }

    fn data(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| u8::try_from(i * 7 % 251).unwrap())
            .collect()
    }

    #[test]
    fn entries_of_any_size_round_trip() {
        for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK + 17] {
            let plain = data(len);
            let sealed = encrypt(&key(), "a.glb", &plain).unwrap();
            assert_eq!(plain_len(sealed.len() as u64), Some(len as u64), "{len}");
            assert_eq!(decrypt(&key(), "a.glb", &sealed).unwrap(), plain, "{len}");
        }
    }

    #[test]
    fn another_key_or_entry_name_cannot_decrypt() {
        let sealed = encrypt(&key(), "a.glb", &data(100)).unwrap();
        assert!(decrypt(&ContentKey::new("k", [4; 32]), "a.glb", &sealed).is_err());
        assert!(decrypt(&key(), "b.glb", &sealed).is_err());
    }

    #[test]
    fn a_changed_cut_or_reordered_entry_is_refused() {
        let sealed = encrypt(&key(), "a", &data(3 * CHUNK)).unwrap();

        let mut flipped = sealed.clone();
        flipped[PREFIX + 5] ^= 1;
        assert!(decrypt(&key(), "a", &flipped).is_err());

        // Cut at a chunk boundary: the last chunk left was not sealed as the last.
        let cut = &sealed[..PREFIX + 2 * (CHUNK + TAG)];
        assert!(decrypt(&key(), "a", cut).is_err());

        let first = PREFIX..PREFIX + CHUNK + TAG;
        let second = PREFIX + CHUNK + TAG..PREFIX + 2 * (CHUNK + TAG);
        let mut swapped = sealed.clone();
        swapped[first.clone()].copy_from_slice(&sealed[second.clone()]);
        swapped[second].copy_from_slice(&sealed[first]);
        assert!(decrypt(&key(), "a", &swapped).is_err());
    }

    #[test]
    fn written_in_any_pieces_it_decrypts_the_same() {
        let plain = data(3 * CHUNK + 5);
        for piece in [1, 4096, CHUNK, CHUNK + 7] {
            let mut encryptor = Encryptor::new(Vec::new(), &key(), "a.glb").unwrap();
            for part in plain.chunks(piece) {
                encryptor.write_all(part).unwrap();
            }
            let sealed = encryptor.finish().unwrap();
            assert_eq!(
                plain_len(sealed.len() as u64),
                Some(plain.len() as u64),
                "{piece}"
            );
            assert_eq!(decrypt(&key(), "a.glb", &sealed).unwrap(), plain, "{piece}");
        }
    }

    #[test]
    fn the_same_plain_text_encrypts_differently_each_time() {
        let plain = data(10);
        assert_ne!(
            encrypt(&key(), "a", &plain).unwrap(),
            encrypt(&key(), "a", &plain).unwrap()
        );
    }

    #[test]
    fn a_decryptor_seeks_anywhere() {
        let plain = data(2 * CHUNK + 999);
        let sealed = encrypt(&key(), "v.mp4", &plain).unwrap();
        let mut reader =
            Decryptor::new(Cursor::new(&sealed), &key(), "v.mp4", sealed.len() as u64).unwrap();
        for at in [
            CHUNK as u64 + 3,
            5,
            2 * CHUNK as u64 + 990,
            CHUNK as u64 - 2,
        ] {
            reader.seek(SeekFrom::Start(at)).unwrap();
            let mut got = [0; 9];
            let n = reader.read(&mut got).unwrap();
            let at = usize::try_from(at).unwrap();
            assert_eq!(&got[..n], &plain[at..at + n]);
            assert!(n > 0);
        }
        assert_eq!(
            reader.seek(SeekFrom::End(-1)).unwrap(),
            plain.len() as u64 - 1
        );
        assert!(
            reader
                .seek(SeekFrom::Current(-i64::try_from(plain.len()).unwrap()))
                .is_err()
        );
    }

    #[test]
    fn a_content_key_file_opens_only_with_its_password() {
        let key = ContentKey::generate("lake").unwrap();
        let file = key.protect("correct horse").unwrap();
        assert_eq!(ContentKey::unprotect(&file, "correct horse").unwrap(), key);
        assert!(matches!(
            ContentKey::unprotect(&file, "wrong"),
            Err(LicenceError::Password)
        ));
        assert!(!format!("{key:?}").contains(&format!("{:?}", key.key)));
        // A world key's file is not a content key's.
        let world_key = crate::keys::KeyPair::generate()
            .unwrap()
            .protect("correct horse")
            .unwrap();
        assert!(ContentKey::unprotect(&world_key, "correct horse").is_err());
    }

    #[test]
    fn an_empty_entry_is_authenticated_too() {
        let sealed = encrypt(&key(), "a", b"").unwrap();
        assert_eq!(decrypt(&key(), "a", &sealed).unwrap(), b"");
        assert!(decrypt(&ContentKey::new("k", [4; 32]), "a", &sealed).is_err());
        assert!(decrypt(&key(), "b", &sealed).is_err());
        assert!(decrypt(&key(), "a", &[0xAA; PREFIX + TAG]).is_err());
    }
}
