//! App-lobby selection, readiness and selected-content serving.

use std::path::PathBuf;

use crate::activity::{ActivityLobby, Participant, Selection};
use crate::content::{self, ContentError, Source};

use super::{LobbyError, MAX_RIDERS, Message, RecvStream, RiderId, SendStream, rider_id, watch};

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

pub(super) struct Activities {
    pub(super) enabled: bool,
    lobby: ActivityLobby,
    members: [Option<Participant>; MAX_RIDERS],
    ready: [bool; MAX_RIDERS],
    pub(super) selected: watch::Sender<Option<SharedActivity>>,
}
impl Activities {
    pub(super) fn new(enabled: bool, organizer: RiderId) -> Result<Self, LobbyError> {
        let mut lobby = ActivityLobby::new(organizer);
        let mut members = [None; MAX_RIDERS];
        members[usize::from(organizer.value())] = Some(lobby.join(organizer)?);
        let (selected, _) = watch::channel(None);
        Ok(Self {
            enabled,
            lobby,
            members,
            ready: [false; MAX_RIDERS],
            selected,
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
        self.selected.send_replace(Some(activity));
        Ok(())
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
        Ok(())
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

pub(super) async fn transfer(
    mut send: SendStream,
    mut recv: RecvStream,
    selected: SharedActivity,
) -> Result<(), ContentError> {
    let id = content::requested(&mut recv).await?;
    let path = if selected.selection.course == Some(id) {
        selected.course
    } else if selected.selection.workout == Some(id) {
        selected.workout
    } else {
        return Err(ContentError::Mismatch);
    }
    .ok_or(ContentError::Mismatch)?;
    let mut source = Source::open(&path, id.kind()).await?;
    if source.id() != id {
        return Err(ContentError::Mismatch);
    }
    content::send(&mut send, &mut source).await
}
