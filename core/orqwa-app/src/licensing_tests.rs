//! Activating sold worlds (R293): kept on this device only, nothing kept on a refusal, released
//! only when the server confirms, never beside the data folder.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};

use orqwa_licence::activation::{Request, issue};
use orqwa_licence::dev;
use orqwa_licence::device::DeviceKey;
use orqwa_licence::keys::Context;
use orqwa_licence::trust::Revocations;

use super::*;

const WORLD: &str = "6f1c1d84-6a1e-4d55-9a53-0d9e2f1f7a10";

pub(crate) fn content_key() -> ContentKey {
    ContentKey::new("test-1", [7; 32])
}

/// A fake activation server on localhost answering with `answer` for each request; counts them.
pub(crate) fn server(
    answer: impl Fn(&str, &[u8]) -> (u16, Vec<u8>) + Send + 'static,
) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            counted.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let (status, reply) = answer(&path, &body);
            // A redirect's body names where to: "<url>", sent as its location.
            let location = if status == 307 {
                format!(
                    "location: {}/v1/activate\r\n",
                    String::from_utf8_lossy(&reply)
                )
            } else {
                String::new()
            };
            let head = format!(
                "HTTP/1.1 {status} X\r\n{location}content-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                reply.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(&reply).unwrap();
        }
    });
    (url, count)
}

/// Activates as the activation server does, for whichever device asks.
pub(crate) fn activating(path: &str, body: &[u8]) -> (u16, Vec<u8>) {
    match path {
        "/v1/activate" => {
            let request: ActivateRequest = serde_json::from_slice(body).unwrap();
            let device_key = DeviceKey::from_bytes(request.device_key.clone().try_into().unwrap());
            let activation = issue(
                &Request {
                    licence: "L-1",
                    licensee: "Ada Rider",
                    world: &request.world,
                    device: &request.device,
                    device_key: &device_key,
                },
                &content_key(),
                &dev::activation_key(),
                dev::activation_certificate(),
            )
            .unwrap();
            let answer = ActivateResponse {
                activation,
                revocations: None,
            };
            (200, serde_json::to_vec(&answer).unwrap())
        }
        _ => (200, b"{}".to_vec()),
    }
}

fn dirs(name: &str) -> (PathBuf, PathBuf) {
    let root = crate::tests::temp_dir(&format!("licensing-{name}"));
    (root.join("device"), root.join("data"))
}

fn licensing(device: &Path, data: &Path, server: &str, machine: &[u8]) -> Licensing {
    let licensing = Licensing::with_server(device.to_owned(), data, server, true);
    licensing.set_machine_id(machine.to_vec());
    licensing
}

#[tokio::test]
async fn an_activation_is_kept_on_this_device_and_opens_only_here() {
    let (url, _) = server(activating);
    let (device, data) = dirs("kept");
    let here = licensing(&device, &data, &url, b"machine a");
    assert_eq!(
        here.activate(WORLD, " L-1-KEY ").await.unwrap(),
        "Ada Rider"
    );

    let (activation, key) = here.activation(WORLD).unwrap();
    assert_eq!(key, content_key());
    assert_eq!(activation.licensee, "Ada Rider");
    assert!(
        device
            .join(ACTIVATIONS)
            .join(format!("{WORLD}.json"))
            .exists()
    );
    assert!(
        !data.exists(),
        "nothing goes to the data folder, which may sync"
    );

    // The same folder on another computer opens nothing.
    let copied = licensing(&device, &data, &url, b"machine b");
    assert!(copied.activation(WORLD).is_none());
}

