//! Black-box tests of user approval, message ownership, and restart recovery.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cleaner_core::{
        identity::{Account, ConversationId, MessageKey, MessageTs, UserId, WorkspaceId},
        message::{ConversationKind, Message, ReviewFilter, ThreadFilter},
        queue::{AttemptOutcome, DeletionQueue, ItemState, QueueError},
        selection::{Selection, SelectionError},
    };

    // ---------------------------------------------------------------------------
    // Fixtures
    // ---------------------------------------------------------------------------

    fn message(conversation: &str, timestamp: &str, author: &str) -> Result<Message, Box<dyn std::error::Error>> {
        let key = MessageKey::new(
            WorkspaceId::parse("T1")?,
            ConversationId::parse(conversation)?,
            MessageTs::parse(timestamp)?,
        );
        Ok(Message::new(
            key,
            UserId::parse(author)?,
            ConversationKind::DirectMessage,
            None,
            "example".to_owned(),
        ))
    }

    /// Complete account and stable keys used to test interrupted job recovery.
    struct CheckpointFixture {
        /// The restored, always-paused queue.
        recovered: DeletionQueue,
        /// Account used to approve and manually resume the job.
        account: Account,
        /// Identity of the interrupted attempt.
        first_key: MessageKey,
        /// Identity of the still-queued message.
        second_key: MessageKey,
    }

    /// Constructs a valid but interrupted job checkpoint for restart-safety tests.
    fn interrupted_queue_fixture() -> Result<CheckpointFixture, Box<dyn std::error::Error>> {
        let account = Account::new(WorkspaceId::parse("T1")?, UserId::parse("U1")?);
        let first = message("D1", "1.000001", "U1")?;
        let second = message("D2", "1.000002", "U1")?;
        let mut selection = Selection::new(account.clone());
        assert!(selection.insert(&first)?, "first message should be newly selected");
        assert!(selection.insert(&second)?, "second message should be newly selected");
        let batch = selection.confirm(2)?;
        let mut queue = DeletionQueue::new(batch.clone());
        assert!(!queue.is_running(), "new queues are paused");
        queue.resume(&account)?;
        queue.start(first.key())?;
        let mut snapshot = BTreeMap::new();
        snapshot.insert(first.key().clone(), ItemState::InFlight);
        snapshot.insert(second.key().clone(), ItemState::Queued);
        let recovered = DeletionQueue::restore(batch, snapshot)?;
        Ok(CheckpointFixture {
            recovered,
            account,
            first_key: first.key().clone(),
            second_key: second.key().clone(),
        })
    }

    // ---------------------------------------------------------------------------
    // Safety contracts
    // ---------------------------------------------------------------------------

    #[test]
    fn bulk_review_never_selects_mixed_author_results() -> Result<(), Box<dyn std::error::Error>> {
        let account = Account::new(WorkspaceId::parse("T1")?, UserId::parse("U1")?);
        let own = message("D1", "1.000001", "U1")?;
        let foreign = message("D1", "1.000002", "U2")?;
        let filter = ReviewFilter {
            kind: Some(ConversationKind::DirectMessage),
            conversation: None,
            thread: ThreadFilter::All,
            query: None,
        };
        let mut selection = Selection::new(account);
        assert_eq!(
            selection.insert_filtered(&[own.clone(), foreign], &filter),
            Err(SelectionError::AuthorMismatch),
            "foreign messages must prevent partial selections"
        );
        assert!(selection.is_empty(), "rejected bulk selection must not mutate state");
        assert_eq!(
            selection.insert_filtered(&[own], &filter),
            Ok(1),
            "valid own messages may be approved"
        );
        assert_eq!(
            selection.confirm(0),
            Err(SelectionError::ConfirmationCountMismatch),
            "incorrect confirmation count must reject deletion"
        );
        Ok(())
    }

    #[test]
    fn saved_queue_restores_paused_and_never_retries_ambiguous_messages() -> Result<(), Box<dyn std::error::Error>> {
        let CheckpointFixture {
            mut recovered,
            account,
            first_key,
            second_key,
        } = interrupted_queue_fixture()?;
        assert!(!recovered.is_running(), "restored queues must never auto resume");
        assert_eq!(
            recovered.state(&first_key),
            Some(ItemState::UnknownOutcome),
            "in-flight deletion becomes unknown"
        );
        assert_eq!(
            recovered.start(&second_key),
            Err(QueueError::Paused),
            "explicit user resume is required"
        );
        recovered.resume(&account)?;
        assert_eq!(
            recovered.start(&first_key),
            Err(QueueError::InvalidTransition),
            "ambiguous deletions cannot be blindly retried"
        );
        recovered.start(&second_key)?;
        recovered.record_outcome(&second_key, AttemptOutcome::Skipped)?;
        assert_eq!(
            recovered.state(&second_key),
            Some(ItemState::Skipped),
            "known outcomes are preserved"
        );
        Ok(())
    }
}
