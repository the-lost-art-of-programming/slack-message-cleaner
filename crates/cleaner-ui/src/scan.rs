//! Incremental, cancelable, read-only Slack discovery of accessible user messages.

use std::collections::BTreeSet;

use leptos::prelude::{GetUntracked, RwSignal, Set};
use serde_json::Value;

use crate::{
    browser,
    model::{Conversation, OwnedMessage, WorkspaceData, cursor, parse_conversation, parse_owned},
    slack::Slack,
};

/// Separate storage keys by authenticated workspace and user.
#[must_use]
pub(crate) fn storage_key(workspace: &str, user: &str) -> String {
    format!("review_{workspace}_{user}")
}

/// Reads all accessible conversation pages, rejecting repeated cursors.
async fn conversations(slack: &Slack<'_>, cancelled: RwSignal<bool>) -> Result<Vec<Conversation>, String> {
    let mut all = Vec::new();
    let mut next = String::new();
    let mut seen = BTreeSet::new();
    loop {
        if cancelled.get_untracked() {
            return Err("Scan canceled by user".to_owned());
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

/// Fetches every page of a particular Slack conversation or reply thread.
async fn pages(
    slack: &Slack<'_>,
    method: &str,
    channel: &str,
    parent: Option<&str>,
    user: &str,
    conversation: &Conversation,
    cancelled: RwSignal<bool>,
) -> Result<(Vec<OwnedMessage>, Vec<String>), String> {
    let mut found = Vec::new();
    let mut roots = Vec::new();
    let mut next = String::new();
    let mut seen = BTreeSet::new();
    loop {
        if cancelled.get_untracked() {
            return Err("Scan canceled by user".to_owned());
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
            if parent.is_none() && raw.get("reply_count").and_then(Value::as_u64).unwrap_or(0) > 0 {
                if let Some(timestamp) = raw.get("ts").and_then(Value::as_str) {
                    roots.push(timestamp.to_owned());
                }
            }
            if let Some(mut message) = parse_owned(raw, conversation, user) {
                if let Some(root) = parent {
                    if message.timestamp != root {
                        message.parent = Some(root.to_owned());
                    } else {
                        continue;
                    }
                }
                found.push(message);
            }
        }
        next = cursor(&response);
        if next.is_empty() {
            break;
        }
        if !seen.insert(next.clone()) {
            return Err("Slack returned a repeated message cursor".to_owned());
        }
    }
    Ok((found, roots))
}

/// Scans channel history and every visible reply thread, including other authors' roots.
async fn scan_one(
    slack: &Slack<'_>,
    conversation: &Conversation,
    user: &str,
    cancelled: RwSignal<bool>,
) -> Result<Vec<OwnedMessage>, String> {
    let (mut found, roots) = pages(
        slack,
        "conversations.history",
        &conversation.id,
        None,
        user,
        conversation,
        cancelled,
    )
    .await?;
    for root in roots {
        if cancelled.get_untracked() {
            return Err("Scan canceled by user".to_owned());
        }
        let (replies, _nested) = pages(
            slack,
            "conversations.replies",
            &conversation.id,
            Some(&root),
            user,
            conversation,
            cancelled,
        )
        .await?;
        found.extend(replies);
    }
    Ok(found)
}

/// Performs discovery and periodically stores partial results; never sends deletion requests.
///
/// # Errors
/// Reports cancellation, permissions, API errors, or checkpoint failures explicitly.
pub(crate) async fn scan_all(
    token: &str,
    mut data: WorkspaceData,
    state: RwSignal<WorkspaceData>,
    status: RwSignal<String>,
    cancelled: RwSignal<bool>,
) -> Result<(), String> {
    let slack = Slack::new(token);
    status.set("Listing conversations…".to_owned());
    let list = conversations(&slack, cancelled).await?;
    data.conversations = list.clone();
    data.messages.clear();
    data.scan_complete = false;
    let key = storage_key(&data.workspace, &data.user);
    browser::save_local(&key, &data).await?;
    state.set(data.clone());
    let mut problems = Vec::new();
    for (index, conversation) in list.iter().enumerate() {
        if cancelled.get_untracked() {
            return Err("Scan canceled; reviewed results remain saved".to_owned());
        }
        status.set(format!(
            "Scanning {} of {}: {} ({} matches)",
            index.saturating_add(1),
            list.len(),
            conversation.label,
            data.messages.len()
        ));
        match scan_one(&slack, conversation, &data.user, cancelled).await {
            Ok(messages) => data.merge_messages(messages),
            Err(error) => problems.push(format!("{}: {error}", conversation.label)),
        }
        browser::save_local(&key, &data).await?;
        state.set(data.clone());
    }
    data.scan_complete = problems.is_empty();
    browser::save_local(&key, &data).await?;
    state.set(data.clone());
    if problems.is_empty() {
        status.set(format!(
            "Scan complete. {} own messages in {} conversations.",
            data.messages.len(),
            list.len()
        ));
        Ok(())
    } else {
        status.set(format!(
            "Partial scan: {} conversation(s) inaccessible or failed.",
            problems.len()
        ));
        Err(format!("Incomplete scan: {}", problems.join("; ")))
    }
}
