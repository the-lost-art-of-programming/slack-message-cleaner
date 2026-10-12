//! Validated Slack navigation locations and strict permalink verification.
//!
//! Links are treated as untrusted until they are constrained to the selected
//! workspace, conversation and message. Neither bearer tokens nor message
//! contents belong in a navigation URL.

use crate::identity::{ConversationId, MessageTs, WorkspaceId};

// ---------------------------------------------------------------------------
// Workspace and conversation navigation
// ---------------------------------------------------------------------------

/// A direct, browser-safe Slack workspace URL made from validated identifiers.
///
/// # Examples
///
/// ```
/// use cleaner_core::identity::WorkspaceId;
/// use cleaner_core::links::workspace_url;
/// let team = WorkspaceId::parse("T123")?;
/// assert_eq!(workspace_url(&team), "https://app.slack.com/client/T123");
/// # Ok::<(), cleaner_core::identity::IdentityError>(())
/// ```
#[must_use]
pub fn workspace_url(team: &WorkspaceId) -> String {
    format!("https://app.slack.com/client/{}", team.as_str())
}

/// A browser location for a channel, private channel, DM or group DM.
///
/// # Examples
///
/// ```
/// use cleaner_core::identity::{ConversationId, WorkspaceId};
/// use cleaner_core::links::conversation_url;
/// let team = WorkspaceId::parse("T123")?;
/// let channel = ConversationId::parse("D456")?;
/// assert_eq!(conversation_url(&team, &channel), "https://app.slack.com/client/T123/D456");
/// # Ok::<(), cleaner_core::identity::IdentityError>(())
/// ```
#[must_use]
pub fn conversation_url(team: &WorkspaceId, conversation: &ConversationId) -> String {
    format!("{}/{}", workspace_url(team), conversation.as_str())
}

// ---------------------------------------------------------------------------
// Permalink validation
// ---------------------------------------------------------------------------

/// Confirms that a Slack-returned permalink points to the *exact* requested message.
///
/// The Slack API may return a different workspace subdomain depending on the
/// signed-in organization. We therefore validate its HTTPS host and immutable
/// archive path, rather than interpolating an unverified workspace subdomain.
///
/// # Examples
///
/// ```
/// use cleaner_core::identity::{ConversationId, MessageTs};
/// use cleaner_core::links::permalink_is_for;
/// let conversation = ConversationId::parse("C123")?;
/// let timestamp = MessageTs::parse("12345.000006")?;
/// assert!(permalink_is_for(
///     "https://example.slack.com/archives/C123/p12345000006?thread_ts=12344.000001&cid=C123",
///     &conversation, &timestamp
/// ));
/// assert!(!permalink_is_for(
///     "https://example.slack.com.evil.invalid/archives/C123/p12345000006",
///     &conversation, &timestamp
/// ));
/// # Ok::<(), cleaner_core::identity::IdentityError>(())
/// ```
#[must_use]
pub fn permalink_is_for(url: &str, conversation: &ConversationId, timestamp: &MessageTs) -> bool {
    let Some(after_scheme) = url.strip_prefix("https://") else {
        return false;
    };
    let Some((host, path)) = after_scheme.split_once('/') else {
        return false;
    };
    if !(host == "slack.com" || host.ends_with(".slack.com")) {
        return false;
    }
    if host.chars().any(|ch| matches!(ch, '@' | ':' | '\\' | '#')) || path.contains('#') || path.contains('\\') {
        return false;
    }

    let expected = format!("archives/{}/p{}", conversation.as_str(), timestamp.as_str().replace('.', ""));
    let actual = path.split_once('?').map_or(path, |(pathname, _)| pathname);
    actual == expected
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{conversation_url, permalink_is_for, workspace_url};
    use crate::identity::{ConversationId, IdentityError, MessageTs, WorkspaceId};

    /// Workspace and conversation links must use validated identifiers.
    #[test]
    fn channels_and_dms_use_the_same_workspace_scoped_route() -> Result<(), IdentityError> {
        let team = WorkspaceId::parse("T1")?;
        let channel = ConversationId::parse("C1")?;
        let dm = ConversationId::parse("D1")?;
        assert_eq!(workspace_url(&team), "https://app.slack.com/client/T1");
        assert_eq!(conversation_url(&team, &channel), "https://app.slack.com/client/T1/C1");
        assert_eq!(conversation_url(&team, &dm), "https://app.slack.com/client/T1/D1");
        Ok(())
    }

    /// Strict matching prevents deceptive domains and cross-message redirects.
    #[test]
    fn only_exact_slack_messages_can_open() -> Result<(), IdentityError> {
        let channel = ConversationId::parse("C1")?;
        let ts = MessageTs::parse("1.000001")?;
        assert!(permalink_is_for("https://acme.slack.com/archives/C1/p1000001", &channel, &ts));
        for rejected in [
            "http://acme.slack.com/archives/C1/p1000001",
            "https://acme.slack.com.evil.com/archives/C1/p1000001",
            "https://evil@acme.slack.com/archives/C1/p1000001",
            "https://acme.slack.com:443/archives/C1/p1000001",
            "https://acme.slack.com/archives/C2/p1000001",
            "https://acme.slack.com/archives/C1/p1000002",
            "https://acme.slack.com/archives/C1/p1000001/other",
        ] {
            assert!(!permalink_is_for(rejected, &channel, &ts), "invalid link: {rejected}");
        }
        Ok(())
    }
}
