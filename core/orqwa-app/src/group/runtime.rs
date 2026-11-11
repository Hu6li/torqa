//! One owned app group task; content and audio work never run in the trainer loop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use orqwa_domain::workout::Plan;
use orqwa_net::activity::{Assistance, ContentId, ContentKind, CourseKey, Selection};
use orqwa_net::code::{Invite, Reply};
use orqwa_net::content::{self, Source};
use orqwa_net::link::{ConnectOptions, Host, Joiner};
use orqwa_net::lobby::{
    HostLobby, JoinerLobby, LobbyEvent, RideProgress, Rider, SharedActivity, Stage,
};
use orqwa_net::preview::Preview;
use orqwa_net::state::{RiderId, RiderState};
use orqwa_net::voice::VoicePort;
use orqwa_voice::audio_test::{AudioTest, TestKind};
use orqwa_voice::{VoiceChat, device::DeviceOptions};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

use super::preview::Choice;
use super::{Phase, Previewed, Refusal, Snapshot, VoiceControls};
use crate::Kind;
use std::sync::{Arc, atomic::Ordering};

pub(super) enum Command {
    Admit(String),
    Select(Option<PathBuf>, Option<Plan>),
    Assistance(Assistance),
    Prepared(u64),
    /// How far building the selected course's world has got, 0–100 (#419).
    Preparing(u8),
    Ready(bool),
    Start,
    End(u64),
    Enter,
    WorkoutPause(bool),
    WorkoutSkip(Duration),
    Handover(RiderId),
    /// Selects this membership as fallback, with an offer only after the shared start.
    Fallback(RiderId, u64),
    HandoverAnswer(bool),
    HandoverCancel,
    Remove(RiderId),
    Retry,
    /// Try to reconnect to the organizer now (#434).
    Reconnect,
    /// Lose the connection to the organizer as a failing network would.
    #[cfg(test)]
    LoseConnection,
    Failure(String),
    Voice(u64, bool, bool, bool, f32),
    Volume(f32),
    Devices,
    /// New devices, and whether the microphone was on: it stays so after the restart.
    ChooseDevices(u64, String, String, bool),
    Mute(RiderId, bool),
    AudioTest(u64, Option<TestKind>),
    EchoPrevention(bool),
}
#[derive(Clone, Copy)]
pub(super) struct Local {
    pub(super) state: RiderState,
    pub(super) progress: Option<RideProgress>,
}

pub(super) enum Session {
    Host(HostLobby),
    Join(JoinerLobby),
}
impl Session {
    pub(super) async fn next(&mut self) -> Option<LobbyEvent> {
        match self {
            Self::Host(l) => l.next().await,
            Self::Join(l) => l.next().await,
        }
    }
    pub(super) fn handover(
        &self,
        recipient: Option<RiderId>,
        message: orqwa_net::handover::Message,
    ) -> Result<(), String> {
        match self {
            Self::Host(lobby) => lobby.handover(recipient, message),
            Self::Join(lobby) => lobby.handover(message),
        }
        .map_err(|error| error.to_string())
    }
    async fn ready(&self, revision: u64, ready: bool) -> Result<(), String> {
        match self {
            Self::Host(l) => l.ready(revision, ready).await.map_err(|e| e.to_string()),
            Self::Join(l) => l.ready(revision, ready).map_err(|e| e.to_string()),
        }
    }
    fn preparation(&self, revision: u64, stage: Stage, percent: u8) {
        // Advisory: the next report, or readiness, supersedes a lost one.
        let _ = match self {
            Self::Host(lobby) => lobby.preparation(revision, stage, percent),
            Self::Join(lobby) => lobby.preparation(revision, stage, percent),
        };
    }
    fn state(&self, local: Option<Local>) {
        if let Self::Host(lobby) = self {
            let _ = lobby.progress(local.and_then(|local| local.progress));
        }
        if let Some(local) = local {
            let _ = match self {
                Self::Host(lobby) => lobby.send_state(local.state),
                Self::Join(lobby) => lobby.send_state(local.state),
            };
        }
    }
}
/// This rider's preparation as last told to the group (#419): on a change of stage or once
/// done at once, otherwise at most every `EVERY`, which keeps well within the host's control
/// budget.
#[derive(Default)]
struct Reported(Option<(u64, Stage, u8, tokio::time::Instant)>);
impl Reported {
    const EVERY: Duration = Duration::from_secs(2);
    fn report(&mut self, session: &Session, revision: u64, stage: Stage, percent: u8) {
        if self.due(revision, stage, percent, tokio::time::Instant::now()) {
            session.preparation(revision, stage, percent);
        }
    }
    /// Whether to tell the group, remembering it as told if so. Nothing follows the end of
    /// preparing to supersede it: held back, the rider would show as still building until ready.
    fn due(&mut self, revision: u64, stage: Stage, percent: u8, now: tokio::time::Instant) -> bool {
        let due = self.0.is_none_or(|(r, s, p, at)| {
            r != revision
                || s != stage
                || (p != percent && (percent == 100 || now.duration_since(at) >= Self::EVERY))
        });
        if due {
            self.0 = Some((revision, stage, percent, now));
        }
        due
    }
}
#[cfg(test)]
mod reported_tests {
    use super::{Reported, Stage};

    #[test]
    fn a_course_prepared_quickly_still_tells_the_group_it_is_done() {
        let start = tokio::time::Instant::now();
        let mut reported = Reported::default();
        assert!(reported.due(1, Stage::Preparing, 50, start));
        let soon = start + Reported::EVERY / 4;
        assert!(
            !reported.due(1, Stage::Preparing, 55, soon),
            "progress is told a few times only"
        );
        assert!(
            reported.due(1, Stage::Preparing, 100, soon),
            "done is told at once"
        );
        assert!(!reported.due(1, Stage::Preparing, 100, soon), "and once");
    }
}
/// A friend's copy of what the organizer is choosing (#418): one fetch at a time, at most
/// every [`Previews::EVERY`], of the latest the organizer named.
#[derive(Default)]
struct Previews {
    wanted: Option<ContentId>,
    asked: Option<std::time::Instant>,
    /// Not asked for again until the organizer names another, or the link comes back.
    failed: Option<ContentId>,
    fetching: JoinSet<(ContentId, Result<Preview, String>)>,
}
impl Previews {
    /// Keeps well within the organizer's request budget, one a second, beside downloads.
    const EVERY: Duration = Duration::from_secs(2);
    fn named(&mut self, snapshot: &mut Snapshot, id: Option<ContentId>) {
        self.wanted = id;
        if id.is_none() {
            self.fetching.abort_all();
            snapshot.preview = None;
        }
    }
    fn fetch(&mut self, session: &Session, snapshot: &Snapshot) {
        let (Session::Join(lobby), Some(id)) = (session, self.wanted) else {
            return;
        };
        if !self.fetching.is_empty()
            || self.failed == Some(id)
            || snapshot.start.is_some()
            || snapshot
                .preview
                .as_ref()
                .is_some_and(|shown| shown.id == id)
            || self.asked.is_some_and(|at| at.elapsed() < Self::EVERY)
        {
            return;
        }
        let Some(link) = lobby.content_link() else {
            return;
        };
        self.asked = Some(std::time::Instant::now());
        self.fetching.spawn(async move {
            let preview = content::fetch_preview(&link, id)
                .await
                .map_err(|error| error.to_string())
                .and_then(|bytes| Preview::decode(&bytes).map_err(|error| error.to_string()));
            (id, preview)
        });
    }
    fn fetched(
        &mut self,
        snapshot: &mut Snapshot,
        id: ContentId,
        preview: Result<Preview, String>,
    ) {
        match preview {
            Ok(preview) if self.wanted == Some(id) => {
                snapshot.preview = Some(Arc::new(Previewed { id, preview }));
            }
            Ok(_) => {}
            Err(error) => {
                tracing::debug!(%error, "the organizer's preview is unavailable");
                if self.wanted == Some(id) {
                    self.failed = Some(id);
                }
            }
        }
    }
}

