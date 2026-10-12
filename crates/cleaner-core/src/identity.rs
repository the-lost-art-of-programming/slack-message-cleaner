//! Validated account and message identities used across every safety boundary.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum accepted length for an opaque Slack workspace, user, or conversation ID.
const MAX_IDENTIFIER_BYTES: usize = 128;
/// Maximum accepted number of decimal digits in a Slack timestamp's seconds field.
const MAX_SECONDS_DIGITS: usize = 20;
/// Maximum accepted number of decimal digits in a Slack timestamp's subsecond field.
const MAX_FRACTION_DIGITS: usize = 6;

/// Error when external identifiers cannot be safely treated as Slack IDs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum IdentityError {
    /// A workspace, user, or conversation identifier is malformed.
    InvalidIdentifier,
    /// A message timestamp is not a bounded decimal timestamp.
    InvalidTimestamp,
}

impl fmt::Display for IdentityError {
    /// Describes invalid input without including potentially sensitive raw values.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier => f.write_str("invalid Slack identifier"),
            Self::InvalidTimestamp => f.write_str("invalid Slack message timestamp"),
        }
    }
}

impl std::error::Error for IdentityError {}

/// Workspace identity, not a display name or user-editable label.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    /// Validates and preserves an opaque Slack workspace identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use cleaner_core::identity::WorkspaceId;
    /// # fn main() -> Result<(), cleaner_core::identity::IdentityError> {
    /// let team = WorkspaceId::parse("T123")?;
    /// assert_eq!(
    ///     team.as_str(),
    ///     "T123",
    ///     "valid identity must round-trip exactly"
    /// );
    /// assert!(
    ///     WorkspaceId::parse("invalid whitespace").is_err(),
    ///     "whitespace must fail"
    /// );
    /// # Ok(())
    /// # }
    /// ```

    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    pub fn parse(raw: &str) -> Result<Self, IdentityError> {
        validate_identifier(raw).map(|()| Self(raw.to_owned()))
    }

    /// Returns the validated identifier, preserving its exact byte representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Slack's stable user ID rather than an email or display name.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct UserId(String);

impl UserId {
    /// Validates a user identifier without assuming a particular ID prefix.
    ///
    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    pub fn parse(raw: &str) -> Result<Self, IdentityError> {
        validate_identifier(raw).map(|()| Self(raw.to_owned()))
    }

    /// Returns the exact validated user identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Slack conversation ID, including channels, DMs, and group DMs.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct ConversationId(String);

impl ConversationId {
    /// Validates a conversation identifier without relying on its prefix.
    ///
    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    pub fn parse(raw: &str) -> Result<Self, IdentityError> {
        validate_identifier(raw).map(|()| Self(raw.to_owned()))
    }

    /// Returns the exact validated conversation identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Slack message timestamp as a validated, *unrounded* string.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct MessageTs(String);

impl MessageTs {
    /// Accepts `seconds.fraction` without floating point conversion or rounding.
    ///
    /// # Examples
    ///
    /// ```
    /// use cleaner_core::identity::MessageTs;
    /// # fn main() -> Result<(), cleaner_core::identity::IdentityError> {
    /// let ts = MessageTs::parse("1712345678.000001")?;
    /// assert_eq!(
    ///     ts.as_str(),
    ///     "1712345678.000001",
    ///     "fractional precision is preserved"
    /// );
    /// assert!(
    ///     MessageTs::parse("1712345678").is_err(),
    ///     "missing fraction must fail"
    /// );
    /// # Ok(())
    /// # }
    /// ```

    /// # Errors
    /// Returns an error if required IDs, account ownership or a queue transition are invalid.
    pub fn parse(raw: &str) -> Result<Self, IdentityError> {
        let (seconds, fraction) = raw.split_once('.').ok_or(IdentityError::InvalidTimestamp)?;
        let seconds_valid = !seconds.is_empty()
            && seconds.len() <= MAX_SECONDS_DIGITS
            && seconds.bytes().all(|byte| byte.is_ascii_digit());
        let fraction_valid = !fraction.is_empty()
            && fraction.len() <= MAX_FRACTION_DIGITS
            && fraction.bytes().all(|byte| byte.is_ascii_digit());
        if !seconds_valid || !fraction_valid {
            return Err(IdentityError::InvalidTimestamp);
        }
        Ok(Self(raw.to_owned()))
    }

