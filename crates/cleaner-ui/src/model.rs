//! Serialisable read-only Slack records and recoverable review checkpoints.

use std::collections::{BTreeMap, BTreeSet};

use cleaner_core::identity::{Account, ConversationId, MessageKey, MessageTs, UserId, WorkspaceId};
use cleaner_core::message::{ConversationKind, Message};
use cleaner_core::queue::{DeletionQueue, ItemState};
use cleaner_core::selection::Selection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Schema generation of local data, upgraded only by explicit migration.
const SCHEMA_VERSION: u32 = 1;

/// Minimal navigable conversation metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Conversation {
    /// Stable Slack conversation identifier.
    pub id: String,
    /// A human-readable Slack name or safe ID fallback.
    pub label: String,
    /// Public channel, private channel, DM, or group DM.
    pub kind: ConversationKind,
}

/// User-owned message, with enough metadata to recheck ownership before deletion.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OwnedMessage {
    /// Stable Slack channel or DM identifier.
    pub channel: String,
    /// Exact timestamp preserving fractional digits.
    pub timestamp: String,
    /// Thread root only when this message is a reply.
    pub parent: Option<String>,
    /// Conversation classification for local filtering.
    pub kind: ConversationKind,
    /// Bounded review-only preview.
    pub preview: String,
    /// Conversation name or ID captured at discovery time.
    pub conversation_label: String,
}

impl OwnedMessage {
    /// Converts untrusted network-derived data into the validated Rust domain model.
    ///
    /// # Errors
    /// Rejects malformed IDs or timestamps instead of queuing them for deletion.
    pub fn domain(&self, workspace: &str, author: &str) -> Result<Message, String> {
        let workspace_id = WorkspaceId::parse(workspace).map_err(|error| error.to_string())?;
        let channel_id = ConversationId::parse(&self.channel).map_err(|error| error.to_string())?;
        let timestamp = MessageTs::parse(&self.timestamp).map_err(|error| error.to_string())?;
        let user = UserId::parse(author).map_err(|error| error.to_string())?;
        let parent = self.parent.as_deref().map(MessageTs::parse)
            .transpose().map_err(|error| error.to_string())?;
        Ok(Message::new(MessageKey::new(workspace_id, channel_id, timestamp), user,
            self.kind, parent, self.preview.clone()))
    }

    /// Stable key for local review selection and status presentation.
    #[must_use]
    pub fn identifier(&self) -> String {
        format!("{}:{}", self.channel, self.timestamp)
    }
}

/// Persisted state of one reviewed item in a confirmed job.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobEntry {
    /// Immutable approved message snapshot.
    pub message: OwnedMessage,
    /// Recorded attempt outcome, never marked successful without Slack acknowledgment.
    pub status: ItemState,
}

/// A user-approved, manually resumed, account-bound deletion batch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SavedJob {
    /// Exact number confirmed, redundant for checkpoint integrity.
    pub confirmed_count: usize,
    /// Snapshot of individually selected IDs and per-item attempt state.
    pub entries: Vec<JobEntry>,
}

/// Saved data for one exact Slack workspace and user.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceData {
    /// Current JSON data schema version.
    pub schema: u32,
    /// Authenticated Slack team ID.
    pub workspace: String,
    /// Authenticated Slack user ID.
    pub user: String,
    /// Navigation items retained locally.
    pub conversations: Vec<Conversation>,
    /// Own-message snapshots retained locally for review, not credentials.
    pub messages: Vec<OwnedMessage>,
    /// A confirmed deletion queue, if one exists.
    pub job: Option<SavedJob>,
    /// True only if all conversations and pages were scanned successfully.
    pub scan_complete: bool,
}

impl WorkspaceData {
    /// Initializes review data without assuming successful discovery.
    #[must_use]
    pub fn new(workspace: String, user: String) -> Self {
        Self { schema: SCHEMA_VERSION, workspace, user,
            conversations: Vec::new(), messages: Vec::new(), job: None, scan_complete: false }
    }

    /// Restores a saved batch using the pure domain selection and queue checks.
    ///
    /// # Errors
    /// Fails closed on account mismatch, duplicate entries, corrupt counts or IDs.
    pub fn verified_queue(&self, job: &SavedJob) -> Result<DeletionQueue, String> {
        if self.schema != SCHEMA_VERSION || job.entries.len() != job.confirmed_count {
            return Err("Incompatible or inconsistent deletion checkpoint".to_owned());
        }
        let account = Account::new(WorkspaceId::parse(&self.workspace).map_err(|e| e.to_string())?,
            UserId::parse(&self.user).map_err(|e| e.to_string())?);
        let mut selection = Selection::new(account);
        let mut statuses = BTreeMap::new();
        for entry in &job.entries {
            let domain = entry.message.domain(&self.workspace, &self.user)?;
            let inserted = selection.insert(&domain).map_err(|e| e.to_string())?;
            if !inserted {
                return Err("Checkpoint contains a repeated message".to_owned());
            }
            statuses.insert(domain.key().clone(), entry.status);
        }
        let batch = selection.confirm(job.confirmed_count).map_err(|e| e.to_string())?;
        DeletionQueue::restore(batch, statuses).map_err(|e| e.to_string())
    }

