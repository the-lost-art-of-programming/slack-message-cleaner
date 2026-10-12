//! Leptos browser UI: discovery, navigation, deliberate selection, and paused jobs.

#![forbid(unsafe_code)]

mod browser;
mod delete;
mod model;
mod scan;
mod slack;

use std::collections::BTreeSet;

use cleaner_core::message::ConversationKind;
use cleaner_core::queue::ItemState;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::model::{OwnedMessage, WorkspaceData};

/// Maximum number of message DOM rows rendered per review page.
const PAGE_SIZE: usize = 100;

/// Selects only items already retrieved for review, never an unloaded search universe.
#[must_use]
fn visible_messages(data: &WorkspaceData, category: &str, channel: &str, query: &str) -> Vec<OwnedMessage> {
    let phrase = query.to_lowercase();
    data.messages.iter().filter(|message| {
        let category_matches = match category {
            "public" => message.kind == ConversationKind::PublicChannel,
            "private" => message.kind == ConversationKind::PrivateChannel,
            "dm" => message.kind == ConversationKind::DirectMessage,
            "group" => message.kind == ConversationKind::GroupDirectMessage,
            "threads" => message.parent.is_some(),
            "all" => true,
            _ => false,
        };
        category_matches && (channel.is_empty() || message.channel == channel)
            && message.preview.to_lowercase().contains(&phrase)
    }).cloned().collect()
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
            }
            Ok(None) => {}
            Err(error) => status.set(format!("Session storage unavailable: {error}")),
        }
    });

    let connect = move |_| {
        if busy.get_untracked() { return; }
        busy.set(true);
        spawn_local(async move {
            let result = async {
                let captured = token.get_untracked().ok_or("No captured token. Refresh an authenticated Slack tab.")?;
                let (workspace, user) = delete::verify_account(&captured).await?;
                let key = scan::storage_key(&workspace, &user);
                let mut restored = browser::get_local::<WorkspaceData>(&key).await?
                    .unwrap_or_else(|| WorkspaceData::new(workspace.clone(), user.clone()));
                if restored.workspace != workspace || restored.user != user {
                    return Err("Saved data belongs to a different Slack account".to_owned());
                }
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
            }.await;
            match result {
                Ok((workspace, user)) => status.set(format!("Authenticated as {user} in {workspace}. Ready to scan.")),
                Err(error) => status.set(format!("Connection failed: {error}")),
            }
            busy.set(false);
        });
    };

    let scan = move |_| {
        if busy.get_untracked() || data.get_untracked().workspace.is_empty()
            || data.get_untracked().job.is_some() { return; }
        busy.set(true);
        stop.set(false);
        spawn_local(async move {
            let result = if let Some(captured) = token.get_untracked() {
                let current = data.get_untracked();
                match delete::verify_account(&captured).await {
                    Ok((workspace, user)) if workspace == current.workspace && user == current.user => {
                        scan::scan_all(&captured, current, data, status, stop).await
                    }
                    Ok(_) => Err("Active Slack account changed. Scan aborted.".to_owned()),
                    Err(error) => Err(error),
                }
            } else { Err("No authenticated Slack session".to_owned()) };
            if let Err(error) = result { status.set(error); }
            selected.set(BTreeSet::new());
            busy.set(false);
        });
    };

    let select_visible = move |_| {
        let items = visible_messages(&data.get_untracked(), &category.get_untracked(),
            &conversation.get_untracked(), &query.get_untracked());
        selected.update(|keys| {
            for item in items { keys.insert(item.identifier()); }
        });
    };

    let select_page = move |_| {
        let items = visible_messages(&data.get_untracked(), &category.get_untracked(),
            &conversation.get_untracked(), &query.get_untracked());
        selected.update(|keys| {
            for item in items.into_iter().skip(page.get_untracked().saturating_mul(PAGE_SIZE)).take(PAGE_SIZE) {
                keys.insert(item.identifier());
            }
        });
    };

    let create_job = move |_| {
        if busy.get_untracked() || data.get_untracked().job.is_some() { return; }
        let current = data.get_untracked();
        let chosen = selected.get_untracked();
        let count = typed_count.get_untracked().parse::<usize>().unwrap_or(0);
        let approved: Vec<OwnedMessage> = current.messages.iter()
            .filter(|message| chosen.contains(&message.identifier()))
            .cloned().collect();
        match delete::make_job(&current, &approved, count) {
            Ok(job) => {
                let mut snapshot = current;
                snapshot.job = Some(job);
                busy.set(true);
                spawn_local(async move {
                    match browser::save_local(&scan::storage_key(&snapshot.workspace, &snapshot.user), &snapshot).await {
                        Ok(()) => {
                            data.set(snapshot);
                            status.set("Job confirmed and paused. Select Resume to begin deletion.".to_owned());
                        }
                        Err(error) => status.set(format!("Job not queued: {error}")),
                    }
                    busy.set(false);
                });
            }
            Err(error) => status.set(format!("Batch was not created: {error}. Enter the exact selection count.")),
        }
    };

    let resume = move |_| {
        if busy.get_untracked() || data.get_untracked().job.is_none() { return; }
        busy.set(true);
        stop.set(false);
        spawn_local(async move {
            let result = if let Some(captured) = token.get_untracked() {
                delete::resume_job(&captured, data, status, stop).await
            } else { Err("Slack session must be recaptured before resuming".to_owned()) };
            if let Err(error) = result { status.set(format!("Deletion paused: {error}")); }
            busy.set(false);
        });
    };

    let clear_job = move |_| {
        if busy.get_untracked() { return; }
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
                }
                Err(error) => status.set(format!("Could not discard job: {error}")),
            }
            busy.set(false);
        });
    };

    let forget_token = move |_| {
        if busy.get_untracked() { return; }
        spawn_local(async move {
            match browser::clear_session_token().await {
                Ok(()) => { token.set(None); status.set("Credential erased; refresh Slack to recapture.".to_owned()); }
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
                <div class="controls">
                    <select aria-label="Message category" on:change=move |event| {
                        category.set(event_target_value(&event));
                        conversation.set(String::new());
                        page.set(0);
                    }>
                        <option value="all">"All messages"</option>
                        <option value="public">"Public channels"</option>
                        <option value="private">"Private channels"</option>
                        <option value="dm">"Direct messages"</option>
                        <option value="group">"Group DMs"</option>
                        <option value="threads">"Thread replies"</option>
                    </select>
                    <select aria-label="Conversation" on:change=move |event| { conversation.set(event_target_value(&event)); page.set(0); }>
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
                    <span class="muted small">{move || format!("{} shown", visible_messages(&data.get(), &category.get(), &conversation.get(), &query.get()).len())}</span>
                </div>
                <div class="controls" style="margin-top:10px">
                    <button on:click=move |_| page.update(|value| *value = value.saturating_sub(1))
                        disabled=move || page.get() == 0>"Previous page"</button>
                    <span class="small muted">{move || format!("Page {} of {}", page.get().saturating_add(1),
                        visible_messages(&data.get(), &category.get(), &conversation.get(), &query.get())
                            .len().saturating_sub(1).checked_div(PAGE_SIZE).unwrap_or(0).saturating_add(1))}</span>
                    <button on:click=move |_| page.update(|value| *value = value.saturating_add(1))
                        disabled=move || page.get().saturating_add(1).saturating_mul(PAGE_SIZE)
                            >= visible_messages(&data.get(), &category.get(), &conversation.get(), &query.get()).len()>"Next page"</button>
                </div>
                <For
                    each=move || {
                        visible_messages(&data.get(), &category.get(), &conversation.get(), &query.get())
                            .into_iter().skip(page.get().saturating_mul(PAGE_SIZE)).take(PAGE_SIZE).collect::<Vec<_>>()
                    }
                    key=|item| item.identifier()
                    children=move |item| {
                        let key = item.identifier();
                        let checkbox_key = key.clone();
                        let checked_key = key.clone();
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
                                <div><div class="preview">{item.preview.clone()}</div>
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
