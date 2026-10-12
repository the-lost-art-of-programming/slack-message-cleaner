//! Narrow Firefox WebExtension interop without handwritten JavaScript.

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use leptos::prelude::{RwSignal, Set};
use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};

/// Safely reads an object member without evaluating arbitrary code.
fn property(value: &JsValue, key: &str) -> Result<JsValue, String> {
    Reflect::get(value, &JsValue::from_str(key)).map_err(|_| format!("Browser API lacks {key}"))
}

/// Looks up the browser's privileged extension API.
fn api() -> Result<JsValue, String> {
    property(&js_sys::global(), "browser")
}

/// Invokes a JavaScript method with its owning object as `this`.
fn call(target: &JsValue, method: &str, arguments: &[JsValue]) -> Result<JsValue, String> {
    let function = property(target, method)?
        .dyn_into::<Function>()
        .map_err(|_| format!("Browser method {method} is missing"))?;
    let arguments = arguments.iter().fold(Array::new(), |array, argument| {
        array.push(argument);
        array
    });
    function
        .apply(target, &arguments)
        .map_err(|_| format!("Browser method {method} failed"))
}

/// Resolves a standard promise from a WebExtensions API.
async fn promised(target: &JsValue, method: &str, arguments: &[JsValue]) -> Result<JsValue, String> {
    let response = call(target, method, arguments)?;
    let promise = response
        .dyn_into::<Promise>()
        .map_err(|_| format!("Browser method {method} did not return a Promise"))?;
    JsFuture::from(promise)
        .await
        .map_err(|_| format!("Browser {method} request rejected"))
}

/// Selects either ephemeral session or durable local browser storage.
fn storage(area: &str) -> Result<JsValue, String> {
    property(&property(&api()?, "storage")?, area)
}

/// Retrieves the Slack token that lasts only for this Firefox browser session.
pub(crate) async fn get_session_token() -> Result<Option<String>, String> {
    let target = storage("session")?;
    let record = promised(&target, "get", &[JsValue::from_str("slack_session_token")]).await?;
    Ok(property(&record, "slack_session_token")?
        .as_string()
        .filter(|value| value.starts_with("xox") && value.len() <= 2048))
}

/// Writes a token to Firefox's in-memory, session-scoped storage only.
pub(crate) async fn save_session_token(token: &str) -> Result<(), String> {
    let record = Object::new();
    Reflect::set(
        &record,
        &JsValue::from_str("slack_session_token"),
        &JsValue::from_str(token),
    )
    .map_err(|_| "Cannot create session token record".to_owned())?;
    let _response = promised(&storage("session")?, "set", &[record.into()]).await?;
    Ok(())
}

/// Erases captured credentials without clearing the user's review checkpoints.
pub(crate) async fn clear_session_token() -> Result<(), String> {
    let _response = promised(
        &storage("session")?,
        "remove",
        &[JsValue::from_str("slack_session_token")],
    )
    .await?;
    Ok(())
}

/// Stores review state as a versioned JSON document; never call with credentials.
pub(crate) async fn save_local<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
    let serialized = serde_json::to_string(value).map_err(|_| "Cannot serialize review checkpoint".to_owned())?;
    let record = Object::new();
    Reflect::set(&record, &JsValue::from_str(key), &JsValue::from_str(&serialized))
        .map_err(|_| "Cannot create review checkpoint record".to_owned())?;
    let _response = promised(&storage("local")?, "set", &[record.into()]).await?;
    Ok(())
}

/// Loads a durable review state, deferring its integrity checks to the domain layer.
pub(crate) async fn get_local<T: DeserializeOwned>(key: &str) -> Result<Option<T>, String> {
    let record = promised(&storage("local")?, "get", &[JsValue::from_str(key)]).await?;
    property(&record, key)?
        .as_string()
        .map(|serialized| {
            serde_json::from_str(&serialized).map_err(|_| "Saved checkpoint is not valid JSON".to_owned())
        })
        .transpose()
}

/// Extracts Slack's own form-body session token from a browser request event.
fn token_from_request(details: &JsValue) -> Option<String> {
    let body = property(details, "requestBody").ok()?;
    if let Ok(form) = property(&body, "formData") {
        if let Ok(tokens) = property(&form, "token") {
            if !tokens.is_null_or_undefined() {
                let token = Array::from(&tokens).get(0).as_string();
                if token.as_deref().is_some_and(valid_token) {
                    return token;
                }
            }
        }
    }
    let raw = Array::from(&property(&body, "raw").ok()?).get(0);
    let bytes = property(&raw, "bytes").ok()?;
    let utf8 = String::from_utf8(Uint8Array::new(&bytes).to_vec()).ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&utf8).ok()?;
    params.get("token").filter(|token| valid_token(token))
}

