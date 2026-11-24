//! The maintainers' tools for world files (ADR 0043): keys, certificates, withdrawals, sealing
//! and checking. Private keys are only ever written encrypted with `ORQWA_KEY_PASSWORD`.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use clap::{Args, ValueEnum};
use orqwa_licence::keys::{Context, KeyPair, PublicKey};
use orqwa_licence::stream::ContentKey;
use orqwa_licence::trust::{self, Purpose, Revocations, Signed, Trust};
use orqwa_storage::world::{self, Sealer, WorldFile};

/// What a working key is for.
#[derive(Clone, Copy, ValueEnum)]
pub enum KeyPurpose {
    /// Seals world files.
    World,
    /// Signs activations on the activation server.
    Activation,
}

/// Seal a world folder into a signed (and, for sold worlds, encrypted) world file.
#[derive(Args)]
pub struct SealArgs {
    /// The folder holding `world.json` and the parts it names.
    folder: PathBuf,
    /// The world file to write.
    out: PathBuf,
    /// The world key, as `key-new` wrote it.
    #[arg(long, required_unless_present = "dev")]
    key: Option<PathBuf>,
    /// A root's certificate for the world key, as `key-certify` wrote it.
    #[arg(long, required_unless_present = "dev")]
    certificate: Option<PathBuf>,
    /// Encrypt every part that may be encrypted with this content key (`content-key-new`).
    #[arg(long)]
    content_key: Option<PathBuf>,
    /// Seal with the development world key, which only development builds trust.
    #[arg(long, conflicts_with_all = ["key", "certificate"])]
    dev: bool,
    /// Also trust this root (`*.pub.json`), for roots not yet built into ORQWA.
    #[arg(long)]
    trust: Option<PathBuf>,
}

fn password() -> Result<String> {
    std::env::var("ORQWA_KEY_PASSWORD")
        .ok()
        .filter(|p| p.len() >= 12)
        .context("set ORQWA_KEY_PASSWORD to the key's password (at least 12 characters)")
}

fn read_key(path: &Path) -> Result<KeyPair> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    KeyPair::unprotect(&text, &password()?)
        .with_context(|| format!("cannot open {}", path.display()))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("{} is not what was expected", path.display()))
}

fn write_new(path: &Path, text: &str) -> Result<()> {
    if path.exists() {
        bail!(
            "{} exists already; keys and certificates are never overwritten",
            path.display()
        );
    }
    orqwa_licence::device::write_private(path, text.as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))
}

fn public_path(key: &Path) -> PathBuf {
    let mut name = key.file_name().unwrap_or_default().to_os_string();
    name.push(".pub.json");
    key.with_file_name(name)
}

/// The trust ORQWA builds in, plus `extra` if given.
fn trust(extra: Option<&Path>) -> Result<Trust> {
    let builtin = Trust::builtin();
    let Some(extra) = extra else {
        return Ok(builtin);
    };
    let root: PublicKey = read_json(extra)?;
    #[allow(unused_mut)] // without dev-keys, nothing is added
    let mut roots = vec![root];
    // Keep the built-in roots too: a world sealed for release must open with them alone.
    #[cfg(feature = "dev-keys")]
    roots.push(orqwa_licence::dev::root().public().clone());
    Ok(Trust::new(roots, builtin.revocations().clone()))
}

/// Makes a new key, written encrypted to `out`, its public half to `out.pub.json`.
pub fn key_new(out: &Path) -> Result<()> {
    let password = password()?;
    let key = KeyPair::generate()?;
    write_new(out, &key.protect(&password)?)?;
    let public = serde_json::to_string_pretty(key.public())?;
    write_new(&public_path(out), &public)?;
    println!("key {} written to {}", key.public().id(), out.display());
    println!("public key in {}", public_path(out).display());
    Ok(())
}

