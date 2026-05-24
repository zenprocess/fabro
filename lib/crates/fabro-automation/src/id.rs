use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::AutomationValidationError;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AutomationId(String);

impl AutomationId {
    pub fn new(value: impl Into<String>) -> Result<Self, AutomationValidationError> {
        Self::try_from(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for AutomationId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for AutomationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for AutomationId {
    type Error = AutomationValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_id(&value, IdKind::Automation)?;
        Ok(Self(value))
    }
}

impl TryFrom<&str> for AutomationId {
    type Error = AutomationValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_string())
    }
}

impl Serialize for AutomationId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AutomationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AutomationTriggerId(String);

impl AutomationTriggerId {
    pub fn new(value: impl Into<String>) -> Result<Self, AutomationValidationError> {
        Self::try_from(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for AutomationTriggerId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for AutomationTriggerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for AutomationTriggerId {
    type Error = AutomationValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_id(&value, IdKind::Trigger)?;
        Ok(Self(value))
    }
}

impl TryFrom<&str> for AutomationTriggerId {
    type Error = AutomationValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_string())
    }
}

impl Serialize for AutomationTriggerId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AutomationTriggerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy)]
enum IdKind {
    Automation,
    Trigger,
}

fn validate_id(value: &str, kind: IdKind) -> Result<(), AutomationValidationError> {
    let valid_len = (1..=63).contains(&value.len());
    let first_valid = value
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    let rest_valid = value.bytes().skip(1).all(|b| {
        b.is_ascii_lowercase()
            || b.is_ascii_digit()
            || b == b'-'
            || (matches!(kind, IdKind::Trigger) && b == b'_')
    });

    if valid_len && first_valid && rest_valid {
        return Ok(());
    }

    match kind {
        IdKind::Automation => Err(AutomationValidationError::InvalidAutomationId(
            value.to_string(),
        )),
        IdKind::Trigger => Err(AutomationValidationError::InvalidTriggerId(value.to_string())),
    }
}
