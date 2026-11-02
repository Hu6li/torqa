//! R159–R161: real CLI processes keep recording only their own rider after host dropout.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::{Duration, Instant, SystemTime};

struct Process {
    child: Child,
    lines: Receiver<String>,
}
impl Process {
    fn start(directory: &Path, name: &str, power: &str, invite: Option<&str>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_orqwa-cli"));
        command.args([
            "ride-together",
            "--loopback",
            "--fake",
            "--offline",
            "--countdown",
            "1",
            "--name",
            name,
            "--fake-power",
            power,
        ]);
        command
            .arg("--route")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/oberalp.gpx"));
        command
            .arg("--output")
            .arg(directory.join(format!("{name}.fit")));
        if let Some(invite) = invite {
            command.args(["--join", invite]);
        }
        let mut child = command
            .current_dir(directory)
            .env("RUST_LOG", "error")
            .env("XDG_CACHE_HOME", directory.join("cache"))
            .env("XDG_CONFIG_HOME", directory.join("config"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = sync_channel(32);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }
    fn until(&self, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            // Do not include output in failures: it contains private invite/reply codes.
            let line = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("CLI output deadline");
            if predicate(&line) {
                return line;
            }
        }
    }
    fn input(&mut self, text: &str) {
        let input = self.child.stdin.as_mut().unwrap();
        writeln!(input, "{text}").unwrap();
        input.flush().unwrap();
    }
    fn quit(&mut self) {
        self.input("q");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "CLI shutdown deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn separate_trainers_record_only_their_own_power_and_survive_host_exit() {
    let directory = Directory(std::env::temp_dir().join(format!(
            "orqwa-group-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&directory.0).unwrap();
    let mut host = Process::start(&directory.0, "Host", "180", None);
    let invite = host.until(|line| line.starts_with("Invite: "));
    let mut joiner = Process::start(
        &directory.0,
        "Anna",
        "300",
        Some(invite.strip_prefix("Invite: ").unwrap()),
    );
    let reply = joiner.until(|line| line.starts_with("Reply: "));
    host.input(reply.strip_prefix("Reply: ").unwrap());
    host.until(|line| line == "Anna joined");
    host.input("go");
    host.until(|line| line == "Go!");
    joiner.until(|line| line == "Go!");
    host.until(|line| line.contains("Anna ") && line.contains(" m"));
    joiner.until(|line| line.contains("Host ") && line.contains(" m"));
    std::thread::sleep(Duration::from_secs(2));
    host.quit();
    joiner.until(|line| line.contains("continue riding alone"));
    std::thread::sleep(Duration::from_secs(2));
    joiner.quit();
    let read = |name| {
        orqwa_storage::decode_fit(&std::fs::read(directory.0.join(format!("{name}.fit"))).unwrap())
            .unwrap()
            .1
    };
    let host_samples = read("Host");
    let joiner_samples = read("Anna");
    assert!(
        host_samples.len() >= 2,
        "at least two seconds were recorded"
    );
    assert!(joiner_samples.len() > host_samples.len());
    for (samples, power) in [(&host_samples, 180.0), (&joiner_samples, 300.0)] {
        assert!(samples.iter().all(|sample| {
            sample
                .power
                .is_some_and(|watts| (watts.0 - power).abs() < 1.0)
        }));
        assert!(
            samples.iter().any(|sample| sample.heart_rate.is_some()),
            "own heart rate stays in the local activity"
        );
    }
}