/// A root certifies a working key.
pub fn key_certify(
    root: &Path,
    public: &Path,
    purpose: KeyPurpose,
    name: &str,
    out: &Path,
) -> Result<()> {
    let root = read_key(root)?;
    let public: PublicKey = read_json(public)?;
    let purpose = match purpose {
        KeyPurpose::World => Purpose::World,
        KeyPurpose::Activation => Purpose::Activation,
    };
    let certificate = trust::certify(&root, &public, purpose, name)?;
    write_new(out, &serde_json::to_string_pretty(&certificate)?)?;
    println!(
        "{} certified by root {} as {name}",
        public.id(),
        root.public().id()
    );
    Ok(())
}

/// A root signs a list of withdrawals.
pub fn revocation_sign(root: &Path, list: &Path, out: &Path) -> Result<()> {
    let root = read_key(root)?;
    let list: Revocations = read_json(list)?;
    let signed = Signed::sign(&list, Context::Revocations, &root)?;
    write_new(out, &serde_json::to_string_pretty(&signed)?)?;
    println!(
        "withdrawals {} signed by root {}",
        list.sequence,
        root.public().id()
    );
    Ok(())
}

/// Makes a world's content key.
pub fn content_key_new(id: &str, out: &Path) -> Result<()> {
    write_new(out, &ContentKey::generate(id)?.to_json()?)?;
    println!("content key {id} written to {}", out.display());
    Ok(())
}

/// Seals a world folder, then checks the file as importing would.
pub fn seal(args: &SealArgs) -> Result<()> {
    let (key, certificate) = if args.dev {
        dev_key()?
    } else {
        let key = read_key(args.key.as_deref().context("--key")?)?;
        let certificate: Signed = read_json(args.certificate.as_deref().context("--certificate")?)?;
        (key, certificate)
    };
    let content_key = args
        .content_key
        .as_deref()
        .map(|path| -> Result<ContentKey> {
            Ok(ContentKey::from_json(&std::fs::read_to_string(path)?)?)
        })
        .transpose()?;
    let sealer = Sealer {
        key: &key,
        certificate,
        content_key: content_key.as_ref(),
    };
    let trust = trust(args.trust.as_deref())?;
    world::seal(&args.folder, &args.out, &sealer, &trust)
        .with_context(|| format!("cannot seal {}", args.folder.display()))?;
    check(&args.out, args.trust.as_deref())
}

#[cfg(feature = "dev-keys")]
#[allow(clippy::unnecessary_wraps)] // the same shape as without dev keys
fn dev_key() -> Result<(KeyPair, Signed)> {
    Ok((
        orqwa_licence::dev::world_key(),
        orqwa_licence::dev::world_certificate(),
    ))
}

#[cfg(not(feature = "dev-keys"))]
fn dev_key() -> Result<(KeyPair, Signed)> {
    bail!("--dev needs orqwa-cli built with --features dev-keys")
}

/// Checks a world file as importing it does, and shows what it holds and who signed it.
pub fn check(file: &Path, extra_root: Option<&Path>) -> Result<()> {
    let started = std::time::Instant::now();
    let trust = trust(extra_root)?;
    let world = world::validate(file, &trust)
        .with_context(|| format!("{} is not a valid world file", file.display()))?;
    let opened = WorldFile::open(file, &trust)?;
    println!(
        "{} {} — {}",
        world.name,
        world.version.as_deref().unwrap_or(""),
        world.licence
    );
    let (signer, signer_name) = opened.signer();
    println!("signed by {signer_name} ({signer})");
    match opened.content_key() {
        Some(key) => println!("encrypted with content key {key}"),
        None => println!("not encrypted"),
    }
    println!(
        "{} sections, {} assets, horizon: {}",
        world.sections.len(),
        world.assets.len(),
        if world.horizon.is_some() { "yes" } else { "no" }
    );
    for route in &world.routes {
        println!("route {}: {}", route.id, route.name);
    }
    for credit in &world.attribution {
        println!("credit: {credit}");
    }
    println!("valid, checked in {:.1} s", started.elapsed().as_secs_f64());
    Ok(())
}
