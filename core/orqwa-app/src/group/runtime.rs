//! One owned app group task; content and audio work never run in the trainer loop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use orqwa_domain::workout::Plan;
use orqwa_net::activity::{Assistance, ContentId, ContentKind, Selection};
use orqwa_net::code::{Invite, Reply};
use orqwa_net::content::{self, Source};
use orqwa_net::link::{ConnectOptions, Host, Joiner};
use orqwa_net::lobby::{HostLobby, JoinerLobby, LobbyEvent, RideProgress, Rider, SharedActivity};
use orqwa_net::state::{RiderId, RiderState};
use orqwa_voice::audio_test::{AudioTest, TestKind};
use orqwa_voice::{VoiceChat, device::DeviceOptions};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

use super::{Phase, Snapshot, VoiceControls};
use crate::Kind;
use std::sync::{Arc, atomic::Ordering};

pub(super) enum Command {
    Admit(String),
    Select(Option<PathBuf>, Option<Plan>),
    Assistance(Assistance),
    Prepared(u64),
    Ready(bool),
    Start,
    Enter,
    WorkoutPause(bool),
    WorkoutSkip(Duration),
    Handover(RiderId),
    HandoverAnswer(bool),
    HandoverCancel,
    Remove(RiderId),
    Retry,
    Failure(String),
    Voice(u64, bool, bool, bool, f32),
    Volume(f32),
    Devices,
    ChooseDevices(u64, String, String),
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
enum Prepared {
    Host(SharedActivity),
    Join(Option<PathBuf>, Option<PathBuf>),
}
struct Audio {
    generation: u64,
    chat: VoiceChat,
    deaf: bool,
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
        snapshot.report(Kind::GroupRide, error);
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
        let joiner = Joiner::lobby(invite, &request.options)
            .await
            .map_err(|e| e.to_string())?;
        snapshot.reply = joiner.reply().to_code(policy).map_err(|e| e.to_string())?;
        snapshot.reachability = Some(joiner.reply().reachability());
        updates.send_replace(snapshot.clone());
        let lobby = JoinerLobby::join(joiner, request.rider.clone())
            .await
            .map_err(|e| e.to_string())?;
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
    let mut mic_starts = JoinSet::new();
    let mut resume_voice = false;
    let mut internal = None;
    let mut jobs = JoinSet::new();
    let mut next_revision = 0u64;
    let (progress, mut latest_progress) = watch::channel(content::Progress {
        received: 0,
        total: 0,
    });
    let mut ticker = tokio::time::interval(Duration::from_millis(200));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        updates.send_replace(snapshot.clone());
        tokio::select! {
            event=voice_event(&mut voice)=>{
                match event {
                    Some(orqwa_voice::VoiceEvent::PeerMuted {peer})=>{snapshot.muted[usize::from(peer.value())]=true;snapshot.report(Kind::GroupVoice,"A speaker sent invalid audio and was muted locally.");}
                    Some(orqwa_voice::VoiceEvent::DeviceUnavailable {error})=>{voice=None;snapshot.voice=false;snapshot.microphone=false;snapshot.report(Kind::GroupVoice,error);}
                    Some(orqwa_voice::VoiceEvent::Stopped)|None=>{voice=None;snapshot.voice=false;snapshot.microphone=false;}
                }
            },
            command = async {if let Some(command)=internal.take() {Some(command)} else {commands.recv().await}} => {
                let Some(command) = command else {return Ok(());};
                if handover.busy() && matches!(command, Command::Admit(_) | Command::Remove(_) | Command::WorkoutPause(_) | Command::WorkoutSkip(_) | Command::Ready(_) | Command::Enter) {
                    snapshot.report(Kind::GroupRide,"Wait until the organizer change finishes, or cancel it first."); continue;
                }
                let result = match command {
                    Command::Handover(id) => handover.propose(&session,snapshot,id),
                    Command::HandoverAnswer(accepted) => handover.answer(&session,snapshot,accepted),
                    Command::HandoverCancel => {
                        if snapshot.host || snapshot.successor == Some(snapshot.me) {handover.cancel(&session,snapshot)}
                        else {Err("Only the organizer or selected successor can cancel this request.".into())}
                    }
                    Command::Admit(text) => match &session {Session::Host(l)=>match Reply::from_code(&text,policy) {Ok(reply)=>l.admit(&reply).await.map_err(|e|e.to_string()),Err(e)=>Err(e.to_string())},Session::Join(_)=>Err("Only the host can admit riders.".into())},
                    Command::Select(course,plan) => {
                        if snapshot.start.is_some() || !snapshot.host {continue;}
                        jobs.abort_all(); while jobs.join_next().await.is_some() {}
                        next_revision = next_revision.checked_add(1).ok_or("Activity revision exhausted.")?;
                        let revision = next_revision; let dir = temp.0.clone();
                        snapshot.phase = Phase::Downloading; snapshot.error.clear();snapshot.activity_failed=false; snapshot.prepared = false;
                        jobs.spawn(async move {chosen(course,plan,dir,revision).await.map(Prepared::Host)}); Ok(())
                    }
                    Command::Assistance(rules) => change_rules(&session,snapshot,&mut next_revision,rules,jobs.is_empty()).await,
                    Command::Prepared(revision) => {if snapshot.selection.as_ref().is_some_and(|s|s.revision==revision) {snapshot.prepared = true; snapshot.phase = Phase::Ready;} Ok(())}
                    Command::Ready(ready) => {
                        if !snapshot.prepared {continue;}
                        session.ready(snapshot.selection.as_ref().ok_or("Select an activity first.")?.revision,ready).await
                    }
                    Command::Start => start(&session,snapshot).await,
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
                    Command::Retry => {
                        if let (Session::Join(l),Some(selection)) = (&session,&snapshot.selection) {
                            jobs.abort_all(); while jobs.join_next().await.is_some() {}
                            let link = l.content_link().ok_or("The group connection ended.")?;
                            let selection = selection.clone(); let data = request.data.clone();
                            snapshot.phase = Phase::Downloading; snapshot.error.clear();snapshot.activity_failed=false;
                            let progress=progress.clone();jobs.spawn(async move {download(&link,&selection,&data,&progress).await.map(|(c,w)|Prepared::Join(c,w))});
                        } Ok(())
                    }
                    Command::Failure(error) => {snapshot.activity_error(error); Ok(())}
                    Command::Devices => {
                        if scans.is_empty() {scans.spawn(async {tokio::task::spawn_blocking(||orqwa_voice::device::devices().map(|devices|devices.into_iter().take(32).map(|d|(d.id,d.name,d.input,d.output)).collect::<Vec<_>>()).map_err(|e|e.to_string())).await.map_err(|e|e.to_string())?});} Ok(())
                    }
                    Command::ChooseDevices(generation,input,output) => {
                        if generation!=request.voice_controls.generation.load(Ordering::Acquire){continue;}
                        request.voice_controls.audio_test.store(0,Ordering::Release);
                        let restart = snapshot.voice || snapshot.voice_pending || resume_voice;
                        resume_voice=false;
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
                            let backend=request.audio.clone();let port=port.clone();let options=device_options.clone();let deaf=snapshot.deafened;
                            audio.spawn(async move {
                                if let Some(test)=previous_test {test.close().await;}
                                if let Some(chat)=previous {chat.close().await;}
                                backend.voice(port,options).await.map(|chat|Audio {generation,chat,deaf}).map_err(|e|e.to_string())
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
                    Command::EchoPrevention(enabled)=>{snapshot.echo_prevention=enabled;if let Some(chat)=&voice {chat.echo_prevention(enabled);}Ok(())}
                    Command::AudioTest(generation,kind)=>{
                        if generation!=request.voice_controls.generation.load(Ordering::Acquire){continue;}
                        request.voice_controls.audio_test.store(kind.map_or(0,|kind|kind as u8),Ordering::Release);
                        audio.abort_all();while audio.join_next().await.is_some() {}
                        mic_starts.abort_all();while mic_starts.join_next().await.is_some() {}
                        let previous=voice.take();let previous_test=microphone_test.take();
                        resume_voice=resume_voice || snapshot.voice || snapshot.voice_pending;
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
                        } else if resume_voice && let Some(port)=&port {
                            snapshot.voice_pending=true;
                            let backend=request.audio.clone();let port=port.clone();let options=device_options.clone();let deaf=snapshot.deafened;
                            audio.spawn(async move {
                                if let Some(chat)=previous {chat.close().await;}
                                if let Some(test)=previous_test {test.close().await;}
                                backend.voice(port,options).await.map(|chat|Audio {generation,chat,deaf}).map_err(|error|error.to_string())
                            });
                            resume_voice=false;
                        } Ok(())
                    }
                    Command::Voice(generation,on,mic,deaf,volume) => {
                        if snapshot.microphone_test || snapshot.speaker_test {continue;}
                        // Serialize generation checking and transmission changes with the UI's
                        // immediate mute gate; an older queued unmute cannot reopen it later.
                        let _gate=request.voice_controls.port.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        if generation!=request.voice_controls.generation.load(Ordering::Acquire){continue;}
                        snapshot.voice_volume=volume;
                        if !on {audio.abort_all(); voice = None; snapshot.voice = false; snapshot.voice_pending=false; snapshot.microphone = false; snapshot.deafened = false;}
                        else if let Some(chat) = &voice {
                            let configured = (|| {chat.deafen(deaf); chat.volume(volume).map_err(|e|e.to_string())?;
                            chat.microphone(mic && !deaf).map_err(|e|e.to_string())?; Ok::<(),String>(())})();
                            if let Err(error) = configured {snapshot.report(Kind::GroupVoice,error);}
                            snapshot.microphone = chat.microphone_enabled(); snapshot.deafened = deaf;
                        } else if audio.is_empty() && let Some(port) = &port {
                            snapshot.voice_pending=true;
                            let backend=request.audio.clone();let port = port.clone();let options=device_options.clone(); audio.spawn(async move {backend.voice(port,options).await.map(|chat|Audio {generation,chat,deaf}).map_err(|e|e.to_string())});
                        } Ok(())
                    }
                };
                if let Err(error) = result {snapshot.report(Kind::GroupRide,error);}
            },
            event = session.next(), if !handover.old_lost => match event {
                Some(LobbyEvent::Handover {sender,message}) => {
                    if let Err(error) = handover.message(&session,snapshot,sender,message).await {
                        snapshot.report(Kind::GroupRide,error);
                        if handover.fail(&session,snapshot) {end(snapshot);return Ok(());}
                    }
                }
                Some(LobbyEvent::Joined {id,rider}) => {
                    if snapshot.handover==super::Handover::Committing {continue;}
                    if handover.busy() && handover.fail(&session,snapshot) {end(snapshot);return Ok(());}
                    let index=usize::from(id.value());
                    snapshot.generations[index]=snapshot.generations[index].checked_add(1).ok_or("Rider membership exhausted.")?;
                    snapshot.roster[index] = Some(rider); snapshot.ready[index] = false;snapshot.states[index]=None;
                }
                Some(LobbyEvent::Left {id}) => {if handover.departure(snapshot,id) {continue;}
                    if handover.busy() && handover.fail(&session,snapshot) {end(snapshot);return Ok(());}
                    let index = usize::from(id.value()); snapshot.roster[index] = None; snapshot.ready[index] = false; snapshot.states[index] = None;}
                Some(LobbyEvent::Readiness {id,revision,ready}) => if snapshot.selection.as_ref().is_some_and(|s|s.revision==revision) {snapshot.ready[usize::from(id.value())] = ready;},
                Some(LobbyEvent::Selection(selection)) => {
                    if snapshot.start.is_some() {continue;}
                    if snapshot.selection.as_ref().is_some_and(|s|s.revision==selection.revision) {continue;}
                    let retained=snapshot.prepared && snapshot.selection.as_ref().is_some_and(|s|s.course==selection.course&&s.workout==selection.workout);
                    snapshot.assistance=selection.assistance;
                    if retained && jobs.is_empty() {
                        snapshot.selection=Some(selection);snapshot.ready.fill(false);snapshot.phase=Phase::Ready;continue;
                    }
                    jobs.abort_all(); while jobs.join_next().await.is_some() {}
                    snapshot.selection = Some(selection.clone()); snapshot.ready.fill(false); snapshot.prepared = false; snapshot.error.clear();snapshot.activity_failed=false;
                    snapshot.course = None; snapshot.workout = None; snapshot.plan=None;snapshot.workout_id.clear();
                    if let Session::Join(l) = &session {
                        let link = l.content_link().ok_or("The group connection ended.")?; let data = request.data.clone();
                        snapshot.phase = Phase::Downloading;
                        progress.send_replace(content::Progress {received:0,total:0});let progress=progress.clone();jobs.spawn(async move {download(&link,&selection,&data,&progress).await.map(|(c,w)|Prepared::Join(c,w))});
                    }
                }
                Some(LobbyEvent::Start {at}) => {snapshot.start = Some(at.into_std()); snapshot.phase = Phase::Countdown;}
                Some(LobbyEvent::Running {at}) => {snapshot.start = Some(at.into_std()); snapshot.late = true;}
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
                Some(LobbyEvent::Expired) => snapshot.report(Kind::GroupRide,"An admission reply expired. Exchange fresh codes."),
                Some(LobbyEvent::HostLeft)|None => {if !handover.lost() {end(snapshot);updates.send_replace(snapshot.clone());return Ok(());}}
            },
            event = handover.next() => {
                match handover.event(&session,snapshot,event) {
                    Ok(Some(replacement)) => {
                        // Close transmission synchronously before replacing any audio owner. Keep
                        // listening consent, but require a fresh microphone action on the new link.
                        let restart = snapshot.voice || snapshot.voice_pending || resume_voice;
                        resume_voice=false;
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
                            let backend=request.audio.clone();let port=port.clone();let options=device_options.clone();let deaf=snapshot.deafened;
                            snapshot.voice_pending=true;
                            audio.spawn(async move {
                                // Exclusive native devices must finish their bounded cleanup
                                // before the replacement opens them; transport polling continues.
                                if let Some(chat)=previous {chat.close().await;}
                                backend.voice(port,options).await.map(|chat|Audio {generation,chat,deaf}).map_err(|error|error.to_string())
                            });
                        }
                    }
                    Ok(None)=>{},
                    Err(error)=>{snapshot.report(Kind::GroupRide,error);if handover.fail(&session,snapshot) {end(snapshot);updates.send_replace(snapshot.clone());return Ok(());}}
                }
            },
            changed=latest_progress.changed()=>{if changed.is_ok(){let p=*latest_progress.borrow_and_update();snapshot.received=p.received;snapshot.total=p.total;}},
            changed=local.changed()=>{if changed.is_ok(){session.state(*local.borrow_and_update());}},
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
                Some(Ok(Ok(Audio {generation,chat,deaf}))) if generation==request.voice_controls.generation.load(Ordering::Acquire) => {
                    let _=chat.volume(snapshot.voice_volume);chat.deafen(deaf);chat.echo_prevention(snapshot.echo_prevention);
                    for (index,muted) in snapshot.muted.iter().enumerate() {if *muted && let Ok(id)=u8::try_from(index) && let Ok(id)=RiderId::new(id) {chat.mute_peer(id,true);}}
                    voice = Some(chat); snapshot.voice = true;snapshot.voice_pending=false; snapshot.microphone = false; snapshot.deafened = deaf;
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
                if let Some(chat) = &voice {snapshot.microphone = chat.microphone_enabled();}
            }
        }
    }
}

fn end(snapshot: &mut Snapshot) {
    snapshot.microphone_test = false;
    snapshot.microphone_level = 0.0;
    snapshot.speaker_test = false;
    snapshot.microphone_voice_detected = false;
    snapshot.voice = false;
    snapshot.voice_pending = false;
    snapshot.microphone = false;
    snapshot.start = None;
    snapshot.phase = Phase::Ended;
    snapshot.states.fill(None);
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
    snapshot.assistance = rules;
    Ok(())
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
    let course_id = if let Some(path) = &course {
        let file = path.clone();
        let manifest =
            tokio::task::spawn_blocking(move || orqwa_storage::course::shared::validate(&file))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
        name = manifest.name;
        Some(
            Source::open(path, ContentKind::Course)
                .await
                .map_err(|e| e.to_string())?
                .id(),
        )
    } else {
        None
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
        let path = library.join(id.basename());
        if path.is_file()
            && Source::open(&path, id.kind())
                .await
                .is_ok_and(|s| s.id() == id)
        {
            files[index] = Some(path);
            continue;
        }
        let received = content::fetch(connection, id, &library, Some(progress))
            .await
            .map_err(|e| e.to_string())?;
        let validated = received
            .validate(move |path| match id.kind() {
                ContentKind::Course => orqwa_storage::course::shared::validate(path).is_ok(),
                ContentKind::Workout => std::fs::read(path)
                    .is_ok_and(|bytes| orqwa_net::workout::decode(&bytes).is_ok()),
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
