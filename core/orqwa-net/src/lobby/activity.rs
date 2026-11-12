//! App-lobby selection, readiness, previews and selected-content serving.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use crate::activity::{ActivityLobby, Assistance, ContentId, ContentKind, Participant, Selection};
use crate::content::{self, ContentError, Source};

use super::{LobbyError, MAX_RIDERS, Message, RecvStream, RiderId, SendStream, rider_id, watch};

/// Riders hear of a new preview at most this often, however fast the organizer browses.
const PREVIEW_EVERY: Duration = Duration::from_millis(500);

/// A host-selected descriptor and its two locally owned source paths. Paths never go on wire.
#[derive(Clone, Debug)]
pub struct SharedActivity {
    /// The content identities and shared rules advertised to participants.
    pub selection: Selection,
    /// Course source, present exactly when a course is selected.
    pub course: Option<PathBuf>,
    /// Normalized workout source, present exactly when a workout is selected.
    pub workout: Option<PathBuf>,
}
impl SharedActivity {
    fn validate(&self) -> Result<(), LobbyError> {
        self.selection.validate()?;
        if self.selection.course.is_some() != self.course.is_some()
            || self.selection.workout.is_some() != self.workout.is_some()
        {
            return Err(LobbyError::Protocol);
        }
        Ok(())
    }
}

/// What the organizer is choosing (#418), served from memory to riders who ask for it.
#[derive(Clone, Debug)]
pub(crate) struct Shown {
    pub(crate) id: ContentId,
    pub(crate) bytes: Arc<[u8]>,
}

pub(super) struct Activities {
    pub(super) enabled: bool,
    lobby: ActivityLobby,
    members: [Option<Participant>; MAX_RIDERS],
    ready: [bool; MAX_RIDERS],
    pub(super) selected: watch::Sender<Option<SharedActivity>>,
    pub(super) shown: watch::Sender<Option<Shown>>,
    /// The preview riders last heard of, and when.
    announced: (Option<ContentId>, Option<Instant>),
    /// The organizer's latest drafting and keep-together choices (#557).
    assistance: Assistance,
    started: bool,
    ended_revision: u64,
}
impl Activities {
    pub(super) fn new(enabled: bool, organizer: RiderId) -> Result<Self, LobbyError> {
        let mut lobby = ActivityLobby::new(organizer);
        let mut members = [None; MAX_RIDERS];
        members[usize::from(organizer.value())] = Some(lobby.join(organizer)?);
        let (selected, _) = watch::channel(None);
        let (shown, _) = watch::channel(None);
        Ok(Self {
            enabled,
            lobby,
            members,
            ready: [false; MAX_RIDERS],
            selected,
            shown,
            announced: (None, None),
            assistance: Assistance::default(),
            started: false,
            ended_revision: 0,
        })
    }
    pub(super) fn joined(&mut self, id: RiderId) -> Result<(), LobbyError> {
        let slot = usize::from(id.value());
        self.members[slot] = Some(self.lobby.join(id)?);
        self.ready[slot] = false;
        Ok(())
    }
    pub(super) fn left(&mut self, id: RiderId) {
        let slot = usize::from(id.value());
        if let Some(member) = self.members[slot].take() {
            self.lobby.leave(member);
        }
        self.ready[slot] = false;
    }
    pub(super) fn select(&mut self, activity: SharedActivity) -> Result<(), LobbyError> {
        if !self.enabled {
            return Err(LobbyError::Protocol);
        }
        activity.validate()?;
        self.lobby.select(activity.selection.clone())?;
        self.ready.fill(false);
        self.assistance = activity.selection.assistance;
        self.selected.send_replace(Some(activity));
        Ok(())
    }
    /// Takes the organizer's choices before the countdown; the control to send riders if
    /// they changed. A selection carries them too, so riders hear of them without one (#557).
    ///
    /// # Errors
    /// Returns [`LobbyError::Protocol`] outside an app lobby or from the countdown on.
    pub(super) fn assist(&mut self, rules: Assistance) -> Result<Option<Message>, LobbyError> {
        if !self.enabled || self.started {
            return Err(LobbyError::Protocol);
        }
        if rules == self.assistance {
            return Ok(None);
        }
        self.assistance = rules;
        Ok(Some(Message::Assistance(rules)))
    }
    pub(super) fn ready(
        &mut self,
        id: RiderId,
        revision: u64,
        ready: bool,
    ) -> Result<(), LobbyError> {
        if !self.enabled {
            return Err(LobbyError::Protocol);
        }
        let slot = usize::from(id.value());
        self.lobby.ready(
            self.members[slot].ok_or(LobbyError::Protocol)?,
            revision,
            ready,
        )?;
        self.ready[slot] = ready;
        Ok(())
    }
    pub(super) fn start(&mut self) -> Result<(), LobbyError> {
        if self.enabled {
            self.lobby.start()?;
        }
        // The activity is frozen: nothing is left to choose, so nothing to preview.
        self.started = true;
        self.shown.send_replace(None);
        self.announced = (None, None);
        Ok(())
    }
    pub(super) fn end(&mut self, revision: u64) -> Result<(), LobbyError> {
        if !self.enabled
            || !self.started
            || self
                .lobby
                .selection()
                .is_none_or(|s| s.revision != revision)
        {
            return Err(LobbyError::Protocol);
        }
        self.lobby.end();
        self.started = false;
        self.ended_revision = revision;
        self.ready.fill(false);
        self.selected.send_replace(None);
        self.shown.send_replace(None);
        self.announced = (None, None);
        Ok(())
    }
    /// Takes the organizer's latest preview, until the countdown.
    pub(super) fn show(&mut self, shown: Option<Shown>) {
        if self.enabled && !self.started {
            self.shown.send_replace(shown);
        }
    }
    /// Tells riders of a changed preview, unless they heard of one within [`PREVIEW_EVERY`].
    pub(super) fn announcement(&mut self, now: Instant) -> Option<Message> {
        let current = self.shown.borrow().as_ref().map(|shown| shown.id);
        if current == self.announced.0
            || self.announced.1.is_some_and(|at| now < at + PREVIEW_EVERY)
        {
            return None;
        }
        self.announced = (current, Some(now));
        Some(Message::Preview(current))
    }
    pub(super) fn can_enter(&self, id: RiderId, revision: u64) -> bool {
        self.enabled
            && self.ready[usize::from(id.value())]
            && self
                .lobby
                .selection()
                .is_some_and(|selected| selected.revision == revision)
    }
    /// Whether `id` may tell the group how far it has prepared `revision` (#419): a member
    /// not yet ready, for the current selection.
    pub(super) fn preparing(&self, id: RiderId, revision: u64) -> bool {
        let slot = usize::from(id.value());
        self.enabled
            && self.members[slot].is_some()
            && !self.ready[slot]
            && self
                .lobby
                .selection()
                .is_some_and(|selected| selected.revision == revision)
    }
    pub(super) fn initial(&self) -> Vec<Message> {
        let mut messages = Vec::new();
        if self.ended_revision > 0 {
            messages.push(Message::Lobby(self.ended_revision));
        }
        if let Some(shown) = self.shown.borrow().as_ref() {
            messages.push(Message::Preview(Some(shown.id)));
        }
        // A newcomer assumes the defaults; any selection below repeats the same choices.
        if self.enabled && self.assistance != Assistance::default() {
            messages.push(Message::Assistance(self.assistance));
        }
        if let Some(selected) = self.lobby.selection() {
            messages.push(Message::Selection(selected.clone()));
            for (id, ready) in self.ready.iter().enumerate() {
                if *ready {
                    messages.push(Message::Readiness(rider_id(id), selected.revision, true));
                }
            }
        }
        messages
    }
}

