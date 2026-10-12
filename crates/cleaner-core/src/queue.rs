//! Pure queue state and crash-recovery rules; no deletion requests live here.

use std::collections::BTreeMap;
use std::fmt;
use serde::{Deserialize, Serialize};

use crate::identity::{Account, MessageKey};
use crate::selection::ConfirmedBatch;

/// Durable state of an individual message in a confirmed deletion job.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum ItemState {
    /// Not yet attempted and eligible for explicit, checked execution.
    Queued,
    /// An effect may have been sent; the outcome cannot yet be inferred.
    InFlight,
    /// The adapter verified the deletion succeeded.
    Succeeded,
    /// The adapter intentionally did not delete this item.
    Skipped,
    /// The adapter observed a definite error rather than ambiguous completion.
    Failed,
    /// An attempt may have succeeded; blind retries are prohibited.
    UnknownOutcome,
}

/// Result that a transport adapter must explicitly report after an attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum AttemptOutcome {
    /// Slack acknowledged successful deletion of this user-owned message.
    Deleted,
    /// Preflight checks rejected this attempt without sending deletion.
    Skipped,
    /// An observed failure with a known outcome occurred.
    Failed,
    /// Network or browser interruption made the outcome unknowable.
    Unknown,
}

/// Fail-closed reasons a queue transition cannot occur.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum QueueError {
    /// Job is paused; an explicit press of Resume is required.
    Paused,
    /// The authenticated workspace/user differs from the approved batch.
    AccountMismatch,
    /// Message was not among the exact approved keys.
    UnknownMessage,
    /// Transition is invalid for this item's current lifecycle state.
    InvalidTransition,
}

impl fmt::Display for QueueError {
    /// Describes a rejected job transition without exposing message contents.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Paused => f.write_str("deletion queue requires explicit resume"),
            Self::AccountMismatch => f.write_str("current account does not match approved batch"),
            Self::UnknownMessage => f.write_str("message not present in approved batch"),
            Self::InvalidTransition => f.write_str("invalid deletion queue transition"),
        }
    }
}

impl std::error::Error for QueueError {}

/// A deterministic job checkpoint with an immutable approved batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeletionQueue {
    /// Immutable selected IDs and account used to create this job.
    batch: ConfirmedBatch,
    /// Durable outcome for every selected key.
    statuses: BTreeMap<MessageKey, ItemState>,
    /// A volatile, explicit user action; always false after restart.
    running: bool,
}

impl DeletionQueue {
    /// Prepares a new, initially paused job from a confirmed selection.
    #[must_use]
    pub fn new(batch: ConfirmedBatch) -> Self {
        let statuses = batch.keys().iter().cloned().map(|key| (key, ItemState::Queued)).collect();
        Self { batch, statuses, running: false }
    }

    /// Rehydrates a saved job, rejects foreign keys, and fails closed after exit.
    ///
    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    ///
    /// No persisted running flag is accepted: a restored job is always paused,
    /// and interrupted attempts become `UnknownOutcome` before user interaction.
    pub fn restore(
        batch: ConfirmedBatch,
        statuses: BTreeMap<MessageKey, ItemState>,
    ) -> Result<Self, QueueError> {
        if statuses.len() != batch.keys().len()
            || !batch.keys().iter().all(|key| statuses.contains_key(key))
        {
            return Err(QueueError::UnknownMessage);
        }
        let mut queue = Self { batch, statuses, running: false };
        queue.recover_after_interruption();
        Ok(queue)
    }

    /// Returns the approved and immutable message IDs.
    #[must_use]
    pub fn batch(&self) -> &ConfirmedBatch {
        &self.batch
    }

    /// Returns true only after an explicit resume for the matching user.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Returns the last local lifecycle state for an approved key.
    #[must_use]
    pub fn state(&self, key: &MessageKey) -> Option<ItemState> {
        self.statuses.get(key).copied()
    }

    /// Arms this queue only for the exact user/workspace that confirmed it.
    ///
    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    ///
    /// The caller must also verify a fresh browser session before this call.
    pub fn resume(&mut self, active_account: &Account) -> Result<(), QueueError> {
        if active_account != self.batch.account() {
            return Err(QueueError::AccountMismatch);
        }
        self.running = true;
        Ok(())
    }

    /// Stops new attempts without rewriting the outcome of an in-flight item.
    pub fn pause(&mut self) {
        self.running = false;
    }

    /// Marks an approved, queued item in flight *before* the destructive request.
    ///
    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    ///
    /// Adapters must persist this transition before making a Slack request,
    /// and must revalidate authorship before sending it.
    pub fn start(&mut self, key: &MessageKey) -> Result<(), QueueError> {
        if !self.running {
            return Err(QueueError::Paused);
        }
        let state = self.statuses.get_mut(key).ok_or(QueueError::UnknownMessage)?;
        if *state != ItemState::Queued {
            return Err(QueueError::InvalidTransition);
        }
        *state = ItemState::InFlight;
        Ok(())
    }

    /// Finishes only a known in-flight attempt with an explicit result.
    ///
    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    pub fn record_outcome(&mut self, key: &MessageKey, outcome: AttemptOutcome) -> Result<(), QueueError> {
        let state = self.statuses.get_mut(key).ok_or(QueueError::UnknownMessage)?;
        if *state != ItemState::InFlight {
            return Err(QueueError::InvalidTransition);
        }
        *state = match outcome {
            AttemptOutcome::Deleted => ItemState::Succeeded,
            AttemptOutcome::Skipped => ItemState::Skipped,
            AttemptOutcome::Failed => ItemState::Failed,
            AttemptOutcome::Unknown => ItemState::UnknownOutcome,
        };
        Ok(())
    }