/// Shows friends the organizer's `choice` (#418), unless it is nothing or what they share
/// already. Building it reads the course file, off the group's task.
async fn browse(
    session: &Session,
    snapshot: &Snapshot,
    building: &mut JoinSet<Result<Preview, String>>,
    choice: Choice,
    shared: Option<&Choice>,
) {
    let Session::Host(lobby) = session else {
        return;
    };
    if snapshot.start.is_some() {
        return;
    }
    building.abort_all();
    while building.join_next().await.is_some() {}
    if choice.is_empty() || shared == Some(&choice) {
        let _ = lobby.preview(None);
        return;
    }
    building.spawn(async move {
        tokio::task::spawn_blocking(move || super::preview::build(&choice))
            .await
            .map_err(|error| error.to_string())?
    });
}

enum Prepared {
    Host(SharedActivity),
    Join(Option<PathBuf>, Option<PathBuf>),
}
struct Audio {
    generation: u64,
    chat: VoiceChat,
    deaf: bool,
    /// An automatic restart keeps the microphone on if it was; a first start begins muted.
    mic: bool,
}

/// Opens voice on `port` with the chosen devices; app tests may ask for silent audio instead.
/// Why the organizer's reply added nobody, in words for the rider beside their field (R195);
/// `None` for failures that are no fault of the paste.
fn refusal(error: &orqwa_net::lobby::LobbyError) -> Option<Refusal> {
    use orqwa_net::code::CodeError;
    use orqwa_net::link::LinkError;
    use orqwa_net::lobby::LobbyError;
    match error {
        LobbyError::Link(LinkError::Code(CodeError::WrongRide)) => Some(Refusal::OtherRide),
        LobbyError::Link(LinkError::Code(_)) => Some(Refusal::Invalid),
        LobbyError::Link(LinkError::Replay) => Some(Refusal::Again),
        LobbyError::Link(LinkError::Full) => Some(Refusal::Full),
        LobbyError::Started => Some(Refusal::Started),
        _ => None,
    }
}

async fn open_voice(
    backend: AudioBackend,
    port: orqwa_net::voice::VoicePort,
    options: DeviceOptions,
    controls: Arc<VoiceControls>,
) -> Result<VoiceChat, String> {
    #[cfg(test)]
    if controls.silent.load(Ordering::Acquire) {
        return VoiceChat::with_audio(port, |_| Ok(Box::new(super::tests::Silent)))
            .await
            .map_err(|error| error.to_string());
    }
    let _ = controls;
    backend
        .voice(port, options)
        .await
        .map_err(|error| error.to_string())
}

type AudioFactory = Arc<
    dyn Fn(
            Arc<std::sync::atomic::AtomicBool>,
        ) -> Result<
            Box<dyn orqwa_domain::voice::VoiceAudio>,
            orqwa_domain::voice::VoiceDeviceError,
        > + Send
        + Sync,
>;

#[derive(Clone, Default)]
pub(super) struct AudioBackend {
    factory: Option<AudioFactory>,
}
impl AudioBackend {
    async fn voice(
        self,
        port: orqwa_net::voice::VoicePort,
        options: DeviceOptions,
    ) -> Result<VoiceChat, orqwa_voice::VoiceError> {
        if let Some(factory) = self.factory {
            VoiceChat::with_audio(port, move |stop| factory(stop)).await
        } else {
            VoiceChat::open(port, options).await
        }
    }
    async fn test(
        self,
        kind: TestKind,
        volume: f32,
        options: DeviceOptions,
    ) -> Result<AudioTest, orqwa_voice::VoiceError> {
        if let Some(factory) = self.factory {
            match kind {
                TestKind::Microphone => AudioTest::with_audio(move |stop| factory(stop)).await,
                TestKind::Speakers => {
                    AudioTest::with_speaker_audio(volume, move |stop| factory(stop)).await
                }
            }
        } else {
            match kind {
                TestKind::Microphone => AudioTest::open(options).await,
                TestKind::Speakers => AudioTest::speakers(options, volume).await,
            }
        }
    }
}

#[cfg(test)]
#[path = "audio_tests.rs"]
mod audio_tests;

pub(super) struct Request {
    pub(super) audio: AudioBackend,
    pub(super) voice_controls: Arc<VoiceControls>,
    pub(super) invite: Option<String>,
    pub(super) rider: Rider,
    pub(super) options: ConnectOptions,
    pub(super) data: PathBuf,
    pub(super) temporary: PathBuf,
    /// What the organizer's activity picker has, to show friends (#418).
    pub(super) choices: watch::Receiver<Option<Choice>>,
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) async fn run(
    request: Request,
    mut commands: mpsc::Receiver<Command>,
    updates: watch::Sender<Snapshot>,
    mut local: watch::Receiver<Option<Local>>,
) {
    let mut snapshot = Snapshot {
        phase: Phase::Connecting,
        host: request.invite.is_none(),
        ..Snapshot::default()
    };
    updates.send_replace(snapshot.clone());
    if let Err(error) = run_inner(request, &mut commands, &updates, &mut snapshot, &mut local).await
    {
        snapshot.phase = Phase::Failed;
        if !snapshot.reply_expired {
            snapshot.report(Kind::GroupRide, error);
        }
        snapshot.invite.clear();
        snapshot.reply.clear();
    }
    updates.send_replace(snapshot);
}