/// Serves one request for the selected activity or the preview, cancelled when that changes.
pub(super) async fn transfer(
    mut send: SendStream,
    mut recv: RecvStream,
    mut selected: watch::Receiver<Option<SharedActivity>>,
    mut shown: watch::Receiver<Option<Shown>>,
) -> Result<(), ContentError> {
    let id = content::requested(&mut recv).await?;
    if id.kind() == ContentKind::Preview {
        let preview = shown
            .borrow_and_update()
            .clone()
            .filter(|preview| preview.id == id)
            .ok_or(ContentError::Mismatch)?;
        return tokio::select! {
            result = content::send_preview(&mut send, id, &preview.bytes) => result,
            _ = shown.changed() => Err(ContentError::Interrupted),
        };
    }
    let current = selected
        .borrow_and_update()
        .clone()
        .ok_or(ContentError::Mismatch)?;
    tokio::select! {
        result = activity(send, id, current) => result,
        _ = selected.changed() => Err(ContentError::Interrupted),
    }
}

async fn activity(
    mut send: SendStream,
    id: ContentId,
    selected: SharedActivity,
) -> Result<(), ContentError> {
    let path = if selected.selection.course == Some(id) {
        selected.course
    } else if selected.selection.workout == Some(id) {
        selected.workout
    } else {
        return Err(ContentError::Mismatch);
    }
    .ok_or(ContentError::Mismatch)?;
    let mut source = Source::open(&path, id.kind()).await?;
    // A course goes as this rider's own copy, which may be renamed or show another map
    // picture than the one selected (#435); the receiver checks its course key.
    if id.kind() == ContentKind::Workout && source.id() != id {
        return Err(ContentError::Mismatch);
    }
    content::send(&mut send, &mut source).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn told(messages: &[Message]) -> Option<Assistance> {
        messages.iter().find_map(|message| match message {
            Message::Assistance(rules) => Some(*rules),
            _ => None,
        })
    }

    #[test]
    fn riders_and_newcomers_hear_the_organizers_assistance_before_anything_is_selected() {
        let mut activities = Activities::new(true, RiderId::HOST).unwrap();
        assert_eq!(
            told(&activities.initial()),
            None,
            "the defaults need no control"
        );
        let off = Assistance {
            drafting: false,
            keep_together: false,
        };
        assert!(matches!(
            activities.assist(off),
            Ok(Some(Message::Assistance(rules))) if rules == off
        ));
        assert!(
            matches!(activities.assist(off), Ok(None)),
            "nothing changed"
        );
        assert_eq!(told(&activities.initial()), Some(off));
        assert!(
            Activities::new(false, RiderId::HOST)
                .unwrap()
                .assist(off)
                .is_err(),
            "course lobbies have no choices to share"
        );
    }
}