    /// Returns the exact decimal string for use as a Slack message identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The user's currently authenticated workspace/account combination.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Account {
    /// Workspace whose message ownership is being reviewed.
    workspace: WorkspaceId,
    /// User whose message ownership is being reviewed.
    user: UserId,
}

impl Account {
    /// Creates an account identity only from already validated IDs.
    #[must_use]
    pub fn new(workspace: WorkspaceId, user: UserId) -> Self {
        Self { workspace, user }
    }

    /// Returns the workspace bound to this account.
    #[must_use]
    pub fn workspace(&self) -> &WorkspaceId {
        &self.workspace
    }

    /// Returns the user bound to this account.
    #[must_use]
    pub fn user(&self) -> &UserId {
        &self.user
    }
}

/// Stable message key; text and UI array positions must never be used as IDs.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct MessageKey {
    /// Workspace containing the message.
    workspace: WorkspaceId,
    /// Conversation containing the message.
    conversation: ConversationId,
    /// Exact Slack message timestamp.
    timestamp: MessageTs,
}

impl MessageKey {
    /// Combines already validated identity components into a stable key.
    #[must_use]
    pub fn new(workspace: WorkspaceId, conversation: ConversationId, timestamp: MessageTs) -> Self {
        Self {
            workspace,
            conversation,
            timestamp,
        }
    }

    /// Returns the workspace component of this key.
    #[must_use]
    pub fn workspace(&self) -> &WorkspaceId {
        &self.workspace
    }

    /// Returns the conversation component of this key.
    #[must_use]
    pub fn conversation(&self) -> &ConversationId {
        &self.conversation
    }

    /// Returns the exact message timestamp component of this key.
    #[must_use]
    pub fn timestamp(&self) -> &MessageTs {
        &self.timestamp
    }
}

/// Checks an opaque identifier without storing unsafe or unbounded input.
fn validate_identifier(raw: &str) -> Result<(), IdentityError> {
    if raw.is_empty()
        || raw.len() > MAX_IDENTIFIER_BYTES
        || !raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(IdentityError::InvalidIdentifier);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ConversationId, IdentityError, MessageTs, UserId, WorkspaceId};

    /// Identifier validation rejects whitespace, emptiness, and long tokens.
    #[test]
    fn identifiers_reject_whitespace_and_unbounded_input() {
        assert_eq!(
            WorkspaceId::parse(" T123"),
            Err(IdentityError::InvalidIdentifier),
            "whitespace is forbidden"
        );
        assert_eq!(
            UserId::parse(""),
            Err(IdentityError::InvalidIdentifier),
            "empty user ID is forbidden"
        );
        assert_eq!(
            ConversationId::parse(&"C".repeat(129)),
            Err(IdentityError::InvalidIdentifier),
            "unbounded input is forbidden"
        );
    }

    /// Timestamp parsing preserves exact decimal precision.
    #[test]
    fn timestamps_are_exact_and_never_parsed_as_floating_point() -> Result<(), IdentityError> {
        let timestamp = MessageTs::parse("1712345678.000001")?;
        assert_eq!(
            timestamp.as_str(),
            "1712345678.000001",
            "timestamp representation must remain unchanged"
        );
        assert_eq!(
            MessageTs::parse("1712345678"),
            Err(IdentityError::InvalidTimestamp),
            "missing fraction is invalid"
        );
        assert_eq!(
            MessageTs::parse("1712345678.00000x"),
            Err(IdentityError::InvalidTimestamp),
            "nondecimal fraction is invalid"
        );
        assert_eq!(
            MessageTs::parse("1712345678.0000001"),
            Err(IdentityError::InvalidTimestamp),
            "overprecision is invalid"
        );
        Ok(())
    }
}
