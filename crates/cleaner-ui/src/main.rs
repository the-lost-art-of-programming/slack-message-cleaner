//! Leptos browser UI: discovery, navigation, deliberate selection, and paused jobs.

#![forbid(unsafe_code)]

mod browser;
mod delete;
mod model;
mod scan;
mod slack;

use std::collections::BTreeSet;

use cleaner_core::{
    identity::{ConversationId, MessageTs, WorkspaceId},
    links::{conversation_url, permalink_is_for, workspace_url},
    message::ConversationKind,
    queue::ItemState,
};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::{
    model::{OwnedMessage, ScanStatus, WorkspaceData},
    slack::Slack,
};

/// Maximum number of message DOM rows rendered per review page.
const PAGE_SIZE: usize = 100;

/// Selects only items already retrieved for review, never an unloaded search universe.
#[must_use]
fn visible_messages(
    data: &WorkspaceData,
    category: &str,
    channel: &str,
    thread: &str,
    query: &str,
) -> Vec<OwnedMessage> {
    let phrase = query.to_lowercase();
    data.messages
        .iter()
        .filter(|message| {
            let category_matches = match category {
                "public" => message.kind == ConversationKind::PublicChannel,
                "private" => message.kind == ConversationKind::PrivateChannel,
                "dm" => message.kind == ConversationKind::DirectMessage,
                "group" => message.kind == ConversationKind::GroupDirectMessage,
                "threads" => message.parent.is_some(),
                "all" => true,
                _ => false,
            };
            let thread_matches = thread.is_empty()
                || (message.channel == channel
                    && (message.timestamp == thread || message.parent.as_deref() == Some(thread)));
            category_matches
                && (channel.is_empty() || message.channel == channel)
                && thread_matches
                && message.preview.to_lowercase().contains(&phrase)
        })
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Browser navigation: only verified Slack links
// ---------------------------------------------------------------------------

/// Opens a typed Slack workspace in the user's existing Slack browser tab.
async fn open_workspace(workspace: &str) -> Result<(), String> {
    let team = WorkspaceId::parse(workspace).map_err(|error| error.to_string())?;
    browser::navigate_slack(&workspace_url(&team), workspace).await
}

/// Opens a channel, private channel, individual DM or group DM.
async fn open_conversation(workspace: &str, channel: &str) -> Result<(), String> {
    let team = WorkspaceId::parse(workspace).map_err(|error| error.to_string())?;
    let target = ConversationId::parse(channel).map_err(|error| error.to_string())?;
    browser::navigate_slack(&conversation_url(&team, &target), workspace).await
}

/// Uses Slack's canonical permalink so replies open at their exact location.
///
/// # Errors
/// Rejects invalid identifiers, unavailable sessions, and off-origin links.
async fn open_message(token: &str, workspace: &str, channel: &str, timestamp: &str) -> Result<(), String> {
    let conversation = ConversationId::parse(channel).map_err(|error| error.to_string())?;
    let ts = MessageTs::parse(timestamp).map_err(|error| error.to_string())?;
    let response = Slack::new(token)
        .call("chat.getPermalink", &[("channel", channel), ("message_ts", timestamp)])
        .await
        .map_err(|error| error.to_string())?;
    let link = response
        .get("permalink")
        .and_then(serde_json::Value::as_str)
        .ok_or("Slack did not return a message permalink")?;
    if !permalink_is_for(link, &conversation, &ts) {
        return Err("Slack returned a permalink for a different message or an untrusted host".to_owned());
    }
    browser::navigate_slack(link, workspace).await
}

/// Friendly status title for an item in a durable, manually resumed job.
#[must_use]
fn state_label(status: ItemState) -> &'static str {
    match status {
        ItemState::Queued => "Queued",
        ItemState::InFlight => "Interrupted attempt",
        ItemState::Succeeded => "Deleted",
        ItemState::Skipped => "Skipped: not verified",
        ItemState::Failed => "Failed",
        ItemState::UnknownOutcome => "Unknown outcome: never auto-retried",
    }
}

