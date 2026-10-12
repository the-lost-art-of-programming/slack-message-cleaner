//! Paged, read-only Slack discovery with independently verified container coverage.
//!
//! The user never needs to scroll Slack. Every visible workspace, conversation
//! and thread is fetched through authenticated Slack requests and recorded in
//! checkpointed review state. A failed or interrupted page never becomes a
//! falsely "complete" deletion scope.

use std::collections::BTreeSet;

use leptos::prelude::{GetUntracked, RwSignal, Set};
use serde_json::Value;

use crate::{
    browser,
    model::{
        Conversation, OwnedMessage, ScanStatus, ThreadRecord, WorkspaceData, cursor, parse_conversation, parse_owned,
    },
    slack::Slack,
};

// ---------------------------------------------------------------------------
// Workspaces and pagination
// ---------------------------------------------------------------------------

/// Stores review and deletion checkpoints under the verified workspace/user pair.
#[must_use]
pub fn storage_key(workspace: &str, user: &str) -> String {
    format!("review_{workspace}_{user}")
}

/// Lists all accessible conversations and rejects pagination loops.
async fn conversations(slack: &Slack<'_>, cancelled: RwSignal<bool>) -> Result<Vec<Conversation>, String> {
    let mut all = Vec::new();
    let mut next = String::new();
    let mut seen = BTreeSet::new();

    loop {
        if cancelled.get_untracked() {
            return Err("Conversation discovery canceled".to_owned());
        }
        let page = slack
            .call(
                "conversations.list",
                &[
                    ("types", "public_channel,private_channel,im,mpim"),
                    ("limit", "200"),
                    ("cursor", &next),
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        for raw in page.get("channels").and_then(Value::as_array).into_iter().flatten() {
            if let Some(channel) = parse_conversation(raw) {
                all.push(channel);
            }
        }
        next = cursor(&page);
        if next.is_empty() {
            if page.get("has_more").and_then(Value::as_bool) == Some(true) {
                return Err("Conversation pagination stopped without a cursor".to_owned());
            }
            break;
        }
        if !seen.insert(next.clone()) {
            return Err("Slack returned a repeated conversation cursor".to_owned());
        }
    }
    all.sort_by(|left, right| left.id.cmp(&right.id));
    all.dedup_by(|left, right| left.id == right.id);
    Ok(all)
}

/// Result of fully paginating either a channel history or one reply thread.
struct PageSet {
    /// Only messages owned by the authenticated account.
    messages: Vec<OwnedMessage>,
    /// Roots observed in the outer history, including other authors' roots.
    threads: Vec<ThreadRecord>,
}

/// Fetches every cursor page, never interpreting a dangling `has_more` as success.
async fn pages(
    slack: &Slack<'_>,
    method: &str,
    channel: &str,
    parent: Option<&str>,
    user: &str,
    conversation: &Conversation,
    cancelled: RwSignal<bool>,
) -> Result<PageSet, String> {
    let mut result = PageSet {
        messages: Vec::new(),
        threads: Vec::new(),
    };
    let mut next = String::new();
    let mut seen = BTreeSet::new();

    loop {
        if cancelled.get_untracked() {
            return Err("Message discovery canceled".to_owned());
        }
        let response = if let Some(root) = parent {
            slack
                .call(
                    method,
                    &[("channel", channel), ("ts", root), ("limit", "200"), ("cursor", &next)],
                )
                .await
                .map_err(|error| error.to_string())?
        } else {
            slack
                .call(method, &[("channel", channel), ("limit", "200"), ("cursor", &next)])
                .await
                .map_err(|error| error.to_string())?
        };
        for raw in response.get("messages").and_then(Value::as_array).into_iter().flatten() {
            if parent.is_none() {
                let replies = raw.get("reply_count").and_then(Value::as_u64).unwrap_or(0);
                if replies > 0 {
                    if let Some(timestamp) = raw.get("ts").and_then(Value::as_str) {
                        result.threads.push(ThreadRecord {
                            channel: conversation.id.clone(),
                            root: timestamp.to_owned(),
                            reply_count: replies,
                            status: ScanStatus::Pending,
                        });
                    }
                }
            }
            if let Some(mut message) = parse_owned(raw, conversation, user) {
                if let Some(root) = parent {
                    if message.timestamp == root {
                        continue;
                    }
                    message.parent = Some(root.to_owned());
                }
                result.messages.push(message);
            }
        }
        next = cursor(&response);
        if next.is_empty() {
            if response.get("has_more").and_then(Value::as_bool) == Some(true) {
                return Err("Message pagination stopped without a cursor".to_owned());
            }
            break;
        }
        if !seen.insert(next.clone()) {
            return Err("Slack returned a repeated message cursor".to_owned());
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Container coverage and checkpoints
// ---------------------------------------------------------------------------

/// Completed or partially completed scan of one conversation.
struct ConversationScan {
    /// Discovered messages belonging to the authenticated user.
    messages: Vec<OwnedMessage>,
    /// Thread roots, including roots written by someone else.
    threads: Vec<ThreadRecord>,
    /// Individual thread failures; an empty list proves all pages were scanned.
    errors: Vec<String>,
}

/// Scans all history, then every thread, retaining successful work on errors.
async fn scan_one(
    slack: &Slack<'_>,
    conversation: &Conversation,
    user: &str,
    cancelled: RwSignal<bool>,
) -> Result<ConversationScan, String> {
    let history = pages(
        slack,
        "conversations.history",
        &conversation.id,
        None,
        user,
        conversation,
        cancelled,
    )
    .await?;
    let mut scan = ConversationScan {
        messages: history.messages,
        threads: history.threads,
        errors: Vec::new(),
    };
    for thread in &mut scan.threads {
        if cancelled.get_untracked() {
            scan.errors
                .push("Stopped before every reply thread was read".to_owned());
            break;
        }
        match pages(
            slack,
            "conversations.replies",
            &conversation.id,
            Some(&thread.root),
            user,
            conversation,
            cancelled,
        )
        .await
        {
            Ok(replies) => {
                scan.messages.extend(replies.messages);
                thread.status = ScanStatus::Complete;
            },
            Err(error) => {
                thread.status = ScanStatus::Partial;
                scan.errors.push(format!("Thread {}: {error}", thread.root));
            },
        }
    }
    Ok(scan)
}

/// Persists a scan checkpoint, never including session credentials.
async fn checkpoint(data: &WorkspaceData, state: RwSignal<WorkspaceData>) -> Result<(), String> {
    browser::save_local(&storage_key(&data.workspace, &data.user), data).await?;
    state.set(data.clone());
    Ok(())
}

/// Scans a particular channel or DM without clearing other conversations.
///
/// # Errors
/// Returns errors for canceled, failed, or only partially completed scans.
/// Incomplete results are retained for individual review but cannot be used
/// as proof of complete container-wide selection.
pub async fn scan_conversation(
    token: &str,
    mut data: WorkspaceData,
    channel: &str,
    state: RwSignal<WorkspaceData>,
    status: RwSignal<String>,
    cancelled: RwSignal<bool>,
) -> Result<(), String> {
    let item = data
        .conversations
        .iter()
        .find(|entry| entry.id == channel)
        .cloned()
        .ok_or("Conversation was not discovered in this workspace")?;
    let slack = Slack::new(token);
    data.coverage.insert(item.id.clone(), ScanStatus::Scanning);
    data.scan_complete = false;
    checkpoint(&data, state).await?;
    status.set(format!("Scanning {} and all reply threads…", item.label));

    let outcome = scan_one(&slack, &item, &data.user, cancelled).await;
    data.messages.retain(|message| message.channel != item.id);
    data.threads.retain(|thread| thread.channel != item.id);
    let result = match outcome {
        Ok(scan) => {
            data.merge_messages(scan.messages);
            data.threads.extend(scan.threads);
            if scan.errors.is_empty() {
                data.coverage.insert(item.id.clone(), ScanStatus::Complete);
                status.set(format!("{} fully scanned, including its threads.", item.label));
                Ok(())
            } else {
                data.coverage.insert(item.id.clone(), ScanStatus::Partial);
                let error = format!("Partial scan of {}: {}", item.label, scan.errors.join("; "));
                status.set(error.clone());
                Err(error)
            }
        },
        Err(error) => {
            data.coverage.insert(item.id.clone(), ScanStatus::Partial);
            status.set(format!("Incomplete scan of {}: {error}", item.label));
            Err(error)
        },
    };
    checkpoint(&data, state).await?;
    result
}

/// Discovers all accessible containers and scans every thread, read-only.
///
/// # Errors
/// Returns an error for interrupted discovery or any incomplete channel.
pub async fn scan_all(
    token: &str,
    mut data: WorkspaceData,
    state: RwSignal<WorkspaceData>,
    status: RwSignal<String>,
    cancelled: RwSignal<bool>,
) -> Result<(), String> {
    let slack = Slack::new(token);
    status.set("Listing accessible Slack conversations…".to_owned());
    let list = conversations(&slack, cancelled).await?;
    data.conversations = list.clone();
    data.messages.clear();
    data.threads.clear();
    data.coverage.clear();
    data.scan_complete = false;
    checkpoint(&data, state).await?;

    let mut failures = Vec::new();
    for (index, conversation) in list.iter().enumerate() {
        if cancelled.get_untracked() {
            return Err("Scan canceled. Completed conversations remain saved.".to_owned());
        }
        status.set(format!(
            "Conversation {} / {}: {}",
            index.saturating_add(1),
            list.len(),
            conversation.label
        ));
        let current = state.get_untracked();
        if let Err(error) = scan_conversation(token, current, &conversation.id, state, status, cancelled).await {
            failures.push(error);
        }
    }
    let mut current = state.get_untracked();
    current.scan_complete = failures.is_empty()
        && current
            .conversations
            .iter()
            .all(|item| current.conversation_complete(&item.id));
    checkpoint(&current, state).await?;
    if failures.is_empty() {
        status.set(format!(
            "Workspace scan complete: {} own messages, {} conversations, {} threads.",
            current.messages.len(),
            current.conversations.len(),
            current.threads.len()
        ));
        Ok(())
    } else {
        let summary = format!(
            "Incomplete workspace scan: {} conversations require review.",
            failures.len()
        );
        status.set(summary.clone());
        Err(summary)
    }
}