    /// Appends discovered messages by stable key without inflating counts.
    pub fn merge_messages(&mut self, incoming: Vec<OwnedMessage>) {
        let mut existing: BTreeSet<String> = self.messages.iter().map(OwnedMessage::identifier).collect();
        for message in incoming {
            if existing.insert(message.identifier()) {
                self.messages.push(message);
            }
        }
    }
}

/// Converts a Slack conversation into the smallest necessary navigation record.
pub fn parse_conversation(raw: &Value) -> Option<Conversation> {
    let id = raw.get("id")?.as_str()?.to_owned();
    ConversationId::parse(&id).ok()?;
    let kind = if raw.get("is_im").and_then(Value::as_bool).unwrap_or(false) {
        ConversationKind::DirectMessage
    } else if raw.get("is_mpim").and_then(Value::as_bool).unwrap_or(false) {
        ConversationKind::GroupDirectMessage
    } else if raw.get("is_private").and_then(Value::as_bool).unwrap_or(false) {
        ConversationKind::PrivateChannel
    } else {
        ConversationKind::PublicChannel
    };
    let label = raw.get("name").and_then(Value::as_str)
        .or_else(|| raw.get("user").and_then(Value::as_str))
        .unwrap_or(&id).chars().take(150).collect();
    Some(Conversation { id, label, kind })
}

/// Parses only supported, explicitly user-authored Slack message types.
pub fn parse_owned(raw: &Value, conversation: &Conversation, user_id: &str) -> Option<OwnedMessage> {
    if raw.get("user")?.as_str()? != user_id {
        return None;
    }
    let subtype = raw.get("subtype").and_then(Value::as_str).unwrap_or("");
    if !matches!(subtype, "" | "me_message" | "file_share") {
        return None;
    }
    let timestamp = raw.get("ts")?.as_str()?.to_owned();
    MessageTs::parse(&timestamp).ok()?;
    let parent = raw.get("thread_ts").and_then(Value::as_str)
        .filter(|parent| *parent != timestamp.as_str())
        .map(str::to_owned);
    if parent.as_deref().is_some_and(|value| MessageTs::parse(value).is_err()) {
        return None;
    }
    let preview = raw.get("text").and_then(Value::as_str).unwrap_or("[Non-text message]")
        .chars().take(280).collect();
    Some(OwnedMessage { channel: conversation.id.clone(), timestamp, parent,
        kind: conversation.kind, conversation_label: conversation.label.clone(), preview })
}

/// Extracts the next pagination cursor, treating its absence as completion.
#[must_use]
pub fn cursor(value: &Value) -> String {
    value.pointer("/response_metadata/next_cursor").and_then(Value::as_str)
        .unwrap_or("").to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::{Conversation, OwnedMessage, WorkspaceData, parse_conversation, parse_owned};
    use cleaner_core::message::ConversationKind;
    use cleaner_core::queue::ItemState;

    /// Rejects other users and unsupported system message types.
    #[test]
    fn discovery_rejects_foreign_authors_and_system_events() {
        let conversation = Conversation { id: "C1".to_owned(), label: "general".to_owned(),
            kind: ConversationKind::PublicChannel };
        assert!(parse_owned(&json!({"user":"U2","ts":"1.000001"}), &conversation, "U1").is_none(),
            "another author's messages must never enter review");
        assert!(parse_owned(&json!({"user":"U1","ts":"1.000001","subtype":"bot_message"}), &conversation, "U1").is_none(),
            "bot-type events must never enter review");
    }

    /// The domain preserves thread parents and exact Slack timestamps.
    #[test]
    fn discovery_round_trip_keeps_thread_identity() -> Result<(), Box<dyn std::error::Error>> {
        let conversation = parse_conversation(&json!({"id":"D1","is_im":true,"user":"U2"}))
            .ok_or("conversation fixture was rejected")?;
        let item = parse_owned(&json!({"user":"U1","ts":"123.000002","thread_ts":"123.000001", "text":"hello"}),
            &conversation, "U1").ok_or("message fixture was rejected")?;
        assert_eq!(item.parent.as_deref(), Some("123.000001"), "thread reply must retain parent timestamp");
        assert_eq!(item.domain("T1", "U1")?.key().timestamp().as_str(), "123.000002", "timestamp must be exact");
        Ok(())
    }

    /// Corrupt and duplicated checkpoints are rejected before re-arming any job.
    #[test]
    fn saved_jobs_never_silently_replay_unknown_items() -> Result<(), Box<dyn std::error::Error>> {
        let mut data = WorkspaceData::new("T1".to_owned(), "U1".to_owned());
        let message = OwnedMessage { channel:"C1".to_owned(), timestamp:"1.000001".to_owned(),
            parent:None, kind:ConversationKind::PublicChannel, preview:"hello".to_owned(),
            conversation_label:"general".to_owned() };
        let job = super::SavedJob { confirmed_count: 2, entries: vec![
            super::JobEntry {message:message.clone(), status:ItemState::Queued},
            super::JobEntry {message, status:ItemState::Queued},
        ] };
        assert!(data.verified_queue(&job).is_err(), "duplicate selected IDs must be rejected");
        data.job = Some(job);
        Ok(())
    }
}
