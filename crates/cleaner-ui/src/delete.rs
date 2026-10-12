//! Safe, sequential deletion with independent ownership checks and checkpoints.

use cleaner_core::{
    identity::{Account, UserId, WorkspaceId},
    queue::{AttemptOutcome, ItemState},
};
use leptos::prelude::{GetUntracked, RwSignal, Set};
use serde_json::Value;

use crate::{
    browser,
    model::{JobEntry, OwnedMessage, SavedJob, WorkspaceData},
    scan::storage_key,
    slack::{Slack, SlackError},
};

/// Verified identity from Slack's live `auth.test` response.
pub(crate) async fn verify_account(token: &str) -> Result<(String, String), String> {
    let response = Slack::new(token)
        .call("auth.test", &[])
        .await
        .map_err(|error| error.to_string())?;
    let workspace = response
        .get("team_id")
        .and_then(Value::as_str)
        .ok_or("Slack did not return a workspace ID")?
        .to_owned();
    let user = response
        .get("user_id")
        .and_then(Value::as_str)
        .ok_or("Slack did not return a user ID")?
        .to_owned();
    WorkspaceId::parse(&workspace).map_err(|error| error.to_string())?;
    UserId::parse(&user).map_err(|error| error.to_string())?;
    Ok((workspace, user))
}

/// Builds an immutable domain-confirmed batch from currently selected snapshots.
///
/// # Errors
/// Refuses empty sets, wrong counts, other-account data, or duplicates.
pub(crate) fn make_job(data: &WorkspaceData, selected: &[OwnedMessage], count: usize) -> Result<SavedJob, String> {
    let account = Account::new(
        WorkspaceId::parse(&data.workspace).map_err(|e| e.to_string())?,
        UserId::parse(&data.user).map_err(|e| e.to_string())?,
    );
    let mut selection = cleaner_core::selection::Selection::new(account);
    for message in selected {
        let domain = message.domain(&data.workspace, &data.user)?;
        selection.insert(&domain).map_err(|error| error.to_string())?;
    }
    let confirmed = selection.confirm(count).map_err(|error| error.to_string())?;
    let selected_keys = confirmed.keys();
    let mut entries: Vec<JobEntry> = selected
        .iter()
        .filter_map(|message| {
            message
                .domain(&data.workspace, &data.user)
                .ok()
                .filter(|domain| selected_keys.contains(&domain.key().clone()))
                .map(|_| JobEntry {
                    message: message.clone(),
                    status: ItemState::Queued,
                })
        })
        .collect();
    entries.sort_by(|left, right| {
        left.message
            .parent
            .is_none()
            .cmp(&right.message.parent.is_none())
            .then(left.message.identifier().cmp(&right.message.identifier()))
    });
    let job = SavedJob {
        confirmed_count: count,
        entries,
    };
    let _queue = data.verified_queue(&job)?;
    Ok(job)
}

/// Ensures an exact matching user-authored item still exists immediately before deletion.
async fn preflight(slack: &Slack<'_>, message: &OwnedMessage, user: &str) -> Result<bool, SlackError> {
    let response = if let Some(root) = &message.parent {
        slack
            .call(
                "conversations.replies",
                &[
                    ("channel", &message.channel),
                    ("ts", root),
                    ("oldest", &message.timestamp),
                    ("latest", &message.timestamp),
                    ("inclusive", "true"),
                    ("limit", "100"),
                ],
            )
            .await?
    } else {
        slack
            .call(
                "conversations.history",
                &[
                    ("channel", &message.channel),
                    ("oldest", &message.timestamp),
                    ("latest", &message.timestamp),
                    ("inclusive", "true"),
                    ("limit", "100"),
                ],
            )
            .await?
    };
    Ok(response
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|raw| {
            raw.get("ts").and_then(Value::as_str) == Some(&message.timestamp)
                && raw.get("user").and_then(Value::as_str) == Some(user)
                && matches!(
                    raw.get("subtype").and_then(Value::as_str).unwrap_or(""),
                    "" | "me_message" | "file_share"
                )
        }))
}

