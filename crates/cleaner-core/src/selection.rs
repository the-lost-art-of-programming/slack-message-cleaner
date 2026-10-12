//! Verified local selection and exact-count confirmation of immutable batches.

use std::collections::BTreeSet;
use std::fmt;
use serde::{Deserialize, Serialize};

use crate::identity::{Account, MessageKey};
use crate::message::{Message, ReviewFilter};

/// Reasons an item or batch cannot advance from review into a confirmed queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum SelectionError {
    /// The discovered message does not belong to the active workspace.
    WorkspaceMismatch,
    /// The discovered message is not authored by the active account.
    AuthorMismatch,
    /// A destructive batch cannot contain zero selected records.
    EmptySelection,
    /// The user's explicit confirmation count differs from the snapshot count.
    ConfirmationCountMismatch,
}

impl fmt::Display for SelectionError {
    /// Describes a failed confirmation without exposing message content.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkspaceMismatch => f.write_str("message belongs to another workspace"),
            Self::AuthorMismatch => f.write_str("message is not authored by the active user"),
            Self::EmptySelection => f.write_str("no messages have been selected"),
            Self::ConfirmationCountMismatch => f.write_str("confirmation count does not match selection"),
        }
    }
}

impl std::error::Error for SelectionError {}

/// A mutable review selection for exactly one authenticated account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    /// Account against which all inserts are checked.
    account: Account,
    /// Ordered, deduplicated stable message keys.
    keys: BTreeSet<MessageKey>,
}

impl Selection {
    /// Starts a review selection with nothing implicitly selected.
    #[must_use]
    pub fn new(account: Account) -> Self {
        Self { account, keys: BTreeSet::new() }
    }

    /// Adds a discovered message only when author and workspace match exactly.
    ///
    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    ///
    /// This is a review check, not a replacement for authorization immediately
    /// before sending a deletion request.
    pub fn insert(&mut self, message: &Message) -> Result<bool, SelectionError> {
        self.validate(message)?;
        Ok(self.keys.insert(message.key().clone()))
    }

    /// Adds only messages matching the visible filter, atomically validating ownership.
    ///
    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    ///
    /// A mixed-author or cross-workspace result set causes *zero* new selections,
    /// rather than leaving a partly approved "select all filtered" operation.
    pub fn insert_filtered(
        &mut self,
        messages: &[Message],
        filter: &ReviewFilter,
    ) -> Result<usize, SelectionError> {
        for message in messages.iter().filter(|message| filter.matches(message)) {
            self.validate(message)?;
        }
        let mut added = 0_usize;
        for message in messages.iter().filter(|message| filter.matches(message)) {
            if self.keys.insert(message.key().clone()) {
                added = added.saturating_add(1);
            }
        }
        Ok(added)
    }

    /// Removes one selected key without mutating any confirmed batch snapshot.
    pub fn remove(&mut self, key: &MessageKey) -> bool {
        self.keys.remove(key)
    }

    /// Returns the exact number of distinct selected messages.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Returns whether this review selection contains no messages.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Creates an immutable snapshot after an exact-count user confirmation.
    ///
    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    pub fn confirm(&self, confirmed_count: usize) -> Result<ConfirmedBatch, SelectionError> {
        if self.keys.is_empty() {
            return Err(SelectionError::EmptySelection);
        }
        if confirmed_count != self.keys.len() {
            return Err(SelectionError::ConfirmationCountMismatch);
        }
        Ok(ConfirmedBatch { account: self.account.clone(), keys: self.keys.iter().cloned().collect() })
    }

    /// Refuses mismatched discovery data before changing the selection.
    fn validate(&self, message: &Message) -> Result<(), SelectionError> {
        if message.key().workspace() != self.account.workspace() {
            return Err(SelectionError::WorkspaceMismatch);
        }
        if message.author() != self.account.user() {
            return Err(SelectionError::AuthorMismatch);
        }
        Ok(())
    }
}

/// An immutable snapshot of the account and IDs explicitly approved by the user.
///
/// Revalidate session account and per-item authorship just before any network
/// deletion. A confirmed batch is authorization to *attempt revalidation*,
/// not permission to skip that check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmedBatch {
    /// Account that must match the active Slack session when the job runs.
    account: Account,
    /// Stable ordered message keys; never add items after confirmation.
    keys: Vec<MessageKey>,
}

impl ConfirmedBatch {
    /// Returns the account captured when the batch was confirmed.
    #[must_use]
    pub fn account(&self) -> &Account {
        &self.account
    }