#[allow(clippy::too_many_lines)] // one owner makes all state/transfer/audio cancellation paths explicit
async fn run_inner(
    request: Request,
    commands: &mut mpsc::Receiver<Command>,
    updates: &watch::Sender<Snapshot>,
    snapshot: &mut Snapshot,
    local: &mut watch::Receiver<Option<Local>>,
) -> Result<(), String> {
    let policy = request.options.gather.policy;
    let temp = Directory(request.temporary);
    tokio::fs::create_dir_all(&temp.0)
        .await
        .map_err(|e| e.to_string())?;
    let mut session = if let Some(invite) = request.invite {
        let invite = Invite::from_code(&invite, policy).map_err(|e| e.to_string())?;
        let mode = invite.setup.mode();
        // Taken before the joiner starts its own, so never later than the reply's real end.
        let until = std::time::Instant::now() + request.options.window;
        let joiner = Joiner::lobby(invite, &request.options)
            .await
            .map_err(|e| e.to_string())?;
        snapshot.reply = joiner.reply().to_code(policy).map_err(|e| e.to_string())?;
        snapshot.reachability = Some(joiner.reply().reachability(mode));
        snapshot.reply_until = Some(until);
        updates.send_replace(snapshot.clone());
        let lobby = JoinerLobby::join(joiner, request.rider.clone())
            .await
            .map_err(|e| {
                // Not added in time: the rider makes a new reply rather than read a failure.
                snapshot.reply_expired = std::time::Instant::now() >= until;
                e.to_string()
            })?;
        snapshot.reply_until = None;
        snapshot.me = lobby.me().value();
        Session::Join(lobby)
    } else {
        let host = Host::lobby(&request.rider.name, &request.options)
            .await
            .map_err(|e| e.to_string())?;
        snapshot.invite = host.invite().to_code(policy).map_err(|e| e.to_string())?;
        snapshot.reachability = Some(host.invite().reachability());
        snapshot.host = true;
        Session::Host(HostLobby::open(host, request.rider.clone()).map_err(|e| e.to_string())?)
    };
    let mut handover =
        super::handover::Coordinator::new(request.options.clone(), request.rider.clone());
    let (root, epoch, organizer) = match &session {
        Session::Host(l) => (l.root(), l.epoch(), l.organizer()),
        Session::Join(l) => (l.root(), l.epoch(), l.organizer()),
    };
    snapshot.root = Some(root);
    snapshot.epoch = epoch;
    snapshot.organizer = organizer.value();
    snapshot.roster[usize::from(snapshot.me)] = Some(request.rider);
    snapshot.generations[usize::from(snapshot.me)] = 1;
    snapshot.phase = Phase::Waiting;
    snapshot.reply.clear();
    let mut port = match &mut session {
        Session::Host(l) => l.take_voice(),
        Session::Join(l) => l.take_voice(),
    };
    request
        .voice_controls
        .port
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone_from(&port);
    let mut voice: Option<VoiceChat> = None;
    let mut device_options = DeviceOptions::default();
    let mut audio = JoinSet::new();
    let mut scans = JoinSet::new();
    let mut microphone_test: Option<AudioTest> = None;
    // The output the rider last said how they listen on; `None` takes the next one voice
    // opens, as after joining or choosing devices right after being asked (R194).
    let mut listening_for: Option<String> = None;
    let mut mic_starts = JoinSet::new();
    // Voice to restart once a link allows, and whether with the microphone on.
    let mut resume_voice: Option<bool> = None;
    let mut internal = None;
    let mut jobs = JoinSet::new();
    let mut next_revision = 0u64;
    let (progress, mut latest_progress) = watch::channel(content::Progress {
        received: 0,
        total: 0,
    });
    let mut reported = Reported::default();
    let mut choices = request.choices.clone();
    // The organizer's last shared choice, its preview's builder, and a friend's previews.
    let mut shared_choice: Option<Choice> = None;
    let mut building = JoinSet::new();
    let mut previews = Previews::default();
    let mut ticker = tokio::time::interval(Duration::from_millis(200));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        previews.fetch(&session, snapshot);
        updates.send_replace(snapshot.clone());
        tokio::select! {
            event=voice_event(&mut voice)=>{
                match event {
                    Some(orqwa_voice::VoiceEvent::PeerMuted {peer})=>{snapshot.muted[usize::from(peer.value())]=true;snapshot.report(Kind::GroupVoice,"A speaker sent invalid audio and was muted locally.");}
                    Some(orqwa_voice::VoiceEvent::DeviceUnavailable {error})=>{voice=None;snapshot.voice=false;snapshot.microphone=false;snapshot.report(Kind::GroupVoice,error);}
                    Some(orqwa_voice::VoiceEvent::Stopped)|None=>{
                        // The organizer's link ended under voice, as when they leave: a fallback
                        // taking over restarts it with the microphone as it was (#426, R188).
                        if !port.as_ref().is_some_and(VoicePort::alive) {resume_voice=Some(snapshot.microphone);}
                        voice=None;snapshot.voice=false;snapshot.microphone=false;
                    }
                }
            },
            command = async {if let Some(command)=internal.take() {Some(command)} else {commands.recv().await}} => {
                let Some(command) = command else {return Ok(());};
                if handover.busy() && matches!(command, Command::Admit(_) | Command::Remove(_) | Command::WorkoutPause(_) | Command::WorkoutSkip(_) | Command::Ready(_) | Command::Enter) {
                    snapshot.report(Kind::GroupRide,"Wait until the organizer change finishes, or cancel it first."); continue;
                }
                let result = match command {
                    Command::Handover(id) => handover.propose(&session,snapshot,id),
                    Command::Fallback(id,generation) => {
                        if snapshot.generations[usize::from(id.value())] == generation {
                            handover.choose_fallback(&session,snapshot,id)
                        } else {Err("The selected friend has left the group.".into())}
                    },
                    Command::HandoverAnswer(accepted) => handover.answer(&session,snapshot,accepted),
                    Command::HandoverCancel => {
                        if snapshot.host || snapshot.successor == Some(snapshot.me) {handover.cancel(&session,snapshot)}
                        else {Err("Only the organizer or selected successor can cancel this request.".into())}
                    }
                    Command::Admit(text) => match &session {
                        Session::Host(l) => {
                            // Shown beside the organizer's field, not as a failure (R195).
                            snapshot.refusal = Refusal::None;
                            if let Ok(reply) = Reply::from_code(&text,policy) {
                                match l.admit(&reply).await {
                                    Ok(()) => {snapshot.admitting = snapshot.admitting.saturating_add(1); Ok(())}
                                    Err(error) => refusal(&error).map_or_else(|| Err(error.to_string()), |refused| {snapshot.refuse(refused); Ok(())}),
                                }
                            } else {snapshot.refuse(Refusal::Invalid); Ok(())}
                        }
                        Session::Join(_) => Err("Only the host can admit riders.".into()),
                    },
                    Command::Select(course,plan) => {
                        if snapshot.start.is_some() || !snapshot.host {continue;}
                        // What is shared now needs no preview (#418).
                        building.abort_all(); while building.join_next().await.is_some() {}
                        shared_choice = Some(Choice {course: course.clone(), plan: plan.clone()});
                        if let Session::Host(lobby) = &session {let _ = lobby.preview(None);}
                        jobs.abort_all(); while jobs.join_next().await.is_some() {}
                        next_revision = next_revision.checked_add(1).ok_or("Activity revision exhausted.")?;
                        let revision = next_revision; let dir = temp.0.clone();
                        snapshot.phase = Phase::Downloading; snapshot.error.clear();snapshot.activity_failed=false; snapshot.prepared = false;
                        jobs.spawn(async move {chosen(course,plan,dir,revision).await.map(Prepared::Host)}); Ok(())
                    }
                    Command::Assistance(rules) => change_rules(&session,snapshot,&mut next_revision,rules,jobs.is_empty()).await,
                    Command::Prepared(revision) => {if snapshot.selection.as_ref().is_some_and(|s|s.revision==revision) {snapshot.prepared = true; snapshot.phase = Phase::Ready; reported.report(&session,revision,Stage::Preparing,100);} Ok(())}
                    Command::Preparing(percent) => {if snapshot.phase==Phase::Preparing && let Some(selection)=&snapshot.selection {reported.report(&session,selection.revision,Stage::Preparing,percent.min(99));} Ok(())}
                    Command::Ready(ready) => {
                        if !snapshot.prepared {continue;}
                        session.ready(snapshot.selection.as_ref().ok_or("Select an activity first.")?.revision,ready).await
                    }
                    Command::Start => start(&session,snapshot).await,
                    Command::End(revision) => async {
                        if snapshot.host && snapshot.start.is_some() && snapshot.selection.as_ref().is_some_and(|s| s.revision == revision) {
                            handover.cancel(&session,snapshot)?;
                            if let Session::Host(lobby) = &session {lobby.end_activity(revision).await.map_err(|error|error.to_string())?;}
                        }
                        Ok(())
                    }.await,
                    Command::WorkoutPause(paused) => match &session {Session::Host(lobby) => lobby.workout_pause(paused).await.map_err(|error| error.to_string()), Session::Join(_) => Err("Only the organizer can control the shared workout.".into())},
                    Command::WorkoutSkip(next) => match &session {Session::Host(lobby) => lobby.workout_skip(next).await.map_err(|error| error.to_string()), Session::Join(_) => Err("Only the organizer can control the shared workout.".into())},
                    Command::Enter => {
                        if let Session::Join(lobby) = &session && snapshot.can_enter() {
                            let revision = snapshot.selection.as_ref().ok_or("Select an activity first.")?.revision;
                            lobby.ready(revision, true).map_err(|error| error.to_string())?;
                            lobby.enter(revision).map_err(|error| error.to_string())?;
                            snapshot.entering = true;
                        }
                        Ok(())
                    },
                    Command::Remove(id) => match &session {Session::Host(l)=>l.remove(id).map_err(|e|e.to_string()),Session::Join(_)=>Err("Only the host removes riders.".into())},
                    Command::Retry => {download_again(&session,snapshot,&mut jobs,&progress,&request.data).await}
                    Command::Reconnect => {if let Session::Join(lobby) = &session {lobby.reconnect();} Ok(())}
                    #[cfg(test)]
                    Command::LoseConnection => {if let Session::Join(lobby) = &session {lobby.lose_connection();} Ok(())}
                    Command::Failure(error) => {snapshot.activity_error(error); Ok(())}
                    Command::Devices => {
                        if scans.is_empty() {scans.spawn(async {tokio::task::spawn_blocking(||orqwa_voice::device::devices().map(|devices|devices.into_iter().take(32).map(|d|(d.id,d.name,d.input,d.output)).collect::<Vec<_>>()).map_err(|e|e.to_string())).await.map_err(|e|e.to_string())?});} Ok(())
                    }
                    Command::ChooseDevices(generation,input,output,mic) => {
                        if generation!=request.voice_controls.generation.load(Ordering::Acquire){continue;}
                        request.voice_controls.audio_test.store(0,Ordering::Release);
                        listening_for=None;
                        let restart = snapshot.voice || snapshot.voice_pending || resume_voice.is_some();
                        resume_voice=None;
                        mic_starts.abort_all();while mic_starts.join_next().await.is_some() {}
                        let previous_test=microphone_test.take();snapshot.microphone_test=false;snapshot.speaker_test=false;snapshot.microphone_voice_detected=false;snapshot.microphone_level=0.0;
                        audio.abort_all(); while audio.join_next().await.is_some() {}
                        let _gate=request.voice_controls.port.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        if generation!=request.voice_controls.generation.load(Ordering::Acquire){continue;}
                        let previous=voice.take();snapshot.voice=false;snapshot.voice_pending=false;snapshot.microphone=false;
                        if snapshot.input_device!=input {snapshot.microphone_checked=false;snapshot.microphone_voice_heard=false;}
                        snapshot.input_device.clone_from(&input);snapshot.output_device.clone_from(&output);
                        device_options=DeviceOptions {input:(!input.is_empty()).then_some(input),output:(!output.is_empty()).then_some(output)};
                        if restart && let Some(port)=&port {
                            snapshot.voice_pending=true;
                            let backend=request.audio.clone();let port=port.clone();let options=device_options.clone();let deaf=snapshot.deafened;let controls=request.voice_controls.clone();
                            audio.spawn(async move {
                                if let Some(test)=previous_test {test.close().await;}
                                if let Some(chat)=previous {chat.close().await;}
                                open_voice(backend,port,options,controls).await.map(|chat|Audio {generation,chat,deaf,mic})
                            });
                        } Ok(())
                    }
                    Command::Mute(id,muted) => {if let Some(chat)=&voice {chat.mute_peer(id,muted);snapshot.muted[usize::from(id.value())]=muted;} Ok(())}
                    Command::Volume(volume)=>{
                        snapshot.voice_volume=volume;
                        if let Some(chat)=&voice {let _=chat.volume(volume);}
                        if let Some(test)=&microphone_test {let _=test.volume(volume);}
                        Ok(())
                    }
                    Command::EchoPrevention(enabled)=>{
                        snapshot.echo_prevention=enabled;
                        if let Some(chat)=&voice {chat.echo_prevention(enabled);}
                        listening_for=(!snapshot.output_name.is_empty()).then(||snapshot.output_name.clone());
                        snapshot.ask_listening=false;
                        Ok(())
                    }
                    Command::AudioTest(generation,kind)=>{
                        if generation!=request.voice_controls.generation.load(Ordering::Acquire){continue;}
                        request.voice_controls.audio_test.store(kind.map_or(0,|kind|kind as u8),Ordering::Release);
                        audio.abort_all();while audio.join_next().await.is_some() {}
                        mic_starts.abort_all();while mic_starts.join_next().await.is_some() {}
                        let previous=voice.take();let previous_test=microphone_test.take();
                        // Voice comes back after the check with the microphone as it was (R188).
                        resume_voice=resume_voice.or((snapshot.voice || snapshot.voice_pending).then_some(snapshot.microphone));
                        snapshot.voice=false;snapshot.voice_pending=false;snapshot.microphone=false;
                        if snapshot.microphone_test && let Some(test)=&previous_test {snapshot.microphone_voice_heard |= test.voice_heard();}
                        snapshot.microphone_test=kind==Some(TestKind::Microphone);snapshot.speaker_test=kind==Some(TestKind::Speakers);snapshot.microphone_level=0.0;snapshot.microphone_voice_detected=false;
                        if snapshot.microphone_test {snapshot.microphone_checked=true;snapshot.microphone_voice_heard=false;}
                        if let Some(kind)=kind {
                            let volume=snapshot.voice_volume;let backend=request.audio.clone();let options=device_options.clone();
                            mic_starts.spawn(async move {
                                if let Some(chat)=previous {chat.close().await;}
                                if let Some(test)=previous_test {test.close().await;}
                                backend.test(kind,volume,options).await.map(|test|(generation,test)).map_err(|error|error.to_string())
                            });
                        } else if resume_voice.is_some() && let Some(port)=&port {
                            snapshot.voice_pending=true;
                            let backend=request.audio.clone();let port=port.clone();let options=device_options.clone();let deaf=snapshot.deafened;let controls=request.voice_controls.clone();
                            let mic=resume_voice.unwrap_or(false);
                            audio.spawn(async move {
                                if let Some(chat)=previous {chat.close().await;}
                                if let Some(test)=previous_test {test.close().await;}
                                open_voice(backend,port,options,controls).await.map(|chat|Audio {generation,chat,deaf,mic})
                            });
                            resume_voice=None;
                        } Ok(())
                    }
                    Command::Voice(generation,on,mic,deaf,volume) => {
                        if generation!=request.voice_controls.generation.load(Ordering::Acquire){continue;}
                        let previous_test=if snapshot.microphone_test || snapshot.speaker_test {
                            mic_starts.abort_all();while mic_starts.join_next().await.is_some() {}
                            audio.abort_all();while audio.join_next().await.is_some() {}
                            let test=microphone_test.take();
                            if snapshot.microphone_test && let Some(test)=&test {snapshot.microphone_voice_heard |= test.voice_heard();}
                            snapshot.microphone_test=false;snapshot.speaker_test=false;snapshot.microphone_level=0.0;snapshot.microphone_voice_detected=false;
                            resume_voice=None;
                            test
                        } else {None};
                        // Serialize generation checking and transmission changes with the UI's
                        // immediate mute gate; an older queued unmute cannot reopen it later.
                        let _gate=request.voice_controls.port.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        if generation!=request.voice_controls.generation.load(Ordering::Acquire){continue;}
                        request.voice_controls.audio_test.store(0,Ordering::Release);
                        snapshot.voice_volume=volume;
                        // A later choice can close a microphone kept for a restart, never open it.
                        resume_voice=resume_voice.filter(|_| on).map(|was| was && mic && !deaf);
                        if !on {listening_for=None; snapshot.ask_listening=false; audio.abort_all(); voice = None; snapshot.voice = false; snapshot.voice_pending=false; snapshot.microphone = false; snapshot.deafened = false;}
                        else if let Some(chat) = &voice {
                            let configured = (|| {chat.deafen(deaf); chat.volume(volume).map_err(|e|e.to_string())?;
                            chat.microphone(mic && !deaf).map_err(|e|e.to_string())?; Ok::<(),String>(())})();
                            if let Err(error) = configured {snapshot.report(Kind::GroupVoice,error);}
                            snapshot.microphone = chat.microphone_enabled(); snapshot.deafened = deaf;
                        } else if audio.is_empty() && let Some(port) = &port {
                            snapshot.voice_pending=true;
                            let backend=request.audio.clone();let port = port.clone();let options=device_options.clone();let controls=request.voice_controls.clone();
                            // Joining with the microphone on is the rider's explicit choice (R194).
                            audio.spawn(async move {
                                if let Some(test)=previous_test {test.close().await;}
                                open_voice(backend,port,options,controls).await.map(|chat|Audio {generation,chat,deaf,mic})
                            });
                        } Ok(())
                    }
                };
                if let Err(error) = result {snapshot.report(Kind::GroupRide,error);}
            },
            event = session.next(), if !handover.old_lost => match event {
                Some(LobbyEvent::Lobby {revision}) => {
                    if revision <= snapshot.ended_revision {continue;}
                    handover.reset(snapshot);
                    jobs.abort_all(); while jobs.join_next().await.is_some() {}
                    building.abort_all(); while building.join_next().await.is_some() {}
                    drop(choices.borrow_and_update()); shared_choice = None;
                    previews.named(snapshot, None);
                    next_revision = next_revision.max(revision);
                    snapshot.ended_revision = revision;
                    snapshot.phase = Phase::Waiting; snapshot.start = None;
                    snapshot.selection = None; snapshot.prepared = false;
                    snapshot.ready.fill(false); snapshot.preparation.fill(None);
                    snapshot.states.fill(None); snapshot.workout_clock = None;
                    snapshot.course = None; snapshot.workout = None; snapshot.plan = None;
                    snapshot.workout_id.clear(); snapshot.late = false;
                    snapshot.entering = false; snapshot.entry = None;
                    snapshot.received = 0; snapshot.total = 0; snapshot.activity_failed = false;
                    progress.send_replace(content::Progress {received:0,total:0}); drop(latest_progress.borrow_and_update());
                    drop(local.borrow_and_update()); session.state(None);
                    snapshot.refusal = Refusal::None; snapshot.error.clear();
                    snapshot.fallback = None; snapshot.standby_offer = !snapshot.host && snapshot.fallback_consent.is_some();
                }
                Some(LobbyEvent::Handover {sender,message}) => {
                    if let Err(error) = handover.message(&session,snapshot,sender,message).await {
                        snapshot.report(Kind::GroupRide,error);
                        if handover.fail(&session,snapshot) {end(snapshot);return Ok(());}
                    }
                }
                Some(LobbyEvent::Joined {id,rider}) => {
                    handover.forget_member(snapshot,id);
                    if snapshot.handover==super::Handover::Committing {continue;}
                    if handover.busy() && handover.fail(&session,snapshot) {end(snapshot);return Ok(());}
                    let index=usize::from(id.value());
                    snapshot.admitting = snapshot.admitting.saturating_sub(1);
                    snapshot.generations[index]=snapshot.generations[index].checked_add(1).ok_or("Rider membership exhausted.")?;
                    snapshot.roster[index] = Some(rider); snapshot.ready[index] = false;snapshot.states[index]=None; snapshot.latencies[index]=None;
                    handover.membership(&session,snapshot);
                }
                Some(LobbyEvent::Left {id}) => {handover.forget_member(snapshot,id); if handover.departure(snapshot,id) {continue;}
                    if handover.busy() && handover.fail(&session,snapshot) {end(snapshot);return Ok(());}
                    let index = usize::from(id.value()); snapshot.roster[index] = None; snapshot.ready[index] = false; snapshot.states[index] = None; snapshot.latencies[index] = None; snapshot.away[index] = false;
                    handover.membership(&session,snapshot);}
                // A rider whose connection was lost keeps their seat while reconnecting (#434).
                Some(LobbyEvent::Away {id}) => {let index = usize::from(id.value()); snapshot.away[index] = true; snapshot.states[index] = None; snapshot.latencies[index] = None;}
                Some(LobbyEvent::Back {id}) => {let index = usize::from(id.value()); snapshot.away[index] = false; snapshot.latencies[index] = None;},
                Some(LobbyEvent::Reconnecting {until}) => {
                    if handover.reconnecting(&session,snapshot) {end(snapshot);updates.send_replace(snapshot.clone());return Ok(());}
                    if !handover.old_lost {
                        // Others' positions and the group's workout clock come back with the
                        // connection; the microphone stays as it was.
                        snapshot.reconnecting = Some(until.into_std());
                        snapshot.latencies.fill(None);
                        snapshot.states.fill(None); snapshot.workout_clock = None;
                    }
                }
                Some(LobbyEvent::Reconnected) => {
                    snapshot.reconnecting = None; handover.reconnected(); previews.failed = None;
                    // A download the lost connection cut off starts again on the new one.
                    if snapshot.phase == Phase::Downloading || snapshot.can_retry() {download_again(&session,snapshot,&mut jobs,&progress,&request.data).await?;}
                }
                Some(LobbyEvent::Readiness {id,revision,ready}) => if snapshot.selection.as_ref().is_some_and(|s|s.revision==revision) {snapshot.ready[usize::from(id.value())] = ready; if ready {snapshot.preparation[usize::from(id.value())] = None;}},
                Some(LobbyEvent::Preparation {id,revision,stage,percent}) => if snapshot.selection.as_ref().is_some_and(|s|s.revision==revision) && !snapshot.ready[usize::from(id.value())] {snapshot.preparation[usize::from(id.value())] = Some((stage,percent));},
                Some(LobbyEvent::Selection(selection)) => {
                    if snapshot.start.is_some() {continue;}
                    if snapshot.selection.as_ref().is_some_and(|s|s.revision==selection.revision) {continue;}
                    let retained=snapshot.prepared && snapshot.selection.as_ref().is_some_and(|s|s.course==selection.course&&s.workout==selection.workout);
                    snapshot.assistance=selection.assistance;
                    if retained && jobs.is_empty() {
                        snapshot.selection=Some(selection);snapshot.ready.fill(false);snapshot.preparation.fill(None);snapshot.phase=Phase::Ready;continue;
                    }
                    jobs.abort_all(); while jobs.join_next().await.is_some() {}
                    snapshot.selection = Some(selection.clone()); snapshot.ready.fill(false); snapshot.preparation.fill(None); snapshot.prepared = false; snapshot.error.clear();snapshot.activity_failed=false;
                    snapshot.course = None; snapshot.workout = None; snapshot.plan=None;snapshot.workout_id.clear();
                    if let Session::Join(l) = &session {
                        let link = l.content_link().ok_or("The group connection ended.")?; let data = request.data.clone();
                        snapshot.phase = Phase::Downloading;
                        progress.send_replace(content::Progress {received:0,total:0});let progress=progress.clone();jobs.spawn(async move {download(&link,&selection,&data,&progress).await.map(|(c,w)|Prepared::Join(c,w))});
                    }
                }
                Some(LobbyEvent::Preview(id)) => previews.named(snapshot, id),
                Some(LobbyEvent::Assistance(rules)) => if snapshot.start.is_none() {snapshot.assistance = rules;},
                Some(LobbyEvent::Start {at}) => {snapshot.start = Some(at.into_std()); snapshot.phase = Phase::Countdown; previews.named(snapshot, None);}
                Some(LobbyEvent::Running {at}) => {snapshot.start = Some(at.into_std()); snapshot.late = true; previews.named(snapshot, None);}
                Some(LobbyEvent::Entry {revision,progress}) => {
                    if snapshot.entering && snapshot.selection.as_ref().is_some_and(|selected| selected.revision == revision) {
                        snapshot.entering = false;
                        if let Some(progress) = progress {
                            if progress.workout.is_some() != snapshot.plan.is_some() {return Err("The organizer sent progress for a different activity.".into());}
                            snapshot.entry = Some(progress); snapshot.phase = Phase::Riding;
                        } else {snapshot.report(Kind::GroupRide,"The organizer is not riding yet or has paused. Try joining again when they are riding.");}
                    }
                }
                Some(LobbyEvent::Workout(clock)) => {snapshot.workout_clock = Some(clock);}
                Some(LobbyEvent::State {id,state}) => {snapshot.states[usize::from(id.value())] = Some((state,std::time::Instant::now()));}
                Some(LobbyEvent::Expired) => {snapshot.admitting = snapshot.admitting.saturating_sub(1); snapshot.refuse(Refusal::Late);}
                Some(LobbyEvent::Failed {peer,error,operation}) => snapshot.report(Kind::GroupRide,format!("{operation} (rider {peer:?}): {error}")),
                Some(LobbyEvent::HostLeft)|None => {if !handover.lost() {end(snapshot);updates.send_replace(snapshot.clone());return Ok(());}}
            },
            event = handover.next() => {
                match handover.event(&session,snapshot,event) {
                    Ok(Some(replacement)) => {
                        // Close transmission synchronously before replacing any audio owner. Keep
                        // listening consent, and the microphone as it was, on the new link.
                        let restart = snapshot.voice || snapshot.voice_pending || resume_voice.is_some();
                        let mic = voice.as_ref().map_or(resume_voice == Some(true), |chat| microphone(chat, port.as_ref(), snapshot.microphone));
                        resume_voice=None;
                        request.voice_controls.audio_test.store(0,Ordering::Release);
                        mic_starts.abort_all();mic_starts=JoinSet::new();
                        microphone_test=None;snapshot.microphone_test=false;snapshot.speaker_test=false;snapshot.microphone_voice_detected=false;snapshot.microphone_level=0.0;
                        let generation = request.voice_controls.generation.fetch_add(1,Ordering::AcqRel)+1;
                        let mut gate=request.voice_controls.port.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        if let Some(old)=&port {old.enable(false);}
                        audio.abort_all(); audio=JoinSet::new(); let previous=voice.take(); snapshot.voice=false;snapshot.voice_pending=false;snapshot.microphone=false;
                        session=replacement;
                        port=match &mut session {Session::Host(l)=>l.take_voice(),Session::Join(l)=>l.take_voice()};
                        gate.clone_from(&port);drop(gate);
                        session.state(*local.borrow());
                        if restart && let Some(port)=&port {
                            let backend=request.audio.clone();let port=port.clone();let options=device_options.clone();let deaf=snapshot.deafened;let controls=request.voice_controls.clone();
                            snapshot.voice_pending=true;
                            audio.spawn(async move {
                                // Exclusive native devices must finish their bounded cleanup
                                // before the replacement opens them; transport polling continues.
                                if let Some(chat)=previous {chat.close().await;}
                                open_voice(backend,port,options,controls).await.map(|chat|Audio {generation,chat,deaf,mic})
                            });
                        }
                    }
                    Ok(None)=>{},
                    Err(error)=>{snapshot.report(Kind::GroupRide,error);if handover.fail(&session,snapshot) {end(snapshot);updates.send_replace(snapshot.clone());return Ok(());}}
                }
            },
            changed=latest_progress.changed()=>{if changed.is_ok(){let p=*latest_progress.borrow_and_update();snapshot.received=p.received;snapshot.total=p.total;
                if p.total>0 && let Some(selection)=&snapshot.selection {
                    let percent=u8::try_from(p.received.min(p.total)*100/p.total).unwrap_or(100);
                    reported.report(&session,selection.revision,Stage::Downloading,percent);
                }}},
            changed=local.changed()=>{if changed.is_ok(){session.state(*local.borrow_and_update());}},
            Ok(()) = choices.changed(), if snapshot.host => {
                let choice = choices.borrow_and_update().clone().unwrap_or_default();
                browse(&session, snapshot, &mut building, choice, shared_choice.as_ref()).await;
            },
            result = building.join_next(), if !building.is_empty() => match result {
                Some(Ok(Ok(preview))) => if let Session::Host(lobby) = &session && snapshot.start.is_none() && let Err(error) = lobby.preview(Some(&preview)) {tracing::debug!(%error, "the preview could not be shown");},
                Some(Ok(Err(error))) => tracing::debug!(%error, "the preview could not be made"),
                _ => {},
            },
            Some(result) = previews.fetching.join_next(), if !previews.fetching.is_empty() => if let Ok((id, preview)) = result {previews.fetched(snapshot, id, preview);},
            result = jobs.join_next(), if !jobs.is_empty() => {
                match result {
                    Some(Ok(Ok(Prepared::Host(mut selected)))) => {
                        selected.selection.assistance=snapshot.assistance;
                        if let Session::Host(l) = &session {l.select(selected.clone()).await.map_err(|e|e.to_string())?;}
                        snapshot.selection = Some(selected.selection); snapshot.course = selected.course; snapshot.workout = selected.workout;
                        snapshot.plan = load_plan(snapshot.workout.as_ref()).await?;
                        snapshot.ready.fill(false); snapshot.phase = Phase::Preparing;
                    }
                    Some(Ok(Ok(Prepared::Join(course,workout)))) => {snapshot.course = course; snapshot.workout_id=workout.as_ref().map_or(String::new(),|p|p.display().to_string()); snapshot.workout = workout; snapshot.plan=load_plan(snapshot.workout.as_ref()).await?; snapshot.phase = Phase::Preparing;}
                    Some(Ok(Err(error))) => {snapshot.activity_error(error);}
                    Some(Err(error)) if !error.is_cancelled() => {snapshot.activity_error(error);}
                    _=>{}
                }
            },
            result = audio.join_next(), if !audio.is_empty() => match result {
                Some(Ok(Ok(Audio {generation,chat,deaf,mic}))) if generation==request.voice_controls.generation.load(Ordering::Acquire) => {
                    let _=chat.volume(snapshot.voice_volume);chat.deafen(deaf);chat.echo_prevention(snapshot.echo_prevention);
                    for (index,muted) in snapshot.muted.iter().enumerate() {if *muted && let Ok(id)=u8::try_from(index) && let Ok(id)=RiderId::new(id) {chat.mute_peer(id,true);}}
                    if mic && !deaf && let Err(error) = chat.microphone(true) {snapshot.report(Kind::GroupVoice,error);}
                    snapshot.microphone = chat.microphone_enabled();
                    voice = Some(chat); snapshot.voice = true;snapshot.voice_pending=false; snapshot.deafened = deaf;
                }
                Some(Ok(Err(error))) => {snapshot.voice_pending=false;snapshot.report(Kind::GroupVoice,error);},
                Some(Err(error)) if !error.is_cancelled() => {snapshot.voice_pending=false;snapshot.report(Kind::GroupVoice,error);},
                _ => snapshot.voice_pending=false,
            },
            result = scans.join_next(), if !scans.is_empty() => match result {
                Some(Ok(Ok(devices))) => snapshot.devices=devices,
                Some(Ok(Err(error))) => snapshot.report(Kind::GroupVoice,error),
                Some(Err(error)) if !error.is_cancelled() => snapshot.report(Kind::GroupVoice,error),
                _ => {},
            },
            result = mic_starts.join_next(), if !mic_starts.is_empty() => match result {
                Some(Ok(Ok((generation,test)))) if generation==request.voice_controls.generation.load(Ordering::Acquire)=>microphone_test=Some(test),
                Some(Ok(Err(error)))=>{snapshot.report(Kind::GroupVoice,error);internal=Some(Command::AudioTest(request.voice_controls.generation.load(Ordering::Acquire),None));},
                Some(Err(error)) if !error.is_cancelled()=>{snapshot.report(Kind::GroupVoice,error);internal=Some(Command::AudioTest(request.voice_controls.generation.load(Ordering::Acquire),None));},
                _=>{},
            },
            _ = ticker.tick() => {
                snapshot.latencies = if snapshot.reconnecting.is_some() || handover.old_lost {
                    [None; orqwa_net::state::MAX_RIDERS]
                } else {
                    match &session { Session::Host(lobby) => lobby.latencies(), Session::Join(lobby) => lobby.latencies() }
                };
                for (id, value) in snapshot.latencies.iter_mut().enumerate() {
                    if snapshot.roster[id].is_none() || snapshot.away[id] { *value = None; }
                }
                // A standby is background work: when it fails, it is prepared again later.
                if let Err(error) = handover.tick(&session,snapshot) {
                    tracing::debug!(%error, "fallback organizer standby failed");
                    if handover.fail(&session,snapshot) {end(snapshot);updates.send_replace(snapshot.clone());return Ok(());}
                }
                if let Some(test)=&mut microphone_test {
                    snapshot.microphone_level=test.level();snapshot.microphone_voice_detected=test.voice_detected();
                    if snapshot.microphone_test {snapshot.microphone_voice_heard |= test.voice_heard();}
                    if !test.running() {
                        if let Some(error)=test.take_error(){snapshot.report(Kind::GroupVoice,error);}
                        internal=Some(Command::AudioTest(request.voice_controls.generation.load(Ordering::Acquire),None));
                    }
                }
                if handover.expired() {
                    snapshot.report(Kind::GroupRide,"The organizer change timed out. The current ride can continue.");
                    if handover.fail(&session,snapshot) {end(snapshot);updates.send_replace(snapshot.clone());return Ok(());}
                }
                if let Some(start) = snapshot.start && std::time::Instant::now()>=start
                    && (!snapshot.late || snapshot.entry.is_some()) {snapshot.phase = Phase::Riding;}
                if let Some(chat) = &voice {snapshot.microphone = microphone(chat, port.as_ref(), snapshot.microphone);}
                snapshot.speaking = voice.as_ref().map_or([false; orqwa_net::state::MAX_RIDERS], VoiceChat::speaking);
                snapshot.output_name = voice.as_ref().map_or_else(String::new, VoiceChat::output_name);
                if snapshot.voice && !snapshot.output_name.is_empty() {
                    let known = listening_for.get_or_insert_with(|| snapshot.output_name.clone());
                    // Another device took over, e.g. headphones plugged in: ask again.
                    snapshot.ask_listening = *known != snapshot.output_name;
                }
                // Without a check, the meter follows the open microphone in voice itself.
                if microphone_test.is_none() {
                    let sending = voice.as_ref().filter(|_| snapshot.microphone);
                    snapshot.microphone_level = sending.map_or(0.0, VoiceChat::microphone_level);
                    snapshot.microphone_voice_detected = sending.is_some() && snapshot.speaking[usize::from(snapshot.me)];
                }
            }
        }
    }
}

