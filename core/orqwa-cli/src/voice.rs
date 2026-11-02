//! Optional voice UI; setup and native device work never wait in the trainer loop.

use clap::Args;
use orqwa_net::state::RiderId;
use orqwa_net::voice::VoicePort;
use orqwa_voice::{VoiceChat, VoiceError, VoiceEvent, device::DeviceOptions};
use tokio::task::JoinHandle;

#[derive(Args)]
pub(crate) struct VoiceArgs {
    /// Listen to group voice, initially with the microphone muted. Use headphones.
    #[arg(long)]
    voice: bool,
    /// Microphone ID from voice-devices; otherwise use the OS default.
    #[arg(long, requires = "voice")]
    voice_input: Option<String>,
    /// Speaker/headphone ID from voice-devices; otherwise use the OS default.
    #[arg(long, requires = "voice")]
    voice_output: Option<String>,
}

pub(crate) struct Voice {
    pending: Option<JoinHandle<Result<VoiceChat, VoiceError>>>,
    chat: Option<VoiceChat>,
}
impl Voice {
    pub fn open(args: &VoiceArgs, port: Option<VoicePort>) -> Self {
        let pending = if args.voice {
            port.map(|port| {
                let options = DeviceOptions { input: args.voice_input.clone(), output: args.voice_output.clone() };
                println!("Starting voice with microphone muted. Use headphones; mic on / mic off controls transmission.");
                tokio::spawn(VoiceChat::open(port,options))
            })
        } else {
            None
        };
        Self {
            pending,
            chat: None,
        }
    }
    pub fn active(&self) -> bool {
        self.pending.is_some() || self.chat.is_some()
    }
    pub async fn next(&mut self) {
        if let Some(pending) = &mut self.pending {
            let result = pending.await;
            self.pending = None;
            match result {
                Ok(Ok(chat)) => {
                    self.chat = Some(chat);
                    println!(
                        "Voice ready; microphone muted. Commands: mic on, mic off, deafen, undeafen, mute <rider ID>, unmute <rider ID>, volume <0..1>."
                    );
                }
                Ok(Err(error)) => eprintln!("Voice unavailable: {error}. Continuing the ride."),
                Err(_) => eprintln!("Voice setup ended. Continuing the ride."),
            }
        } else if let Some(chat) = &mut self.chat {
            match chat.next().await {
                Some(VoiceEvent::DeviceUnavailable { error }) => {
                    eprintln!("Voice device unavailable: {error}. The ride continues.");
                }
                Some(VoiceEvent::PeerMuted { peer }) => eprintln!(
                    "Invalid speech from rider {}; that speaker was muted locally.",
                    peer.value()
                ),
                Some(VoiceEvent::Stopped) | None => {
                    self.chat = None;
                    println!("Voice stopped. The local ride continues.");
                }
            }
        } else {
            std::future::pending::<()>().await;
        }
    }
    pub fn command(&self, line: &str) -> bool {
        let words: Vec<_> = line.split_whitespace().take(3).collect();
        let recognized = matches!(
            words.first(),
            Some(&"mic" | &"deafen" | &"undeafen" | &"mute" | &"unmute" | &"volume")
        );
        if !recognized {
            return false;
        }
        let Some(chat) = &self.chat else {
            println!(
                "Voice is off or still starting; start the group command with --voice and wait for Voice ready."
            );
            return true;
        };
        match words.as_slice() {
            ["mic", "on"] => match chat.microphone(true) {
                Ok(()) => println!("Microphone on; transmitting to consenting group riders."),
                Err(error) => eprintln!("{error}"),
            },
            ["mic", "off"] => {
                let _ = chat.microphone(false);
                println!("Microphone muted.");
            }
            ["deafen"] => {
                chat.deafen(true);
                println!("Voice deafened and microphone muted.");
            }
            ["undeafen"] => {
                chat.deafen(false);
                println!("Voice listening; microphone remains muted.");
            }
            [action @ ("mute" | "unmute"), number] => match number
                .parse::<u8>()
                .ok()
                .and_then(|id| RiderId::new(id).ok())
            {
                Some(peer) => {
                    chat.mute_peer(peer, *action == "mute");
                    println!(
                        "Voice rider {}: {}",
                        peer.value(),
                        if *action == "mute" {
                            "muted locally"
                        } else {
                            "unmuted locally"
                        }
                    );
                }
                None => eprintln!("Choose a voice rider ID from 0 to 7."),
            },
            ["volume", value] => match value
                .parse::<f32>()
                .ok()
                .and_then(|value| chat.volume(value).ok().map(|()| value))
            {
                Some(value) => println!("Voice volume {value:.2}"),
                None => eprintln!("Choose voice volume from 0 to 1."),
            },
            _ => eprintln!(
                "Use mic on/off, deafen, undeafen, mute/unmute <rider ID>, or volume <0..1>."
            ),
        }
        true
    }
    pub async fn close(mut self) {
        if let Some(pending) = self.pending.take() {
            pending.abort();
            let _ = pending.await;
        }
        if let Some(chat) = self.chat.take() {
            chat.close().await;
        }
    }
}
impl Drop for Voice {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.abort();
        }
    }
}

pub(crate) fn devices() -> anyhow::Result<()> {
    for device in orqwa_voice::device::devices()? {
        println!(
            "input={} output={} {:?}: {:?}",
            device.input, device.output, device.id, device.name
        );
    }
    Ok(())
}
