//! Conversation navigation and review filters; no destructive operations.

use serde::{Deserialize, Serialize};

use crate::identity::{ConversationId, MessageKey, MessageTs, UserId};

/// Conversation buckets available in the dashboard navigation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum ConversationKind {
    /// A public workspace channel.
    PublicChannel,
    /// A private channel visible to the signed-in account.
    PrivateChannel,
    /// One-to-one direct message.
    DirectMessage,
    /// Direct message with multiple participants.
    GroupDirectMessage,
}

/// Thread facet applied independently of the conversation navigation bucket.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum ThreadFilter {
    /// Include both top-level messages and replies.
    All,
    /// Include only replies within threads.
    RepliesOnly,
    /// Include messages that are not replies within threads.
    TopLevelOnly,
}

/// A discovered message and its locally reviewable metadata.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Message {
    /// Stable workspace/conversation/timestamp tuple.
    key: MessageKey,
    /// Author ID reported by the discovery source, not final deletion proof.
    author: UserId,
    /// Conversation bucket independent from thread navigation.
    kind: ConversationKind,
    /// Thread parent timestamp when this message is a reply.
    thread_parent: Option<MessageTs>,
    /// Minimal preview; adapters must apply their own retention/redaction policy.
    preview: String,
}

impl Message {
    /// Constructs a discovered record; source identity is verified separately.
    #[must_use]
    pub fn new(
        key: MessageKey,
        author: UserId,
        kind: ConversationKind,
        thread_parent: Option<MessageTs>,
        preview: String,
    ) -> Self {
        Self { key, author, kind, thread_parent, preview }
    }

    /// Returns the stable key used to select and deduplicate this record.
    #[must_use]
    pub fn key(&self) -> &MessageKey {
        &self.key
    }

    /// Returns the author claimed in discovery data.
    #[must_use]
    pub fn author(&self) -> &UserId {
        &self.author
    }

    /// Returns the message's containing conversation category.
    #[must_use]
    pub fn kind(&self) -> ConversationKind {
        self.kind
    }

    /// Returns the parent timestamp when the record is a thread reply.
    #[must_use]
    pub fn thread_parent(&self) -> Option<&MessageTs> {
        self.thread_parent.as_ref()
    }

    /// Returns a review-only, locally held message preview.
    #[must_use]
    pub fn preview(&self) -> &str {
        &self.preview
    }
}

/// Dashboard filters that do not imply selection or deletion permission.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ReviewFilter {
    /// Optional conversation category.
    pub kind: Option<ConversationKind>,
    /// Optional exact conversation ID.
    pub conversation: Option<ConversationId>,
    /// Independent thread-reply filter.
    pub thread: ThreadFilter,
    /// Case-insensitive substring query, when supplied.
    pub query: Option<String>,
}

impl ReviewFilter {
    /// Returns whether a discovered record belongs in the displayed result set.
    #[must_use]
    pub fn matches(&self, message: &Message) -> bool {
        let kind_matches = self.kind.is_none_or(|kind| kind == message.kind());
        let conversation_matches = self
            .conversation
            .as_ref()
            .is_none_or(|conversation| conversation == message.key().conversation());
        let thread_matches = match self.thread {
            ThreadFilter::All => true,
            ThreadFilter::RepliesOnly => message.thread_parent().is_some(),
            ThreadFilter::TopLevelOnly => message.thread_parent().is_none(),
        };
        let query_matches = self.query.as_ref().is_none_or(|query| {
            message.preview().to_lowercase().contains(&query.to_lowercase())
        });
        kind_matches && conversation_matches && thread_matches && query_matches
    }
}

#[cfg(test)]
mod tests {
    use super::{ConversationKind, Message, ReviewFilter, ThreadFilter};
    use crate::identity::{ConversationId, IdentityError, MessageKey, MessageTs, UserId, WorkspaceId};

    /// Thread reply navigation composes with the DM bucket and preview search.
    #[test]
    fn threads_can_be_filtered_within_their_original_conversation_bucket() -> Result<(), IdentityError> {
        let message = Message::new(
            MessageKey::new(WorkspaceId::parse("T1")?, ConversationId::parse("D1")?, MessageTs::parse("1.000002")?),
            UserId::parse("U1")?,
            ConversationKind::DirectMessage,
            Some(MessageTs::parse("1.000001")?),
            "An Old Thread Reply".to_owned(),
        );
        let filter = ReviewFilter {
            kind: Some(ConversationKind::DirectMessage),
            conversation: Some(ConversationId::parse("D1")?),
            thread: ThreadFilter::RepliesOnly,
            query: Some("OLD thread".to_owned()),
        };
        assert!(filter.matches(&message), "matching DM reply must appear in DM thread navigation");
        assert!(!ReviewFilter { thread: ThreadFilter::TopLevelOnly, ..filter }.matches(&message),
            "reply must not appear in top-level-only view");
        Ok(())
    }
}