/// Executes one attempt, distinguishing definite refusals from ambiguous network errors.
async fn attempt(token: &str, message: &OwnedMessage, workspace: &str, user: &str) -> AttemptOutcome {
    match verify_account(token).await {
        Ok((actual_workspace, actual_user)) if actual_workspace == workspace && actual_user == user => {},
        Ok(_) | Err(_) => return AttemptOutcome::Skipped,
    }
    let slack = Slack::new(token);
    match preflight(&slack, message, user).await {
        Ok(true) => {},
        Ok(false) => return AttemptOutcome::Skipped,
        Err(_) => return AttemptOutcome::Skipped,
    }
    match slack
        .call(
            "chat.delete",
            &[("channel", &message.channel), ("ts", &message.timestamp)],
        )
        .await
    {
        Ok(_) => AttemptOutcome::Deleted,
        Err(SlackError::Transport | SlackError::RateLimited | SlackError::InvalidResponse) => AttemptOutcome::Unknown,
        Err(SlackError::Api(_)) => AttemptOutcome::Failed,
    }
}

/// Persists the latest result after every state transition, in strict order.
async fn checkpoint(data: &WorkspaceData, state: RwSignal<WorkspaceData>) -> Result<(), String> {
    browser::save_local(&storage_key(&data.workspace, &data.user), data).await?;
    state.set(data.clone());
    Ok(())
}

/// Runs only an already confirmed job, after an explicit user Resume click.
///
/// # Errors
/// Stops on identity changes or failures to persist checkpoints. It never
/// automatically retries an uncertain deletion result after interruption.
pub(crate) async fn resume_job(
    token: &str,
    state: RwSignal<WorkspaceData>,
    status: RwSignal<String>,
    stop: RwSignal<bool>,
) -> Result<(), String> {
    if !cfg!(feature = "live-delete") {
        return Err("Live deletion is disabled in this unverified preview build".to_owned());
    }
    let mut data = state.get_untracked();
    let (actual_workspace, actual_user) = verify_account(token).await?;
    if actual_workspace != data.workspace || actual_user != data.user {
        return Err("Authenticated Slack account differs from the confirmed job".to_owned());
    }
    let job = data.job.as_ref().ok_or("No confirmed deletion job")?;
    let mut queue = data.verified_queue(job)?;
    let account = Account::new(
        WorkspaceId::parse(&data.workspace).map_err(|e| e.to_string())?,
        UserId::parse(&data.user).map_err(|e| e.to_string())?,
    );
    queue.resume(&account).map_err(|error| error.to_string())?;
    if let Some(saved) = data.job.as_mut() {
        for entry in &mut saved.entries {
            if entry.status == ItemState::InFlight {
                entry.status = ItemState::UnknownOutcome;
            }
        }
    }
    checkpoint(&data, state).await?;
    let length = data.job.as_ref().map_or(0, |saved| saved.entries.len());
    for index in 0..length {
        if stop.get_untracked() {
            status.set("Paused by user. Future deletion attempts stopped.".to_owned());
            break;
        }
        let Some(item) = data.job.as_ref().and_then(|job| job.entries.get(index)) else {
            return Err("Saved job changed unexpectedly".to_owned());
        };
        if item.status != ItemState::Queued {
            continue;
        }
        let message = item.message.clone();
        let key = message.domain(&data.workspace, &data.user)?.key().clone();
        queue.start(&key).map_err(|error| error.to_string())?;
        if let Some(entry) = data.job.as_mut().and_then(|job| job.entries.get_mut(index)) {
            entry.status = ItemState::InFlight;
        }
        checkpoint(&data, state).await?;
        status.set(format!(
            "Rechecking and deleting {}/{}",
            index.saturating_add(1),
            length
        ));
        let outcome = attempt(token, &message, &data.workspace, &data.user).await;
        queue.record_outcome(&key, outcome).map_err(|error| error.to_string())?;
        let finished = queue.state(&key).ok_or("Queue result missing after deletion attempt")?;
        if let Some(entry) = data.job.as_mut().and_then(|job| job.entries.get_mut(index)) {
            entry.status = finished;
        }
        checkpoint(&data, state).await?;
    }
    queue.pause();
    let remaining = data.job.as_ref().map_or(0, |job| {
        job.entries
            .iter()
            .filter(|entry| entry.status == ItemState::Queued)
            .count()
    });
    status.set(format!(
        "Queue stopped. {remaining} unattempted; Resume is always manual."
    ));
    Ok(())
}