/// Rejects malformed or unbounded captured values.
fn valid_token(token: &str) -> bool {
    token.starts_with("xox") && token.len() <= 2048 && !token.contains(char::is_whitespace)
}

/// Observes Slack form submissions only while the extension sidebar remains open.
///
/// The user must refresh an authenticated Slack tab after opening this sidebar.
/// Tokens never leave Firefox session storage other than requests sent to Slack.
pub(crate) fn register_capture(token: RwSignal<Option<String>>, status: RwSignal<String>) -> Result<(), String> {
    let browser = api()?;
    let web_request = property(&browser, "webRequest")?;
    let event = property(&web_request, "onBeforeRequest")?;
    let callback = Closure::<dyn FnMut(JsValue)>::new(move |details: JsValue| {
        if let Some(captured) = token_from_request(&details) {
            token.set(Some(captured.clone()));
            status.set("Session captured. Select Connect to verify your account.".to_owned());
            spawn_local(async move {
                let _saved = save_session_token(&captured).await;
            });
        }
    });
    let filter = Object::new();
    let urls = Array::new();
    urls.push(&JsValue::from_str("https://*.slack.com/api/*"));
    Reflect::set(&filter, &JsValue::from_str("urls"), &urls)
        .map_err(|_| "Cannot configure Slack request listener".to_owned())?;
    let extras = Array::new();
    extras.push(&JsValue::from_str("requestBody"));
    let _result = call(
        &event,
        "addListener",
        &[callback.as_ref().clone(), filter.into(), extras.into()],
    )?;
    callback.forget();
    Ok(())
}

// ---------------------------------------------------------------------------
// Navigating the signed-in Slack tab
// ---------------------------------------------------------------------------

/// Opens a verified Slack URL in an existing Slack tab when possible.
///
/// Prefer a tab displaying the same workspace. If that is not available,
/// navigate the first Slack tab; create a new tab only when no Slack tab exists.
/// No token is added to the URL or handed to the tabs API.
///
/// # Errors
/// Reports unavailable tabs permissions or browser navigation failures.
pub(crate) async fn navigate_slack(url: &str, workspace: &str) -> Result<(), String> {
    // This is a second trust-boundary check; the caller also validates
    // canonical message permalinks against exact message IDs.
    let is_slack = url.starts_with("https://app.slack.com/client/")
        || url.starts_with("https://slack.com/archives/")
        || url
            .split_once("https://")
            .and_then(|(_, rest)| rest.split_once('/'))
            .is_some_and(|(host, _)| host.ends_with(".slack.com") && !host.contains(['@', ':', '\\', '#']));
    if !is_slack {
        return Err("Refusing to open a non-Slack URL".to_owned());
    }

    let tabs = property(&api()?, "tabs")?;
    let filter = Object::new();
    let patterns = Array::new();
    patterns.push(&JsValue::from_str("https://app.slack.com/*"));
    patterns.push(&JsValue::from_str("https://*.slack.com/*"));
    Reflect::set(&filter, &JsValue::from_str("url"), &patterns)
        .map_err(|_| "Cannot query existing Slack tabs".to_owned())?;
    let matching = promised(&tabs, "query", &[filter.into()]).await?;
    let open_tabs = Array::from(&matching);
    let workspace_prefix = format!("https://app.slack.com/client/{workspace}");
    let existing = open_tabs
        .iter()
        .find(|tab| {
            property(tab, "url")
                .ok()
                .and_then(|url| url.as_string())
                .and_then(|url| url.strip_prefix(&workspace_prefix).map(str::to_owned))
                .is_some_and(|suffix| {
                    suffix.is_empty() || suffix.starts_with('/') || suffix.starts_with('?') || suffix.starts_with('#')
                })
        })
        .or_else(|| open_tabs.iter().next());

    let options = Object::new();
    Reflect::set(&options, &JsValue::from_str("url"), &JsValue::from_str(url))
        .map_err(|_| "Cannot set Slack navigation URL".to_owned())?;
    Reflect::set(&options, &JsValue::from_str("active"), &JsValue::TRUE)
        .map_err(|_| "Cannot activate Slack navigation".to_owned())?;
    if let Some(tab) = existing {
        let id = property(&tab, "id")?;
        let updated = promised(&tabs, "update", &[id, options.into()]).await?;
        let window_id = property(&updated, "windowId")?;
        if !window_id.is_null_or_undefined() {
            let settings = Object::new();
            Reflect::set(&settings, &JsValue::from_str("focused"), &JsValue::TRUE)
                .map_err(|_| "Cannot request Slack window focus".to_owned())?;
            let windows = property(&api()?, "windows")?;
            let _focused = promised(&windows, "update", &[window_id, settings.into()]).await?;
        }
    } else {
        let _tab = promised(&tabs, "create", &[options.into()]).await?;
    }
    Ok(())
}
