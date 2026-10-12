//! Black-box tests of user approval, message ownership, and restart recovery.

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
    let mut recovered = DeletionQueue::restore(batch, snapshot)?;
    assert!(!recovered.is_running(), "restored queues must never auto resume");
    assert_eq!(
        recovered.state(first.key()),
        Some(ItemState::UnknownOutcome),
        "in-flight deletion becomes unknown"
    );
    assert_eq!(
        recovered.start(second.key()),
        Err(QueueError::Paused),
        "explicit user resume is required"
    );
    recovered.resume(&account)?;
    assert_eq!(
        recovered.start(first.key()),
        Err(QueueError::InvalidTransition),
        "ambiguous deletions cannot be blindly retried"
    );
    recovered.start(second.key())?;
    recovered.record_outcome(second.key(), AttemptOutcome::Skipped)?;
    assert_eq!(
        recovered.state(second.key()),
        Some(ItemState::Skipped),
        "known outcomes are preserved"
    );
    Ok(())
}