async fn download_again(
    session: &Session,
    snapshot: &mut Snapshot,
    jobs: &mut JoinSet<Result<Prepared, String>>,
    progress: &watch::Sender<content::Progress>,
    data: &Path,
) -> Result<(), String> {
    if let (Session::Join(lobby), Some(selection)) = (session, &snapshot.selection) {
        jobs.abort_all();
        while jobs.join_next().await.is_some() {}
        let link = lobby.content_link().ok_or("The group connection ended.")?;
        let selection = selection.clone();
        let data = data.to_owned();
        snapshot.phase = Phase::Downloading;
        snapshot.error.clear();
        snapshot.activity_failed = false;
        let progress = progress.clone();
        jobs.spawn(async move {
            download(&link, &selection, &data, &progress)
                .await
                .map(|(c, w)| Prepared::Join(c, w))
        });
    }
    Ok(())
}

fn end(snapshot: &mut Snapshot) {
    snapshot.fallback_consent = None;
    snapshot.standby_offer = false;
    snapshot.preview = None;
    snapshot.microphone_test = false;
    snapshot.microphone_level = 0.0;
    snapshot.speaker_test = false;
    snapshot.microphone_voice_detected = false;
    snapshot.reconnecting = None;
    snapshot.away = [false; orqwa_net::state::MAX_RIDERS];
    snapshot.speaking = [false; orqwa_net::state::MAX_RIDERS];
    snapshot.voice = false;
    snapshot.voice_pending = false;
    snapshot.microphone = false;
    snapshot.start = None;
    snapshot.phase = Phase::Ended;
    snapshot.states.fill(None);
    snapshot.latencies.fill(None);
    for (index, rider) in snapshot.roster.iter_mut().enumerate() {
        if index != usize::from(snapshot.me) {
            *rider = None;
        }
    }
    snapshot.ready.fill(false);
    snapshot.handover = super::Handover::Idle;
    snapshot.successor = None;
}

