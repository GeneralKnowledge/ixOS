//! The guided session: an exercise followed one card at a time, for an
//! owner holding a lead and a treat. A short "get ready" card comes first,
//! then each step on its own, then the outcome choice.
//!
//! A session lives only in memory. Nothing is saved until the owner picks an
//! outcome on the last card, and then it is recorded exactly as the exercise
//! screen records (`AppCore::record_session`). Closing the session part way,
//! or the app being terminated, records nothing; the next session starts
//! from the "get ready" card again.

use std::sync::Mutex;

use forge_runtime::lock;

use crate::model::LinkedText;
use crate::AppError;

/// What a card of a guided session shows.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GuidedCardKind {
    /// The preparation, before the first step.
    GetReady,
    /// One step of the exercise.
    Step,
    /// After the last step: how did it go?
    Outcome,
}

/// The card on screen, and what the owner may do from it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct GuidedCard {
    pub kind: GuidedCardKind,
    /// Where this card is, from 0 (get ready) to `card_count - 1` (outcome).
    pub position: u32,
    pub card_count: u32,
    /// On a step card, its number from 1 ("Step 2 of 5").
    pub step_number: Option<u32>,
    pub step_count: u32,
    /// The preparation, the step, or (on the outcome card) what to look for.
    pub text: LinkedText,
    pub can_go_back: bool,
    pub can_go_next: bool,
    /// Only on the outcome card, and only until the session is recorded.
    pub can_record: bool,
    pub recorded: bool,
}

/// Where a session is. Plain data, so the rules are tested on their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GuidedState {
    pub step_count: u32,
    pub position: u32,
    pub status: GuidedStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuidedStatus {
    Open,
    /// A record is being saved: the session can't move or be recorded again.
    Recording,
    Recorded,
}

impl GuidedState {
    pub fn new(step_count: u32) -> Self {
        Self {
            step_count,
            position: 0,
            status: GuidedStatus::Open,
        }
    }

    /// Get ready, the steps, and the outcome card.
    pub fn card_count(&self) -> u32 {
        self.step_count + 2
    }

    fn outcome_position(&self) -> u32 {
        self.step_count + 1
    }

    pub fn kind(&self) -> GuidedCardKind {
        match self.position {
            0 => GuidedCardKind::GetReady,
            p if p == self.outcome_position() => GuidedCardKind::Outcome,
            _ => GuidedCardKind::Step,
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.status == GuidedStatus::Open && self.position > 0
    }

    pub fn can_go_next(&self) -> bool {
        self.status == GuidedStatus::Open && self.position < self.outcome_position()
    }

    pub fn can_record(&self) -> bool {
        self.status == GuidedStatus::Open && self.kind() == GuidedCardKind::Outcome
    }

    pub fn next(&mut self) -> Result<(), AppError> {
        if !self.can_go_next() {
            return Err(self.refusal("there is no next card"));
        }
        self.position += 1;
        Ok(())
    }

    pub fn back(&mut self) -> Result<(), AppError> {
        if !self.can_go_back() {
            return Err(self.refusal("there is no earlier card"));
        }
        self.position -= 1;
        Ok(())
    }

    /// Claims the session for one record. Refused before the outcome card,
    /// while a record is being saved, and once recorded.
    pub fn begin_recording(&mut self) -> Result<(), AppError> {
        if !self.can_record() {
            return Err(self.refusal("the session can only be recorded once, after the last step"));
        }
        self.status = GuidedStatus::Recording;
        Ok(())
    }

    /// Ends a record started with `begin_recording`. A failed save leaves the
    /// session on the outcome card, so the owner can try again.
    pub fn finish_recording(&mut self, saved: bool) {
        if self.status == GuidedStatus::Recording {
            self.status = if saved {
                GuidedStatus::Recorded
            } else {
                GuidedStatus::Open
            };
        }
    }

    fn refusal(&self, reason: &str) -> AppError {
        let reason = match self.status {
            GuidedStatus::Open => reason,
            GuidedStatus::Recording => "the session is being recorded",
            GuidedStatus::Recorded => "the session has been recorded",
        };
        AppError::InvalidInput {
            reason: reason.into(),
        }
    }
}

/// One guided session of one exercise. Made by
/// `AppCore::start_guided_session`, recorded by
/// `AppCore::record_guided_session`. Thread-safe.
#[derive(uniffi::Object)]
pub struct GuidedSession {
    pub(crate) exercise_id: String,
    pub(crate) activity_id: Option<u64>,
    title: String,
    preparation: LinkedText,
    steps: Vec<LinkedText>,
    look_for: LinkedText,
    if_difficult: LinkedText,
    safety_note: Option<LinkedText>,
    pub(crate) state: Mutex<GuidedState>,
}

impl GuidedSession {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        exercise_id: String,
        activity_id: Option<u64>,
        title: String,
        preparation: LinkedText,
        steps: Vec<LinkedText>,
        look_for: LinkedText,
        if_difficult: LinkedText,
        safety_note: Option<LinkedText>,
    ) -> Self {
        let state = GuidedState::new(steps.len() as u32);
        Self {
            exercise_id,
            activity_id,
            title,
            preparation,
            steps,
            look_for,
            if_difficult,
            safety_note,
            state: Mutex::new(state),
        }
    }

