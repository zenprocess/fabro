use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path};
use std::str::FromStr as _;

use croner::Cron;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::AutomationValidationError;
use crate::id::{AutomationId, AutomationTriggerId};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AutomationRevision(String);

impl AutomationRevision {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for AutomationRevision {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for AutomationRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepositorySlug(String);

impl RepositorySlug {
    pub fn new(value: impl Into<String>) -> Result<Self, AutomationValidationError> {
        let value = value.into();
        validate_repository_slug(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn owner_repo(&self) -> (&str, &str) {
        self.0
            .split_once('/')
            .expect("repository slug validation guarantees owner/repo")
    }
}

impl AsRef<str> for RepositorySlug {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for RepositorySlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for RepositorySlug {
    type Error = AutomationValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for RepositorySlug {
    type Error = AutomationValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for RepositorySlug {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RepositorySlug {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GitRefSelector(String);

impl GitRefSelector {
    pub fn new(value: impl Into<String>) -> Result<Self, AutomationValidationError> {
        let value = value.into();
        validate_git_ref(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for GitRefSelector {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for GitRefSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for GitRefSelector {
    type Error = AutomationValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for GitRefSelector {
    type Error = AutomationValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for GitRefSelector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GitRefSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkflowSlug(String);

impl WorkflowSlug {
    pub fn new(value: impl Into<String>) -> Result<Self, AutomationValidationError> {
        let value = value.into();
        validate_workflow_selector(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for WorkflowSlug {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for WorkflowSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for WorkflowSlug {
    type Error = AutomationValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for WorkflowSlug {
    type Error = AutomationValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for WorkflowSlug {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WorkflowSlug {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Automation {
    pub id:          AutomationId,
    pub revision:    AutomationRevision,
    pub name:        String,
    #[serde(default)]
    pub description: Option<String>,
    pub enabled:     bool,
    pub target:      AutomationTarget,
    pub triggers:    Vec<AutomationTrigger>,
}

impl Automation {
    pub fn api_trigger(&self) -> Option<&ApiTrigger> {
        self.triggers.iter().find_map(AutomationTrigger::as_api)
    }

    pub(crate) fn from_persisted(
        id: AutomationId,
        revision: AutomationRevision,
        persisted: PersistedAutomation,
    ) -> Result<Self, AutomationValidationError> {
        let automation = Self {
            id,
            revision,
            name: persisted.name,
            description: persisted.description,
            enabled: persisted.enabled,
            target: persisted.target,
            triggers: persisted.triggers,
        };
        automation.validate()?;
        Ok(automation)
    }

    pub(crate) fn to_persisted(&self) -> PersistedAutomation {
        PersistedAutomation {
            name:        self.name.clone(),
            description: self.description.clone(),
            enabled:     self.enabled,
            target:      self.target.clone(),
            triggers:    self.triggers.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), AutomationValidationError> {
        validate_name(&self.name)?;
        validate_triggers(&self.triggers)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationTarget {
    pub repository: RepositorySlug,
    #[serde(rename = "ref")]
    pub ref_:       GitRefSelector,
    pub workflow:   WorkflowSlug,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationTrigger {
    Api(ApiTrigger),
    Schedule(ScheduleTrigger),
}

impl AutomationTrigger {
    pub fn id(&self) -> &AutomationTriggerId {
        match self {
            Self::Api(trigger) => &trigger.id,
            Self::Schedule(trigger) => &trigger.id,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Api(trigger) => trigger.enabled,
            Self::Schedule(trigger) => trigger.enabled,
        }
    }

    pub fn as_api(&self) -> Option<&ApiTrigger> {
        match self {
            Self::Api(trigger) => Some(trigger),
            Self::Schedule(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiTrigger {
    pub id:      AutomationTriggerId,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTrigger {
    pub id:         AutomationTriggerId,
    #[serde(default = "default_true")]
    pub enabled:    bool,
    pub expression: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationDraft {
    pub id:          AutomationId,
    pub name:        String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled:     Option<bool>,
    pub target:      AutomationTarget,
    pub triggers:    Vec<AutomationTrigger>,
}

impl AutomationDraft {
    pub(crate) fn into_automation(
        self,
        revision: AutomationRevision,
    ) -> Result<Automation, AutomationValidationError> {
        let automation = Automation {
            id: self.id,
            revision,
            name: self.name,
            description: self.description,
            enabled: self.enabled.unwrap_or(true),
            target: self.target,
            triggers: self.triggers,
        };
        automation.validate()?;
        Ok(automation)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationReplace {
    pub name:        String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub enabled:     bool,
    pub target:      AutomationTarget,
    pub triggers:    Vec<AutomationTrigger>,
}

impl AutomationReplace {
    pub(crate) fn into_automation(
        self,
        id: AutomationId,
        revision: AutomationRevision,
    ) -> Result<Automation, AutomationValidationError> {
        let automation = Automation {
            id,
            revision,
            name: self.name,
            description: self.description,
            enabled: self.enabled,
            target: self.target,
            triggers: self.triggers,
        };
        automation.validate()?;
        Ok(automation)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationPatch {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name:        Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled:     Option<bool>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target:      Option<AutomationTarget>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggers:    Option<Vec<AutomationTrigger>>,
}

impl AutomationPatch {
    pub(crate) fn apply_to(
        self,
        existing: &Automation,
        revision: AutomationRevision,
    ) -> Result<Automation, AutomationValidationError> {
        let automation = Automation {
            id: existing.id.clone(),
            revision,
            name: self.name.unwrap_or_else(|| existing.name.clone()),
            description: self.description.unwrap_or_else(|| existing.description.clone()),
            enabled: self.enabled.unwrap_or(existing.enabled),
            target: self.target.unwrap_or_else(|| existing.target.clone()),
            triggers: self.triggers.unwrap_or_else(|| existing.triggers.clone()),
        };
        automation.validate()?;
        Ok(automation)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PersistedAutomation {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub target: AutomationTarget,
    #[serde(default)]
    pub triggers: Vec<AutomationTrigger>,
}

fn default_true() -> bool {
    true
}

fn deserialize_optional_nullable<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

fn validate_name(name: &str) -> Result<(), AutomationValidationError> {
    if name.trim().is_empty() {
        return Err(AutomationValidationError::EmptyName);
    }
    Ok(())
}

fn validate_triggers(triggers: &[AutomationTrigger]) -> Result<(), AutomationValidationError> {
    let mut ids = HashSet::new();
    let mut api_count = 0_u8;

    for trigger in triggers {
        if !ids.insert(trigger.id().as_str()) {
            return Err(AutomationValidationError::DuplicateTriggerId(
                trigger.id().to_string(),
            ));
        }

        match trigger {
            AutomationTrigger::Api(_) => {
                api_count = api_count.saturating_add(1);
                if api_count > 1 {
                    return Err(AutomationValidationError::TooManyApiTriggers);
                }
            }
            AutomationTrigger::Schedule(schedule) => {
                validate_schedule_expression(&schedule.expression)?;
            }
        }
    }

    Ok(())
}

fn validate_schedule_expression(expression: &str) -> Result<(), AutomationValidationError> {
    let is_five_field = expression.split_whitespace().count() == 5;
    if expression.trim().is_empty() || !is_five_field {
        return Err(AutomationValidationError::InvalidScheduleExpression(
            expression.to_string(),
        ));
    }

    Cron::from_str(expression).map_err(|_| {
        AutomationValidationError::InvalidScheduleExpression(expression.to_string())
    })?;
    Ok(())
}

fn validate_repository_slug(value: &str) -> Result<(), AutomationValidationError> {
    let Some((owner, repo)) = value.split_once('/') else {
        return Err(AutomationValidationError::InvalidRepositorySlug(
            value.to_string(),
        ));
    };

    if repo.contains('/') || !valid_github_segment(owner, 39) || !valid_github_segment(repo, 100) {
        return Err(AutomationValidationError::InvalidRepositorySlug(
            value.to_string(),
        ));
    }

    Ok(())
}

fn valid_github_segment(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn validate_git_ref(value: &str) -> Result<(), AutomationValidationError> {
    let invalid = value.is_empty()
        || value.starts_with('-')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("..")
        || value.contains("//")
        || value.bytes().any(|b| {
            b.is_ascii_control()
                || b.is_ascii_whitespace()
                || matches!(b, b'\\' | b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b']' | b'{' | b'}')
        });

    if invalid {
        return Err(AutomationValidationError::InvalidGitRef(value.to_string()));
    }
    Ok(())
}

fn validate_workflow_selector(value: &str) -> Result<(), AutomationValidationError> {
    if value.trim().is_empty() || value.bytes().any(|b| b.is_ascii_control()) {
        return Err(AutomationValidationError::InvalidWorkflowSelector(
            value.to_string(),
        ));
    }

    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return Err(AutomationValidationError::InvalidWorkflowSelector(
            value.to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn automation_id() -> AutomationId {
        AutomationId::try_from("nightly-deps").expect("valid id")
    }

    fn revision() -> AutomationRevision {
        AutomationRevision::new("revision")
    }

    fn parse_toml(input: &str) -> Result<Automation, AutomationValidationError> {
        let persisted: PersistedAutomation = toml::from_str(input).expect("valid toml syntax");
        Automation::from_persisted(automation_id(), revision(), persisted)
    }

    #[test]
    fn parses_valid_toml() {
        let automation = parse_toml(
            r#"
name = "Nightly dependency update"
description = "Open a PR for dependency updates."
enabled = true

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "dependency-update"

[[triggers]]
id = "api"
type = "api"
enabled = false

[[triggers]]
id = "nightly"
type = "schedule"
enabled = true
expression = "0 3 * * *"
"#,
        )
        .expect("automation should parse");

        assert_eq!(automation.id.as_str(), "nightly-deps");
        assert_eq!(automation.description.as_deref(), Some("Open a PR for dependency updates."));
        assert!(matches!(automation.triggers[0], AutomationTrigger::Api(_)));
        assert!(matches!(automation.triggers[1], AutomationTrigger::Schedule(_)));
    }

    #[test]
    fn applies_toml_defaults() {
        let automation = parse_toml(
            r#"
name = "Nightly dependency update"

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "dependency-update"

[[triggers]]
id = "api"
type = "api"
"#,
        )
        .expect("automation should parse");

        assert!(automation.enabled);
        assert_eq!(automation.description, None);
        let AutomationTrigger::Api(api) = &automation.triggers[0] else {
            panic!("expected api trigger");
        };
        assert!(api.enabled);
    }

    #[test]
    fn rejects_invalid_automation_ids() {
        for id in ["", "-bad", "Bad", "bad_underscore", &"a".repeat(64)] {
            assert!(AutomationId::try_from(id).is_err(), "{id} should be invalid");
        }
    }

    #[test]
    fn rejects_invalid_trigger_ids() {
        for id in ["", "-bad", "Bad", "bad.dot", &"a".repeat(64)] {
            assert!(
                AutomationTriggerId::try_from(id).is_err(),
                "{id} should be invalid"
            );
        }
    }

    #[test]
    fn rejects_duplicate_trigger_ids() {
        let err = parse_toml(
            r#"
name = "Nightly dependency update"

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "dependency-update"

[[triggers]]
id = "api"
type = "api"

[[triggers]]
id = "api"
type = "schedule"
expression = "0 3 * * *"
"#,
        )
        .expect_err("duplicate trigger should fail");

        assert!(matches!(err, AutomationValidationError::DuplicateTriggerId(_)));
    }

    #[test]
    fn rejects_two_api_triggers() {
        let err = parse_toml(
            r#"
name = "Nightly dependency update"

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "dependency-update"

[[triggers]]
id = "api"
type = "api"

[[triggers]]
id = "other_api"
type = "api"
"#,
        )
        .expect_err("second api trigger should fail");

        assert!(matches!(err, AutomationValidationError::TooManyApiTriggers));
    }

    #[test]
    fn rejects_invalid_repository_slug() {
        for repository in ["owner", "owner/repo/extra", "../repo", "owner/bad/repo"] {
            assert!(
                RepositorySlug::try_from(repository).is_err(),
                "{repository} should be invalid"
            );
        }
    }

    #[test]
    fn rejects_invalid_schedule_expression() {
        let err = parse_toml(
            r#"
name = "Nightly dependency update"

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "dependency-update"

[[triggers]]
id = "nightly"
type = "schedule"
expression = "not a cron"
"#,
        )
        .expect_err("invalid schedule should fail");

        assert!(matches!(err, AutomationValidationError::InvalidScheduleExpression(_)));
    }
}