    /// Fails closed after an interruption, never auto-resuming uncertain work.
    ///
    /// Returns how many in-flight attempts need reconciliation. The caller
    /// must persist the recovered checkpoint before offering manual Resume.
    pub fn recover_after_interruption(&mut self) -> usize {
        self.pause();
        let mut unresolved = 0_usize;
        for status in self.statuses.values_mut() {
            if *status == ItemState::InFlight {
                *status = ItemState::UnknownOutcome;
                unresolved = unresolved.saturating_add(1);
            }
        }
        unresolved
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{AttemptOutcome, DeletionQueue, ItemState, QueueError};
    use crate::identity::{Account, ConversationId, MessageKey, MessageTs, UserId, WorkspaceId};
    use crate::message::{ConversationKind, Message};
    use crate::selection::Selection;

    /// Creates an approved one-message fixture using actual review rules.
    fn fixture() -> Result<(DeletionQueue, Account, MessageKey), Box<dyn std::error::Error>> {
        let account = Account::new(WorkspaceId::parse("T1")?, UserId::parse("U1")?);
        let key = MessageKey::new(WorkspaceId::parse("T1")?, ConversationId::parse("C1")?, MessageTs::parse("1.000001")?);
        let message = Message::new(key.clone(), UserId::parse("U1")?, ConversationKind::PublicChannel, None, "a".to_owned());
        let mut selection = Selection::new(account.clone());
        assert!(selection.insert(&message)?, "fixture should select its unique message");
        let batch = selection.confirm(1)?;
        Ok((DeletionQueue::new(batch), account, key))
    }

    /// Queues never execute before explicit user action, even with valid keys.
    #[test]
    fn initial_state_is_paused_and_requires_matching_account() -> Result<(), Box<dyn std::error::Error>> {
        let (mut queue, account, key) = fixture()?;
        assert_eq!(queue.start(&key), Err(QueueError::Paused), "new jobs must be paused");
        let another = Account::new(WorkspaceId::parse("T2")?, UserId::parse("U1")?);
        assert_eq!(queue.resume(&another), Err(QueueError::AccountMismatch), "wrong workspace must be rejected");
        assert!(!queue.is_running(), "failed resume must keep queue paused");
        queue.resume(&account)?;
        assert!(queue.is_running(), "matching explicit resume may arm this queue");
        Ok(())
    }

    /// Completed and unknown outcomes cannot be retried by ordinary queue traversal.
    #[test]
    fn terminal_states_cannot_be_restarted() -> Result<(), Box<dyn std::error::Error>> {
        let (mut queue, account, key) = fixture()?;
        queue.resume(&account)?;
        queue.start(&key)?;
        queue.record_outcome(&key, AttemptOutcome::Deleted)?;
        assert_eq!(queue.start(&key), Err(QueueError::InvalidTransition), "success must be terminal");
        assert_eq!(queue.state(&key), Some(ItemState::Succeeded), "successful outcome must be stable");
        Ok(())
    }

    /// Rapid browser exit turns every in-flight attempt into a halted unknown result.
    #[test]
    fn interruption_requires_manual_resume_and_never_blind_retries() -> Result<(), Box<dyn std::error::Error>> {
        let (mut queue, account, key) = fixture()?;
        queue.resume(&account)?;
        queue.start(&key)?;
        assert_eq!(queue.recover_after_interruption(), 1, "in-flight deletion requires reconciliation");
        assert!(!queue.is_running(), "interruption must disarm the queue");
        assert_eq!(queue.start(&key), Err(QueueError::Paused), "recovered job must not auto-run");
        queue.resume(&account)?;
        assert_eq!(queue.start(&key), Err(QueueError::InvalidTransition), "unknown outcome must not be retried");
        assert_eq!(queue.state(&key), Some(ItemState::UnknownOutcome), "unknown result must remain visible");
        assert_eq!(queue.recover_after_interruption(), 0, "recovery must be idempotent");
        Ok(())
    }

    /// Saved checkpoints cannot substitute foreign keys or auto-run after restart.
    #[test]
    fn restored_job_is_paused_and_validates_all_snapshot_keys() -> Result<(), Box<dyn std::error::Error>> {
        let (mut queue, account, key) = fixture()?;
        queue.resume(&account)?;
        queue.start(&key)?;
        let checkpoint = queue.statuses.clone();
        let restored = DeletionQueue::restore(queue.batch().clone(), checkpoint)?;
        assert!(!restored.is_running(), "restoring must never resume a job");
        assert_eq!(restored.state(&key), Some(ItemState::UnknownOutcome), "interrupted attempt is uncertain");
        let mut replacement = BTreeMap::new();
        replacement.insert(
            MessageKey::new(WorkspaceId::parse("T1")?, ConversationId::parse("C1")?, MessageTs::parse("1.000002")?),
            ItemState::Queued,
        );
        assert_eq!(DeletionQueue::restore(queue.batch().clone(), replacement), Err(QueueError::UnknownMessage),
            "a saved checkpoint must contain exactly the approved keys");
        Ok(())
    }

    /// Prevents unapproved keys from ever entering an active deletion queue.
    #[test]
    fn unknown_message_cannot_be_started() -> Result<(), Box<dyn std::error::Error>> {
        let (mut queue, account, _key) = fixture()?;
        let other_key = MessageKey::new(WorkspaceId::parse("T1")?, ConversationId::parse("C1")?, MessageTs::parse("1.000002")?);
        queue.resume(&account)?;
        assert_eq!(queue.start(&other_key), Err(QueueError::UnknownMessage), "unselected message must be rejected");
        Ok(())
    }
}