    fn card_for(&self, state: &GuidedState) -> GuidedCard {
        let kind = state.kind();
        let (text, step_number) = match kind {
            GuidedCardKind::GetReady => (self.preparation.clone(), None),
            GuidedCardKind::Step => (
                self.steps[(state.position - 1) as usize].clone(),
                Some(state.position),
            ),
            GuidedCardKind::Outcome => (self.look_for.clone(), None),
        };
        GuidedCard {
            kind,
            position: state.position,
            card_count: state.card_count(),
            step_number,
            step_count: state.step_count,
            text,
            can_go_back: state.can_go_back(),
            can_go_next: state.can_go_next(),
            can_record: state.can_record(),
            recorded: state.status == GuidedStatus::Recorded,
        }
    }

    fn change(
        &self,
        apply: impl FnOnce(&mut GuidedState) -> Result<(), AppError>,
    ) -> Result<GuidedCard, AppError> {
        let mut state = lock(&self.state);
        apply(&mut state)?;
        Ok(self.card_for(&state))
    }
}

#[uniffi::export]
impl GuidedSession {
    pub fn exercise_id(&self) -> String {
        self.exercise_id.clone()
    }

    /// The plan activity this session belongs to; `None` for extra practice.
    pub fn activity_id(&self) -> Option<u64> {
        self.activity_id
    }

    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// "If it's difficult", offered on every card without moving.
    pub fn if_difficult(&self) -> LinkedText {
        self.if_difficult.clone()
    }

    /// Shown on the "get ready" card when the exercise has one.
    pub fn safety_note(&self) -> Option<LinkedText> {
        self.safety_note.clone()
    }

    /// The card showing now.
    pub fn card(&self) -> GuidedCard {
        self.card_for(&lock(&self.state))
    }

    /// Moves to the next card. `InvalidInput` on the outcome card or once
    /// the session is recorded.
    pub fn next(&self) -> Result<GuidedCard, AppError> {
        self.change(GuidedState::next)
    }

    /// Moves to the card before. `InvalidInput` on the "get ready" card or
    /// once the session is recorded.
    pub fn back(&self) -> Result<GuidedCard, AppError> {
        self.change(GuidedState::back)
    }
}

/// Holds a session's record claim and gives it back if the save fails or the
/// caller stops waiting (a cancelled Swift task drops the future).
pub(crate) struct RecordingClaim<'a> {
    session: &'a GuidedSession,
    saved: bool,
}

impl<'a> RecordingClaim<'a> {
    pub fn take(session: &'a GuidedSession) -> Result<Self, AppError> {
        lock(&session.state).begin_recording()?;
        Ok(Self {
            session,
            saved: false,
        })
    }

    pub fn saved(mut self) {
        self.saved = true;
    }
}

impl Drop for RecordingClaim<'_> {
    fn drop(&mut self) {
        lock(&self.session.state).finish_recording(self.saved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_run_from_get_ready_through_each_step_to_the_outcome() {
        let mut s = GuidedState::new(3);
        assert_eq!(s.card_count(), 5);
        assert_eq!(s.kind(), GuidedCardKind::GetReady);
        assert!(!s.can_go_back() && s.can_go_next() && !s.can_record());
        assert!(s.back().is_err(), "nothing before get ready");
        for position in 1..=3 {
            s.next().unwrap();
            assert_eq!((s.kind(), s.position), (GuidedCardKind::Step, position));
            assert!(!s.can_record(), "no record before the last step");
        }
        s.next().unwrap();
        assert_eq!(s.kind(), GuidedCardKind::Outcome);
        assert!(s.can_record() && s.can_go_back() && !s.can_go_next());
        assert!(s.next().is_err(), "nothing after the outcome");
        s.back().unwrap();
        assert_eq!((s.kind(), s.position), (GuidedCardKind::Step, 3));
    }

    #[test]
    fn an_exercise_without_steps_goes_from_get_ready_to_the_outcome() {
        let mut s = GuidedState::new(0);
        s.next().unwrap();
        assert_eq!(s.kind(), GuidedCardKind::Outcome);
    }

    #[test]
    fn a_session_is_recorded_once_and_then_stays_put() {
        let mut s = GuidedState::new(1);
        assert!(s.begin_recording().is_err(), "not on the get ready card");
        s.next().unwrap();
        assert!(s.begin_recording().is_err(), "not on a step");
        s.next().unwrap();
        s.begin_recording().unwrap();
        assert!(s.begin_recording().is_err(), "not twice at once");
        assert!(s.back().is_err(), "no moving while saving");
        s.finish_recording(false);
        assert!(s.can_record(), "a failed save can be tried again");
        s.begin_recording().unwrap();
        s.finish_recording(true);
        assert_eq!(s.status, GuidedStatus::Recorded);
        assert!(s.begin_recording().is_err());
        assert!(s.back().is_err() && s.next().is_err());
        assert!(!s.can_go_back() && !s.can_go_next() && !s.can_record());
    }
}