#[tokio::test]
async fn a_refusal_keeps_nothing() {
    let (url, _) = server(|_, _| (409, br#"{"refusal":"device_limit"}"#.to_vec()));
    let (device, data) = dirs("refused");
    let licensing = licensing(&device, &data, &url, b"m");
    assert!(matches!(
        licensing.activate(WORLD, "key").await,
        Err(LicensingError::Refused(Refusal::DeviceLimit))
    ));
    assert!(licensing.activation(WORLD).is_none());
    assert!(!device.join(ACTIVATIONS).exists());
}

#[tokio::test]
async fn an_activation_for_another_device_is_not_kept() {
    let (url, _) = server(|path, body| {
        let mut request: ActivateRequest = serde_json::from_slice(body).unwrap();
        request.device = "someone-else".into();
        activating(path, &serde_json::to_vec(&request).unwrap())
    });
    let (device, data) = dirs("foreign");
    let licensing = licensing(&device, &data, &url, b"m");
    assert!(licensing.activate(WORLD, "key").await.is_err());
    assert!(licensing.activation(WORLD).is_none());
}

#[tokio::test]
async fn releasing_forgets_the_activation_only_once_the_server_confirms() {
    let (url, _) = server(activating);
    let (device, data) = dirs("release");
    let online = licensing(&device, &data, &url, b"m");
    online.activate(WORLD, "key").await.unwrap();

    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let offline_url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let offline = licensing(&device, &data, &offline_url, b"m");
    assert!(matches!(
        offline.release(WORLD).await,
        Err(LicensingError::Unreachable(_))
    ));
    assert!(
        offline.activation(WORLD).is_some(),
        "the slot is not lost unnoticed"
    );

    online.release(WORLD).await.unwrap();
    assert!(online.activation(WORLD).is_none());
}

#[tokio::test]
async fn a_device_folder_inside_the_data_folder_activates_nothing() {
    let (url, requests) = server(activating);
    let (_, data) = dirs("inside");
    let licensing = licensing(&data.join("device"), &data, &url, b"m");
    assert!(matches!(
        licensing.activate(WORLD, "key").await,
        Err(LicensingError::InDataFolder(_))
    ));
    assert_eq!(requests.load(Ordering::SeqCst), 0, "nothing was sent");
}

#[tokio::test]
async fn withdrawals_from_the_server_are_kept() {
    let list = Revocations {
        sequence: 3,
        keys: vec![],
        worlds: vec![orqwa_licence::trust::WithdrawnWorld {
            id: "11111111-2222-4333-8444-555555555555".into(),
            version: None,
        }],
    };
    let signed = Signed::sign(&list, Context::Revocations, &dev::root()).unwrap();
    let (url, _) = server(move |path, body| {
        let (status, reply) = activating(path, body);
        let mut answer: ActivateResponse = serde_json::from_slice(&reply).unwrap();
        answer.revocations = Some(signed.clone());
        (status, serde_json::to_vec(&answer).unwrap())
    });
    let (device, data) = dirs("withdrawals");
    licensing(&device, &data, &url, b"m")
        .activate(WORLD, "key")
        .await
        .unwrap();
    let restarted = Licensing::with_server(device, &data, &url, true);
    let trust = restarted.trust();
    assert!(
        trust
            .revocations()
            .world("11111111-2222-4333-8444-555555555555", None)
    );
}

#[tokio::test]
async fn without_a_machine_id_nothing_is_activated_or_sent() {
    let (url, requests) = server(activating);
    let (device, data) = dirs("unbound");
    let licensing = Licensing::with_server(device.clone(), &data, &url, true);
    licensing.set_machine_id(Vec::new());
    if device::machine_id().is_none() {
        assert!(matches!(
            licensing.activate(WORLD, "key").await,
            Err(LicensingError::Licence(LicenceError::NoMachine))
        ));
        assert_eq!(requests.load(Ordering::SeqCst), 0, "nothing was sent");
        assert!(!device.join(device::FILE).exists());
    }
}

#[tokio::test]
async fn a_redirect_is_not_followed_with_the_keys() {
    let (elsewhere, reached) = server(activating);
    let (url, _) = server(move |_, _| (307, elsewhere.clone().into_bytes()));
    let (device, data) = dirs("redirect");
    let licensing = licensing(&device, &data, &url, b"m");
    assert!(licensing.activate(WORLD, "key").await.is_err());
    assert_eq!(
        reached.load(Ordering::SeqCst),
        0,
        "the licence key went nowhere else"
    );
}