async fn start(session: &Session, snapshot: &mut Snapshot) -> Result<(), String> {
    let Session::Host(lobby) = session else {
        return Err("Only the host starts the ride.".into());
    };
    if !snapshot.can_start() {
        return Err("Prepare the selected activity and wait for everyone to be ready.".into());
    }
    let at = lobby
        .start(Duration::from_secs(5))
        .await
        .map_err(|e| e.to_string())?;
    // Freeze the local command owner too; the corresponding lobby event may still be queued.
    snapshot.start = Some(at.into_std());
    snapshot.phase = Phase::Countdown;
    Ok(())
}

async fn change_rules(
    session: &Session,
    snapshot: &mut Snapshot,
    revision: &mut u64,
    rules: Assistance,
    idle: bool,
) -> Result<(), String> {
    if !snapshot.host || snapshot.start.is_some() {
        return Err("Only the organizer can change ride settings before the start.".into());
    }
    if snapshot.assistance == rules {
        return Ok(());
    }
    if idle && let Some(mut selection) = snapshot.selection.clone() {
        let next = revision
            .checked_add(1)
            .ok_or("Activity revision exhausted.")?;
        selection.revision = next;
        selection.assistance = rules;
        let Session::Host(lobby) = session else {
            return Err("Only the organizer can change ride settings.".into());
        };
        // Start may already have committed in the lobby while its event is still queued.
        // Publish local choices only after the authority accepts them, and keep riding on rejection.
        lobby
            .select(SharedActivity {
                selection: selection.clone(),
                course: snapshot.course.clone(),
                workout: snapshot.workout.clone(),
            })
            .await
            .map_err(|e| e.to_string())?;
        *revision = next;
        snapshot.selection = Some(selection);
        snapshot.ready.fill(false);
    }
    // Riders hear of the choices without a selection too, or while one is still being
    // prepared, rather than keep seeing the defaults (#557).
    if let Session::Host(lobby) = session {
        lobby.assist(rules).await.map_err(|e| e.to_string())?;
    }
    snapshot.assistance = rules;
    Ok(())
}

