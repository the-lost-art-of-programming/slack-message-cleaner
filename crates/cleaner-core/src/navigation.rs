//! Pure, typed scope matching for workspace, conversation, and thread navigation.
//!
//! A scope is a review boundary, not an authorization grant. Callers must still
//! verify ownership before creating or executing a deletion batch.

use serde::{Deserialize, Serialize};

use crate::{
    identity::{ConversationId, MessageTs},
    message::Message,
};

// ---------------------------------------------------------------------------
// Review scopes
// ---------------------------------------------------------------------------

/// Hierarchical set of message IDs presented for review.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MessageScope {
    /// All accessible conversations, once workspace discovery is complete.
    Workspace,
    /// A complete channel, individual DM, or group DM including all replies.
    Conversation(ConversationId),
    /// One thread, including the root when authored by the active user.
    Thread {
        /// Exact containing conversation ID.
        conversation: ConversationId,
        /// Exact thread-root timestamp, with no floating-point conversions.
        root: MessageTs,
    },
}

impl MessageScope {
    /// Checks whether a validated message belongs to a container.
    ///
    /// This is a pure grouping predicate; it does **not** indicate that the
    /// user is authorized to delete that message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cleaner_core::{
    ///     identity::{ConversationId, MessageKey, MessageTs, UserId, WorkspaceId},
    ///     message::{ConversationKind, Message},
    ///     navigation::MessageScope,
    /// };
    ///
    /// let conversation = ConversationId::parse("C123")?;
    /// let root = MessageTs::parse("100.000001")?;
    /// let reply = Message::new(
    ///     MessageKey::new(
    ///         WorkspaceId::parse("T1")?,
    ///         conversation.clone(),
    ///         MessageTs::parse("101.000002")?,
    ///     ),
    ///     UserId::parse("U1")?,
    ///     ConversationKind::PublicChannel,
    ///     Some(root.clone()),
    ///     String::new(),
    /// );
    /// let scope = MessageScope::Thread { conversation, root };
    /// assert!(scope.contains(&reply));
    /// # Ok::<(), cleaner_core::identity::IdentityError>(())
    /// ```
    #[must_use]
    pub fn contains(&self, message: &Message) -> bool {
        match self {
            Self::Workspace => true,
            Self::Conversation(conversation) => message.key().conversation() == conversation,
            Self::Thread { conversation, root } => {
                message.key().conversation() == conversation
                    && (message.key().timestamp() == root || message.thread_parent() == Some(root))
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Safety regression tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::MessageScope;
    use crate::{
        identity::{ConversationId, IdentityError, MessageKey, MessageTs, UserId, WorkspaceId},
        message::{ConversationKind, Message},
    };

    fn fixture(channel: &str, timestamp: &str, parent: Option<&str>) -> Result<Message, IdentityError> {
        let parent = parent.map(MessageTs::parse).transpose()?;
        Ok(Message::new(
            MessageKey::new(
                WorkspaceId::parse("T1")?,
                ConversationId::parse(channel)?,
                MessageTs::parse(timestamp)?,
            ),
            UserId::parse("U1")?,
            ConversationKind::PublicChannel,
            parent,
            String::new(),
        ))
    }

    #[test]
    fn thread_scope_includes_root_and_replies_but_not_neighbor_threads() -> Result<(), IdentityError> {
        let scope = MessageScope::Thread {
            conversation: ConversationId::parse("C1")?,
            root: MessageTs::parse("10.000001")?,
        };
        assert!(scope.contains(&fixture("C1", "10.000001", None)?));
        assert!(scope.contains(&fixture("C1", "11.000001", Some("10.000001"))?));
        assert!(!scope.contains(&fixture("C1", "12.000001", Some("10.000002"))?));
        assert!(!scope.contains(&fixture("C2", "11.000001", Some("10.000001"))?));
        Ok(())
    }

    #[test]
    fn conversation_scope_includes_top_level_and_nested_replies() -> Result<(), IdentityError> {
        let scope = MessageScope::Conversation(ConversationId::parse("D1")?);
        assert!(scope.contains(&fixture("D1", "10.000001", None)?));
        assert!(scope.contains(&fixture("D1", "11.000001", Some("10.000001"))?));
        assert!(!scope.contains(&fixture("D2", "11.000001", Some("10.000001"))?));
        Ok(())
    }
}