    /// Returns the immutable selected-key snapshot in deterministic order.
    #[must_use]
    pub fn keys(&self) -> &[MessageKey] {
        &self.keys
    }
}

#[cfg(test)]
mod tests {
    use super::{Selection, SelectionError};
    use crate::identity::{Account, ConversationId, IdentityError, MessageKey, MessageTs, UserId, WorkspaceId};
    use crate::message::{ConversationKind, Message, ReviewFilter, ThreadFilter};

    /// Constructs a review fixture without an authenticated browser session.
    fn message(workspace: &str, author: &str, timestamp: &str) -> Result<Message, IdentityError> {
        Ok(Message::new(
            MessageKey::new(WorkspaceId::parse(workspace)?, ConversationId::parse("C1")?, MessageTs::parse(timestamp)?),
            UserId::parse(author)?,
            ConversationKind::PublicChannel,
            None,
            "preview".to_owned(),
        ))
    }

    /// Denies messages from either another workspace or another user.
    #[test]
    fn selection_never_crosses_account_boundaries() -> Result<(), IdentityError> {
        let account = Account::new(WorkspaceId::parse("T1")?, UserId::parse("U1")?);
        let mut selection = Selection::new(account);
        assert_eq!(selection.insert(&message("T2", "U1", "1.000001")?), Err(SelectionError::WorkspaceMismatch),
            "cross-workspace record must never be selected");
        assert_eq!(selection.insert(&message("T1", "U2", "1.000001")?), Err(SelectionError::AuthorMismatch),
            "another author's message must never be selected");
        assert_eq!(selection.len(), 0, "rejected inserts must not alter the selection");
        assert_eq!(selection.confirm(0), Err(SelectionError::EmptySelection),
            "empty review must never be promoted to a deletion batch");
        Ok(())
    }

    /// Confirms only an explicitly counted, deduplicated, frozen key set.
    #[test]
    fn confirmation_is_deduplicated_and_independent_of_later_review_changes() -> Result<(), Box<dyn std::error::Error>> {
        let account = Account::new(WorkspaceId::parse("T1")?, UserId::parse("U1")?);
        let mut selection = Selection::new(account);
        let first = message("T1", "U1", "1.000001")?;
        let second = message("T1", "U1", "1.000002")?;
        assert_eq!(selection.insert(&first), Ok(true), "first insertion must select message");
        assert_eq!(selection.insert(&first), Ok(false), "duplicate must not inflate selection count");
        assert_eq!(selection.insert(&second), Ok(true), "second distinct message must be selected");
        assert_eq!(selection.confirm(1), Err(SelectionError::ConfirmationCountMismatch),
            "wrong confirmation count must not create a deletion batch");
        let batch = selection.confirm(2)?;
        assert!(selection.remove(first.key()), "selected item can be removed from review");
        assert_eq!(batch.keys().len(), 2, "confirmed snapshot must not change with later review edits");
        assert_eq!(selection.len(), 1, "review selection should still be editable independently");
        Ok(())
    }

    /// An invalid item must not leave a partially selected visible result set.
    #[test]
    fn select_all_filtered_is_atomic_and_excludes_hidden_items() -> Result<(), IdentityError> {
        let account = Account::new(WorkspaceId::parse("T1")?, UserId::parse("U1")?);
        let mut selection = Selection::new(account);
        let visible = message("T1", "U1", "1.000001")?;
        let foreign = message("T1", "U2", "1.000002")?;
        let hidden = Message::new(
            MessageKey::new(WorkspaceId::parse("T1")?, ConversationId::parse("C2")?, MessageTs::parse("1.000003")?),
            UserId::parse("U1")?, ConversationKind::DirectMessage, None, "hidden".to_owned(),
        );
        let filter = ReviewFilter {
            kind: Some(ConversationKind::PublicChannel),
            conversation: Some(ConversationId::parse("C1")?),
            thread: ThreadFilter::All,
            query: None,
        };
        assert_eq!(selection.insert_filtered(&[visible.clone(), foreign], &filter), Err(SelectionError::AuthorMismatch),
            "invalid visible author must make bulk selection atomic");
        assert_eq!(selection.len(), 0, "no partial selection is allowed");
        assert_eq!(selection.insert_filtered(&[visible, hidden], &filter), Ok(1),
            "only items satisfying every visible filter are added");
        assert_eq!(selection.len(), 1, "hidden messages must not be implicitly selected");
        Ok(())
    }
}