/// The microphone as the rider has it: live while the organizer's link lives, else `was`, how
/// it last was, for a fallback taking over to restore (#426, R188).
fn microphone(chat: &VoiceChat, port: Option<&VoicePort>, was: bool) -> bool {
    // Read before the link: an ended link stays ended, so a microphone found closed on a link
    // still alive afterwards was closed by the rider.
    chat.microphone_enabled() || (was && !port.is_some_and(VoicePort::alive))
}

async fn voice_event(voice: &mut Option<VoiceChat>) -> Option<orqwa_voice::VoiceEvent> {
    if let Some(chat) = voice {
        chat.next().await
    } else {
        std::future::pending().await
    }
}

async fn load_plan(path: Option<&PathBuf>) -> Result<Option<std::sync::Arc<Plan>>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
    Ok(Some(std::sync::Arc::new(
        orqwa_net::workout::decode(&bytes).map_err(|e| e.to_string())?,
    )))
}

async fn chosen(
    course: Option<PathBuf>,
    plan: Option<Plan>,
    directory: PathBuf,
    revision: u64,
) -> Result<SharedActivity, String> {
    let mut name = String::new();
    let (course_id, course_key) = if let Some(path) = &course {
        let file = path.clone();
        let (manifest, key) = tokio::task::spawn_blocking(move || {
            let manifest = orqwa_storage::course::shared::validate(&file)?;
            Ok::<_, orqwa_storage::course::CourseError>((manifest, course_key(&file)?))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        name = manifest.name;
        let id = Source::open(path, ContentKind::Course)
            .await
            .map_err(|e| e.to_string())?
            .id();
        (Some(id), Some(key))
    } else {
        (None, None)
    };
    let (workout, workout_id) = if let Some(plan) = plan {
        if name.is_empty() {
            name.clone_from(&plan.name);
        }
        let bytes = orqwa_net::workout::encode(&plan).map_err(|e| e.to_string())?;
        let id = ContentId::of(ContentKind::Workout, &bytes).map_err(|e| e.to_string())?;
        let path = directory.join(format!("workout-{revision}.oqw"));
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|e| e.to_string())?;
        (Some(path), Some(id))
    } else {
        (None, None)
    };
    while name.len() > 64 {
        name.pop();
    }
    let selection = Selection {
        revision,
        name,
        course: course_id,
        course_key,
        workout: workout_id,
        assistance: Assistance::default(),
    };
    selection.validate().map_err(|e| e.to_string())?;
    Ok(SharedActivity {
        selection,
        course,
        workout,
    })
}

async fn download(
    connection: &orqwa_net::link::Connection,
    selection: &Selection,
    data: &Path,
    progress: &watch::Sender<content::Progress>,
) -> Result<(Option<PathBuf>, Option<PathBuf>), String> {
    let mut files = [None, None];
    for (index, id) in [selection.course, selection.workout]
        .into_iter()
        .enumerate()
    {
        let Some(id) = id else {
            continue;
        };
        let library = data.join(if id.kind() == ContentKind::Course {
            "courses"
        } else {
            "shared-workouts"
        });
        tokio::fs::create_dir_all(&library)
            .await
            .map_err(|e| e.to_string())?;
        let kept = match selection.course_key {
            Some(key) if id.kind() == ContentKind::Course => {
                let library = library.clone();
                tokio::task::spawn_blocking(move || own_course(&library, key))
                    .await
                    .map_err(|e| e.to_string())?
            }
            _ => {
                let path = library.join(id.basename());
                (path.is_file()
                    && Source::open(&path, id.kind())
                        .await
                        .is_ok_and(|s| s.id() == id))
                .then_some(path)
            }
        };
        if let Some(path) = kept {
            files[index] = Some(path);
            continue;
        }
        let received = content::fetch(connection, id, &library, Some(progress))
            .await
            .map_err(|e| e.to_string())?;
        let key = selection.course_key;
        let validated = received
            .validate(move |path| match id.kind() {
                // The sender's own copy may differ in bytes, never in its course (#435).
                ContentKind::Course => {
                    orqwa_storage::course::shared::validate(path).is_ok()
                        && key.is_some_and(|key| course_key(path).is_ok_and(|own| own == key))
                }
                ContentKind::Workout => std::fs::read(path)
                    .is_ok_and(|bytes| orqwa_net::workout::decode(&bytes).is_ok()),
                // A selection never names one; a preview is never stored.
                ContentKind::Preview => false,
            })
            .await
            .map_err(|e| e.to_string())?;
        files[index] = Some(
            validated
                .publish(&library)
                .await
                .map_err(|e| e.to_string())?,
        );
    }
    let [course, workout] = files;
    Ok((course, workout))
}

fn course_key(path: &Path) -> Result<CourseKey, orqwa_storage::course::CourseError> {
    orqwa_storage::course::key_material(path).map(|material| CourseKey::of(&material))
}

/// The course in `library` with `key`, if the rider has it already under whatever name and
/// map picture (#435): riding it needs no download. Reads only each course's zip directory
/// and manifest, so the whole library is looked through every time.
fn own_course(library: &Path, key: CourseKey) -> Option<PathBuf> {
    let mut courses: Vec<PathBuf> = std::fs::read_dir(library)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e == orqwa_storage::course::EXTENSION)
        })
        .collect();
    courses.sort();
    courses
        .into_iter()
        .find(|path| course_key(path).is_ok_and(|own| own == key))
}