/// App's sidebar/standalone-tab entry point.
#[component]
fn App() -> impl IntoView {
    let token = RwSignal::new(None::<String>);
    let status = RwSignal::new("Open a Slack web tab, open this extension, then refresh Slack.".to_owned());
    let data = RwSignal::new(WorkspaceData::new(String::new(), String::new()));
    let category = RwSignal::new("all".to_owned());
    let conversation = RwSignal::new(String::new());
    let thread = RwSignal::new(String::new());
    let query = RwSignal::new(String::new());
    let page = RwSignal::new(0_usize);
    let selected = RwSignal::new(BTreeSet::<String>::new());
    let typed_count = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let stop = RwSignal::new(false);

    if let Err(error) = browser::register_capture(token, status) {
        status.set(format!("Slack request capture unavailable: {error}"));
    }
    spawn_local(async move {
        match browser::get_session_token().await {
            Ok(Some(captured)) => {
                token.set(Some(captured));
                status.set("Existing browser-session token found. Select Connect.".to_owned());
            },
            Ok(None) => {},
            Err(error) => status.set(format!("Session storage unavailable: {error}")),
        }
    });

    let connect = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            let result = async {
                let captured = token
                    .get_untracked()
                    .ok_or("No captured token. Refresh an authenticated Slack tab.")?;
                let (workspace, user) = delete::verify_account(&captured).await?;
                let key = scan::storage_key(&workspace, &user);
                let mut restored = browser::get_local::<WorkspaceData>(&key)
                    .await?
                    .unwrap_or_else(|| WorkspaceData::new(workspace.clone(), user.clone()));
                if restored.workspace != workspace || restored.user != user {
                    return Err("Saved data belongs to a different Slack account".to_owned());
                }
                restored.migrate_legacy()?;
                if let Some(saved) = restored.job.clone() {
                    let recovered = restored.verified_queue(&saved)?;
                    if let Some(job) = restored.job.as_mut() {
                        for entry in &mut job.entries {
                            let key = entry.message.domain(&workspace, &user)?.key().clone();
                            entry.status = recovered.state(&key).ok_or("Missing job checkpoint item")?;
                        }
                    }
                    browser::save_local(&key, &restored).await?;
                }
                data.set(restored);
                selected.set(BTreeSet::new());
                Ok::<(String, String), String>((workspace, user))
            }
            .await;
            match result {
                Ok((workspace, user)) => status.set(format!("Authenticated as {user} in {workspace}. Ready to scan.")),
                Err(error) => status.set(format!("Connection failed: {error}")),
            }
            busy.set(false);
        });
    };

    let scan = move |_| {
        if busy.get_untracked() || data.get_untracked().workspace.is_empty() || data.get_untracked().job.is_some() {
            return;
        }
        busy.set(true);
        stop.set(false);
        spawn_local(async move {
            let result = if let Some(captured) = token.get_untracked() {
                let current = data.get_untracked();
                match delete::verify_account(&captured).await {
                    Ok((workspace, user)) if workspace == current.workspace && user == current.user => {
                        scan::scan_all(&captured, current, data, status, stop).await
                    },
                    Ok(_) => Err("Active Slack account changed. Scan aborted.".to_owned()),
                    Err(error) => Err(error),
                }
            } else {
                Err("No authenticated Slack session".to_owned())
            };
            if let Err(error) = result {
                status.set(error);
            }
            selected.set(BTreeSet::new());
            busy.set(false);
        });
    };

    let scan_selected = move |_| {
        let current = data.get_untracked();
        let selected_channel = conversation.get_untracked();
        if busy.get_untracked() || current.job.is_some() || selected_channel.is_empty() {
            return;
        }
        busy.set(true);
        stop.set(false);
        spawn_local(async move {
            let result = if let Some(captured) = token.get_untracked() {
                match delete::verify_account(&captured).await {
                    Ok((workspace, user)) if workspace == current.workspace && user == current.user => {
                        scan::scan_conversation(&captured, current, &selected_channel, data, status, stop).await
                    },
                    Ok(_) => Err("Session belongs to a different workspace or user".to_owned()),
                    Err(error) => Err(error),
                }
            } else {
                Err("Authenticate with Slack before scanning".to_owned())
            };
            if let Err(error) = result {
                status.set(error);
            }
            selected.set(BTreeSet::new());
            busy.set(false);
        });
    };

    // Container-wide selection is only permitted after every relevant page
    // has completed successfully. Selecting one loaded row needs no such claim.
    let select_container = move |_| {
        let current = data.get_untracked();
        let channel = conversation.get_untracked();
        let root = thread.get_untracked();
        let complete = if !root.is_empty() {
            current.thread_complete(&channel, &root)
        } else if !channel.is_empty() {
            current.conversation_complete(&channel)
        } else {
            current.scan_complete
        };
        if !complete {
            status.set("Container is not fully scanned. Run its scan before selecting ALL.".to_owned());
            return;
        }
        let items = visible_messages(&current, "all", &channel, &root, "");
        // Replacing rather than adding prevents unseen prior selections from
        // leaking into a channel-wide or thread-wide destructive batch.
        selected.set(items.into_iter().map(|item| item.identifier()).collect());
    };

    let open_team = move |_| {
        let workspace = data.get_untracked().workspace;
        spawn_local(async move {
            if let Err(error) = open_workspace(&workspace).await {
                status.set(format!("Could not open workspace: {error}"));
            }
        });
    };

    let select_visible = move |_| {
        let items = visible_messages(
            &data.get_untracked(),
            &category.get_untracked(),
            &conversation.get_untracked(),
            &thread.get_untracked(),
            &query.get_untracked(),
        );
        selected.update(|keys| {
            for item in items {
                keys.insert(item.identifier());
            }
        });
    };

    let select_page = move |_| {
        let items = visible_messages(
            &data.get_untracked(),
            &category.get_untracked(),
            &conversation.get_untracked(),
            &thread.get_untracked(),
            &query.get_untracked(),
        );
        selected.update(|keys| {
            for item in items
                .into_iter()
                .skip(page.get_untracked().saturating_mul(PAGE_SIZE))
                .take(PAGE_SIZE)
            {
                keys.insert(item.identifier());
            }
        });
    };

    let create_job = move |_| {
        if busy.get_untracked() || data.get_untracked().job.is_some() {
            return;
        }
        let current = data.get_untracked();
        let chosen = selected.get_untracked();
        let count = typed_count.get_untracked().parse::<usize>().unwrap_or(0);
        let approved: Vec<OwnedMessage> = current
            .messages
            .iter()
            .filter(|message| chosen.contains(&message.identifier()))
            .cloned()
            .collect();
        match delete::make_job(&current, &approved, count) {
            Ok(job) => {
                let mut snapshot = current;
                snapshot.job = Some(job);
                busy.set(true);
                spawn_local(async move {
                    match browser::save_local(&scan::storage_key(&snapshot.workspace, &snapshot.user), &snapshot).await
                    {
                        Ok(()) => {
                            data.set(snapshot);
                            status.set("Job confirmed and paused. Select Resume to begin deletion.".to_owned());
                        },
                        Err(error) => status.set(format!("Job not queued: {error}")),
                    }
                    busy.set(false);
                });
            },
            Err(error) => status.set(format!(
                "Batch was not created: {error}. Enter the exact selection count."
            )),
        }
    };

    let resume = move |_| {
        if busy.get_untracked() || data.get_untracked().job.is_none() {
            return;
        }
        busy.set(true);
        stop.set(false);
        spawn_local(async move {
            let result = if let Some(captured) = token.get_untracked() {
                delete::resume_job(&captured, data, status, stop).await
            } else {
                Err("Slack session must be recaptured before resuming".to_owned())
            };
            if let Err(error) = result {
                status.set(format!("Deletion paused: {error}"));
            }
            busy.set(false);
        });
    };

    let clear_job = move |_| {
        if busy.get_untracked() {
            return;
        }
        let mut current = data.get_untracked();
        if let Some(job) = &current.job {
            if job.entries.iter().any(|entry| entry.status == ItemState::InFlight) {
                status.set("Cannot discard a job while a request is in flight.".to_owned());
                return;
            }
        }
        current.job = None;
        busy.set(true);
        spawn_local(async move {
            match browser::save_local(&scan::storage_key(&current.workspace, &current.user), &current).await {
                Ok(()) => {
                    data.set(current);
                    status.set("Job discarded. Existing messages were not restored or retried.".to_owned());
                },
                Err(error) => status.set(format!("Could not discard job: {error}")),
            }
            busy.set(false);
        });
    };

    let forget_token = move |_| {
        if busy.get_untracked() {
            return;
        }
        spawn_local(async move {
            match browser::clear_session_token().await {
                Ok(()) => {
                    token.set(None);
                    status.set("Credential erased; refresh Slack to recapture.".to_owned());
                },
                Err(error) => status.set(format!("Unable to clear session: {error}")),
            }
        });
    };

    view! {
        <main class="shell">
            <header>
                <div><h1>"Own Message Cleaner"</h1>
                    <div class="muted small">"Rust · Leptos · Firefox · Local-only"</div></div>
                <a href="index.html" target="_blank" rel="noreferrer">"Open in tab ↗"</a>
            </header>
            <section>
                <h2>"1. Connect to Slack"</h2>
                <p class="small muted">"Open Slack in Firefox, reload its tab to capture your web-session token, then Connect. No Slack app installation is required. Workspace permissions still apply."</p>
                <div class="controls">
                    <button class="primary" on:click=connect disabled=move || busy.get()>"Connect / Restore"</button>
                    <button on:click=forget_token disabled=move || busy.get()>"Clear session credential"</button>
                    <span class="muted small">{move || if token.get().is_some() { "Session captured" } else { "No session captured" }}</span>
                </div>
                <p class="status" role="status">{move || status.get()}</p>
            </section>
            <section>
                <h2>"2. Discover your messages"</h2>
                <div class="controls">
                    <button class="primary" on:click=scan disabled=move || busy.get() || data.get().workspace.is_empty() || data.get().job.is_some()>"Scan all conversations and threads"</button>
                    <button class="danger" on:click=move |_| stop.set(true) disabled=move || !busy.get()>"Stop after current request"</button>
                </div>
                <p class="small muted">{move || format!("{} messages · {} conversations · {}",
                    data.get().messages.len(), data.get().conversations.len(),
                    if data.get().scan_complete { "complete scan" } else { "scan partial / not yet run" })}</p>
            </section>
            <section>
                <h2>"3. Navigate and review"</h2>
                <p class="small muted">"Select a container inside the extension, or click Open to navigate its exact location in Slack. Message and thread links use canonical permalinks."</p>
                <div class="controls">
                    <button on:click=open_team disabled=move || data.get().workspace.is_empty()>"Open workspace in Slack ↗"</button>
                    <button on:click=scan_selected disabled=move || busy.get() || conversation.get().is_empty() || data.get().job.is_some()>"Scan selected channel / DM and threads"</button>
                    <button on:click=select_container disabled=move || busy.get() || data.get().job.is_some()>"Replace selection: ALL in complete container"</button>
                </div>
                <p class="small muted">{move || {
                    let current = data.get();
                    let channel = conversation.get();
                    let root = thread.get();
                    if channel.is_empty() {
                        format!("Workspace coverage: {}", if current.scan_complete { "complete" } else { "incomplete" })
                    } else if !root.is_empty() {
                        format!("Thread coverage: {}", if current.thread_complete(&channel, &root) { "complete" } else { "incomplete" })
                    } else {
                        format!("Channel / DM coverage: {}", if current.conversation_complete(&channel) { "complete" } else { "incomplete" })
                    }
                }}</p>
                <div class="container-list">
                    <For each=move || data.get().conversations key=|item| item.id.clone() children=move |item| {
                        let local_id = item.id.clone();
                        let open_id = item.id.clone();
                        let status_id = item.id.clone();
                        let label = item.label.clone();
                        view! {
                            <div class="controls container-row">
                                <button on:click=move |_| { conversation.set(local_id.clone()); thread.set(String::new()); page.set(0); }>
                                    "Filter here"
                                </button>
                                <button on:click=move |_| {
                                    let current = data.get_untracked();
                                    let target = open_id.clone();
                                    conversation.set(target.clone());
                                    thread.set(String::new());
                                    page.set(0);
                                    spawn_local(async move {
                                        if let Err(error) = open_conversation(&current.workspace, &target).await {
                                            status.set(format!("Could not open conversation: {error}"));
                                        }
                                    });
                                }>{format!("{} ↗", label)}</button>
                                <span class="small muted">{move || if data.get().conversation_complete(&status_id) { "Complete" } else { "Incomplete" }}</span>
                            </div>
                        }
                    }/>
                </div>
                <Show when=move || !conversation.get().is_empty()>
                    <h3>"Threads in this conversation"</h3>
                    <div class="container-list">
                        <For each=move || data.get().threads.into_iter().filter(|item| item.channel == conversation.get()).collect::<Vec<_>>()
                            key=|item| format!("{}:{}", item.channel, item.root)
                            children=move |item| {
                                let local_root = item.root.clone();
                                let open_root = item.root.clone();
                                let channel = item.channel.clone();
                                let complete = item.status == ScanStatus::Complete;
                                view! {
                                    <div class="controls container-row">
                                        <button on:click=move |_| { thread.set(local_root.clone()); page.set(0); }>
                                            "Filter this thread"
                                        </button>
                                        <button on:click=move |_| {
                                            let current = data.get_untracked();
                                            let selected_root = open_root.clone();
                                            let selected_channel = channel.clone();
                                            thread.set(selected_root.clone());
                                            page.set(0);
                                            spawn_local(async move {
                                                let result = if let Some(captured) = token.get_untracked() {
                                                    open_message(&captured, &current.workspace, &selected_channel, &selected_root).await
                                                } else { Err("Recapture Slack session before navigating to a thread".to_owned()) };
                                                if let Err(error) = result { status.set(format!("Cannot open thread: {error}")); }
                                            });
                                        }>{format!("Thread {} · {} replies ↗", item.root, item.reply_count)}</button>
                                        <span class="small muted">{if complete { "Complete" } else { "Incomplete" }}</span>
                                    </div>
                                }
                            }
                        />
                    </div>
                    <button on:click=move |_| { thread.set(String::new()); page.set(0); }>"All messages in selected conversation"</button>
                </Show>
                <div class="controls">
                    <select aria-label="Message category" on:change=move |event| {
                        category.set(event_target_value(&event));
                        conversation.set(String::new());
                        thread.set(String::new());
                        page.set(0);
                    }>
                        <option value="all">"All messages"</option>
                        <option value="public">"Public channels"</option>
                        <option value="private">"Private channels"</option>
                        <option value="dm">"Direct messages"</option>
                        <option value="group">"Group DMs"</option>
                        <option value="threads">"Thread replies"</option>
                    </select>
                    <select aria-label="Conversation" on:change=move |event| { conversation.set(event_target_value(&event)); thread.set(String::new()); page.set(0); }>
                        <option value="">"Every conversation"</option>
                        <For each=move || data.get().conversations key=|item| item.id.clone() children=move |item| {
                            view! { <option value=item.id.clone()>{item.label}</option> }
                        }/>
                    </select>
                    <input type="text" placeholder="Filter loaded message text" aria-label="Message text filter"
                        on:input=move |event| { query.set(event_target_value(&event)); page.set(0); } />
                </div>
                <div class="controls" style="margin-top:12px">
                    <button on:click=select_page disabled=move || busy.get()>"Select displayed page"</button>
                    <button on:click=select_visible disabled=move || busy.get()>"Select ALL matching loaded results"</button>
                    <button on:click=move |_| selected.set(BTreeSet::new()) disabled=move || busy.get()>"Clear selection"</button>
                    <strong class="count">{move || format!("{} selected", selected.get().len())}</strong>
                    <span class="muted small">{move || format!("{} shown", visible_messages(&data.get(), &category.get(), &conversation.get(), &thread.get(), &query.get()).len())}</span>
                </div>
                <div class="controls" style="margin-top:10px">
                    <button on:click=move |_| page.update(|value| *value = value.saturating_sub(1))
                        disabled=move || page.get() == 0>"Previous page"</button>
                    <span class="small muted">{move || format!("Page {} of {}", page.get().saturating_add(1),
                        visible_messages(&data.get(), &category.get(), &conversation.get(), &thread.get(), &query.get())
                            .len().saturating_sub(1).checked_div(PAGE_SIZE).unwrap_or(0).saturating_add(1))}</span>
                    <button on:click=move |_| page.update(|value| *value = value.saturating_add(1))
                        disabled=move || page.get().saturating_add(1).saturating_mul(PAGE_SIZE)
                            >= visible_messages(&data.get(), &category.get(), &conversation.get(), &thread.get(), &query.get()).len()>"Next page"</button>
                </div>
                <For
                    each=move || {
                        let page_items: Vec<_> = visible_messages(&data.get(), &category.get(), &conversation.get(), &thread.get(), &query.get())
                            .into_iter().skip(page.get().saturating_mul(PAGE_SIZE)).take(PAGE_SIZE).collect();
                        page_items
                    }
                    key=|item| item.identifier()
                    children=move |item| {
                        let key = item.identifier();
                        let checkbox_key = key.clone();
                        let checked_key = key.clone();
                        let target_channel = item.channel.clone();
                        let target_timestamp = item.timestamp.clone();
                        let target_parent = item.parent.clone();
                        let open_click = move |_| {
                            let current = data.get_untracked();
                            let selected_channel = target_channel.clone();
                            let selected_timestamp = target_timestamp.clone();
                            conversation.set(selected_channel.clone());
                            thread.set(target_parent.clone().unwrap_or_default());
                            page.set(0);
                            spawn_local(async move {
                                let result = if let Some(captured) = token.get_untracked() {
                                    open_message(&captured, &current.workspace, &selected_channel, &selected_timestamp).await
                                } else { Err("Recapture the Slack token before opening a message".to_owned()) };
                                if let Err(error) = result {
                                    status.set(format!("Cannot open message: {error}"));
                                }
                            });
                        };
                        view! {
                            <div class="row"><div class="item">
                                <input type="checkbox" aria-label="Select message" disabled=move || busy.get()
                                    prop:checked=move || selected.get().contains(&checked_key)
                                    on:change=move |event| {
                                        let checked = event_target_checked(&event);
                                        selected.update(|keys| {
                                            if checked { keys.insert(checkbox_key.clone()); }
                                            else { keys.remove(&checkbox_key); }
                                        });
                                    } />
                                <div>
                                    <button class="message-link" on:click=open_click>{item.preview.clone()} " ↗"</button>
                                    <div class="meta">{format!("{} · {}{}", item.conversation_label,
                                        item.timestamp, if item.parent.is_some() { " · thread reply" } else { "" })}</div>
                                </div>
                            </div></div>
                        }
                    }
                />
            </section>
            <section>
                <h2>"4. Confirm a deletion batch"</h2>
                <Show when=move || !cfg!(feature = "live-delete")>
                    <p class="warning small">"Preview safety lock: live deletion is disabled in this build."</p>
                </Show>
                <p class="warning small">"Permanent deletion. Confirm only the checked messages. The count must match exactly. Scan coverage may be incomplete."</p>
                <div class="controls">
                    <input type="number" min="1" placeholder="Exact selection count" aria-label="Exact selected message count"
                        on:input=move |event| typed_count.set(event_target_value(&event)) />
                    <button class="danger" on:click=create_job
                        disabled=move || busy.get() || data.get().job.is_some() || selected.get().is_empty()>"Create paused batch"</button>
                </div>
                <Show when=move || data.get().job.is_some()>
                    <div class="controls" style="margin-top:14px">
                        <button class="danger" on:click=resume disabled=move || busy.get()>"Resume approved deletions"</button>
                        <button on:click=move |_| stop.set(true) disabled=move || !busy.get()>"Pause"</button>
                        <button on:click=clear_job disabled=move || busy.get()>"Discard remaining job"</button>
                    </div>
                    <div class="small muted" style="margin-top:8px">"Closed browser or interrupted attempts never auto-resume. Unknown outcomes are never retried automatically."</div>
                    <For each=move || data.get().job.map_or_else(Vec::new, |job| job.entries)
                        key=|item| item.message.identifier()
                        children=move |item| {
                            view! { <div class="jobrow small"><span>{item.message.conversation_label} " · " {item.message.timestamp}</span>
                                <strong>{state_label(item.status)}</strong></div> }
                        }
                    />
                </Show>
            </section>
        </main>
    }
}

/// Compiles the entirely Rust-authored client and mounts it in the sidebar document.
fn main() {
    mount_to_body(|| view! { <App /> });
}
