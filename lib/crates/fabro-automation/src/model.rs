use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path};
use std::str::FromStr;

use croner::parser::{CronParser, Seconds, Year};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use toml::de::Error as TomlDeError;
use toml_edit::ser::{Error as TomlEditSerError, to_document};

use crate::error::AutomationValidationError;
use crate::id::{AutomationId, AutomationTriggerId};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AutomationRevision(String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepositorySlug(String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GitRefSelector(String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkflowSlug(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Automation {
    pub id:          AutomationId,
    pub revision:    AutomationRevision,
    pub name:        String,
    pub description: Option<String>,
    pub enabled:     bool,
    pub target:      AutomationTarget,
    pub triggers:    Vec<AutomationTrigger>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationTarget {
    pub repository: RepositorySlug,
    #[serde(rename = "ref")]
    pub ref_:       GitRefSelector,
    pub workflow:   WorkflowSlug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationTrigger {
    Api(ApiTrigger),
    Schedule(ScheduleTrigger),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiTrigger {
    pub id:      AutomationTriggerId,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleTrigger {
    pub id:         AutomationTriggerId,
    #[serde(default = "default_true")]
    pub enabled:    bool,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationReplace {
    pub name:        String,
    #[serde(default)]
    pub description: Option<String>,
    pub enabled:     bool,
    pub target:      AutomationTarget,
    pub triggers:    Vec<AutomationTrigger>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AutomationPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name:        Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled:     Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target:      Option<AutomationTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggers:    Option<Vec<AutomationTrigger>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedAutomation {
    pub name:        String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled:     bool,
    pub target:      AutomationTarget,
    #[serde(default)]
    pub triggers:    Vec<AutomationTrigger>,
}

impl AutomationRevision {
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(bytes)))
    }

    /// Wrap a client-supplied revision string (e.g. from an `If-Match`
    /// header). The value is compared bytewise against a stored revision; no
    /// validation is performed here.
    #[must_use]
    pub fn from_raw(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl RepositorySlug {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn owner_repo(&self) -> (&str, &str) {
        self.0
            .split_once('/')
            .expect("repository slugs are validated to contain one slash")
    }
}

impl GitRefSelector {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl WorkflowSlug {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Automation {
    pub fn from_toml_bytes(id: AutomationId, bytes: &[u8]) -> Result<Self, TomlDeError> {
        let source = std::str::from_utf8(bytes).map_err(TomlDeError::custom)?;
        let persisted = toml::from_str::<PersistedAutomation>(source)?;
        let revision = AutomationRevision::from_bytes(bytes);
        Self::assemble(id, revision, persisted.into_replace()).map_err(TomlDeError::custom)
    }

    /// Build, validate, and assign a revision to an `Automation` in one
    /// step. Used by the store immediately after persisting canonical TOML
    /// bytes so the in-memory revision always matches what is on disk.
    pub(crate) fn assemble(
        id: AutomationId,
        revision: AutomationRevision,
        replace: AutomationReplace,
    ) -> Result<Self, AutomationValidationError> {
        validate_common(&replace.name, &replace.triggers)?;
        Ok(Self {
            id,
            revision,
            name: replace.name,
            description: replace.description,
            enabled: replace.enabled,
            target: replace.target,
            triggers: replace.triggers,
        })
    }

    #[must_use]
    pub fn into_replace(self) -> AutomationReplace {
        AutomationReplace {
            name:        self.name,
            description: self.description,
            enabled:     self.enabled,
            target:      self.target,
            triggers:    self.triggers,
        }
    }

    #[must_use]
    pub fn api_trigger(&self) -> Option<&ApiTrigger> {
        self.triggers.iter().find_map(|trigger| match trigger {
            AutomationTrigger::Api(trigger) => Some(trigger),
            AutomationTrigger::Schedule(_) => None,
        })
    }
}

impl AutomationDraft {
    /// Drop the `id` (which becomes the storage filename) and surface the
    /// remaining fields in the canonical replace shape, applying the
    /// `enabled` default.
    #[must_use]
    pub fn into_replace(self) -> AutomationReplace {
        AutomationReplace {
            name:        self.name,
            description: self.description,
            enabled:     self.enabled.unwrap_or(true),
            target:      self.target,
            triggers:    self.triggers,
        }
    }
}

impl AutomationReplace {
    /// Serialize this replace value into canonical TOML bytes. The
    /// representation matches `PersistedAutomation` so on-disk and in-memory
    /// shapes stay aligned without an extra clone.
    pub(crate) fn to_toml_bytes(&self) -> Result<Vec<u8>, TomlEditSerError> {
        to_document(&PersistedAutomationRef::from(self))
            .map(|document| document.to_string().into_bytes())
    }
}

impl AutomationPatch {
    pub(crate) fn apply_to(self, current: &Automation) -> AutomationReplace {
        AutomationReplace {
            name:        self.name.unwrap_or_else(|| current.name.clone()),
            description: self
                .description
                .unwrap_or_else(|| current.description.clone()),
            enabled:     self.enabled.unwrap_or(current.enabled),
            target:      self.target.unwrap_or_else(|| current.target.clone()),
            triggers:    self.triggers.unwrap_or_else(|| current.triggers.clone()),
        }
    }
}

impl PersistedAutomation {
    fn into_replace(self) -> AutomationReplace {
        AutomationReplace {
            name:        self.name,
            description: self.description,
            enabled:     self.enabled,
            target:      self.target,
            triggers:    self.triggers,
        }
    }
}

#[derive(Debug, Serialize)]
struct PersistedAutomationRef<'a> {
    name:        &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    enabled:     bool,
    target:      &'a AutomationTarget,
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    triggers:    &'a [AutomationTrigger],
}

impl<'a> From<&'a AutomationReplace> for PersistedAutomationRef<'a> {
    fn from(value: &'a AutomationReplace) -> Self {
        Self {
            name:        &value.name,
            description: value.description.as_deref(),
            enabled:     value.enabled,
            target:      &value.target,
            triggers:    &value.triggers,
        }
    }
}

impl AutomationTrigger {
    #[must_use]
    pub fn id(&self) -> &AutomationTriggerId {
        match self {
            Self::Api(trigger) => &trigger.id,
            Self::Schedule(trigger) => &trigger.id,
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        match self {
            Self::Api(trigger) => trigger.enabled,
            Self::Schedule(trigger) => trigger.enabled,
        }
    }

    #[must_use]
    pub fn is_api(&self) -> bool {
        matches!(self, Self::Api(_))
    }

    pub fn validate(&self) -> Result<(), AutomationValidationError> {
        match self {
            Self::Api(_) => Ok(()),
            Self::Schedule(trigger) => validate_schedule_expression(&trigger.expression),
        }
    }
}

fn validate_common(
    name: &str,
    triggers: &[AutomationTrigger],
) -> Result<(), AutomationValidationError> {
    if name.trim().is_empty() {
        return Err(AutomationValidationError::EmptyName);
    }

    let mut ids = HashSet::new();
    let mut api_count = 0_usize;
    for trigger in triggers {
        if !ids.insert(trigger.id().clone()) {
            return Err(AutomationValidationError::DuplicateTriggerId(
                trigger.id().to_string(),
            ));
        }
        if trigger.is_api() {
            api_count += 1;
        }
        trigger.validate()?;
    }
    if api_count > 1 {
        return Err(AutomationValidationError::MultipleApiTriggers);
    }

    Ok(())
}

fn validate_schedule_expression(expression: &str) -> Result<(), AutomationValidationError> {
    if expression.trim().is_empty() || expression.split_whitespace().count() != 5 {
        return Err(AutomationValidationError::InvalidScheduleExpression(
            expression.to_string(),
        ));
    }

    CronParser::builder()
        .seconds(Seconds::Disallowed)
        .year(Year::Disallowed)
        .build()
        .parse(expression)
        .map(|_| ())
        .map_err(|_| AutomationValidationError::InvalidScheduleExpression(expression.to_string()))
}

impl TryFrom<String> for RepositorySlug {
    type Error = AutomationValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let Some((owner, repo)) = value.split_once('/') else {
            return Err(AutomationValidationError::InvalidRepositorySlug(value));
        };
        if repo.contains('/')
            || !valid_github_slug_segment(owner, 39)
            || !valid_github_slug_segment(repo, 100)
        {
            return Err(AutomationValidationError::InvalidRepositorySlug(value));
        }
        Ok(Self(value))
    }
}

impl TryFrom<String> for GitRefSelector {
    type Error = AutomationValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if valid_git_ref_selector(&value) {
            Ok(Self(value))
        } else {
            Err(AutomationValidationError::InvalidGitRefSelector(value))
        }
    }
}

impl TryFrom<String> for WorkflowSlug {
    type Error = AutomationValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if valid_workflow_selector(&value) {
            Ok(Self(value))
        } else {
            Err(AutomationValidationError::InvalidWorkflowSelector(value))
        }
    }
}

macro_rules! impl_string_newtype {
    ($type:ty) => {
        impl AsRef<str> for $type {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $type {
            type Err = AutomationValidationError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::try_from(value.to_string())
            }
        }

        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::try_from(value).map_err(D::Error::custom)
            }
        }
    };
}

impl_string_newtype!(RepositorySlug);
impl_string_newtype!(GitRefSelector);
impl_string_newtype!(WorkflowSlug);

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

impl Serialize for AutomationRevision {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AutomationRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self(String::deserialize(deserializer)?))
    }
}

fn valid_github_slug_segment(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn valid_git_ref_selector(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.starts_with('-')
        && !value.contains("..")
        && !value.contains("@{")
        && !has_lock_suffix(value)
        && !value.ends_with('/')
        && !value.starts_with('/')
        && !value.bytes().any(|b| {
            b.is_ascii_control()
                || b.is_ascii_whitespace()
                || matches!(
                    b,
                    b'\\'
                        | b'^'
                        | b'~'
                        | b':'
                        | b'?'
                        | b'*'
                        | b'['
                        | b';'
                        | b'&'
                        | b'|'
                        | b'$'
                        | b'`'
                        | b'\''
                        | b'"'
                        | b'<'
                        | b'>'
                )
        })
}

fn has_lock_suffix(value: &str) -> bool {
    value.rsplit('/').any(|component| {
        component
            .get(component.len().saturating_sub(".lock".len())..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".lock"))
    })
}

fn valid_workflow_selector(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty()
        || value == "."
        || value.contains('\\')
        || value.bytes().any(|b| b.is_ascii_control())
    {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && path.components().all(|component| {
            matches!(component, Component::Normal(_) | Component::CurDir)
                && !matches!(component, Component::ParentDir)
        })
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{
        Automation, AutomationDraft, AutomationReplace, AutomationRevision, AutomationTrigger,
        GitRefSelector, RepositorySlug, WorkflowSlug,
    };
    use crate::AutomationId;

    fn valid_toml() -> &'static str {
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
"#
    }

    fn valid_draft_toml(id: &str, triggers: &str) -> String {
        format!(
            r#"
id = "{id}"
name = "Nightly"

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "deps"

{triggers}
"#
        )
    }

    #[test]
    fn valid_toml_deserializes_and_computes_revision() {
        let id = AutomationId::try_from("nightly-deps".to_string())
            .expect("automation id should be valid");
        let automation = Automation::from_toml_bytes(id, valid_toml().as_bytes())
            .expect("automation TOML should parse");

        assert_eq!(automation.name, "Nightly dependency update");
        assert!(automation.enabled);
        assert_eq!(automation.triggers.len(), 2);
        assert_eq!(
            automation.revision,
            AutomationRevision::from_bytes(valid_toml().as_bytes())
        );
    }

    #[test]
    fn toml_defaults_enabled_and_description() {
        let source = r#"
name = "Defaulted"

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "dependency-update"

[[triggers]]
id = "api"
type = "api"
"#;
        let id =
            AutomationId::try_from("defaulted".to_string()).expect("automation id should be valid");
        let automation = Automation::from_toml_bytes(id, source.as_bytes())
            .expect("automation TOML should parse");

        assert!(automation.enabled);
        assert_eq!(automation.description, None);
        assert!(automation.triggers[0].enabled());
    }

    #[test]
    fn invalid_automation_ids_are_rejected() {
        for value in ["", "-bad", "Bad", "bad_", &"a".repeat(64)] {
            assert!(AutomationId::try_from(value.to_string()).is_err());
        }
    }

    #[test]
    fn invalid_trigger_ids_are_rejected() {
        let result: Result<AutomationDraft, _> = toml::from_str(&valid_draft_toml(
            "nightly",
            r#"
[[triggers]]
id = "_api"
type = "api"
"#,
        ));
        assert!(result.is_err());
    }

    #[test]
    fn duplicate_trigger_ids_are_rejected() {
        let draft: AutomationDraft = toml::from_str(&valid_draft_toml(
            "nightly",
            r#"
[[triggers]]
id = "api"
type = "api"

[[triggers]]
id = "api"
type = "schedule"
expression = "0 3 * * *"
"#,
        ))
        .expect("draft should deserialize");
        assert!(
            Automation::assemble(
                draft.id.clone(),
                AutomationRevision::from_bytes(b""),
                draft.into_replace(),
            )
            .is_err()
        );
    }

    #[test]
    fn two_api_triggers_are_rejected() {
        let draft: AutomationDraft = toml::from_str(&valid_draft_toml(
            "nightly",
            r#"
[[triggers]]
id = "api"
type = "api"

[[triggers]]
id = "api2"
type = "api"
"#,
        ))
        .expect("draft should deserialize");
        assert!(
            Automation::assemble(
                draft.id.clone(),
                AutomationRevision::from_bytes(b""),
                draft.into_replace(),
            )
            .is_err()
        );
    }

    #[test]
    fn invalid_repository_slug_is_rejected() {
        for value in [
            "fabro-sh",
            "fabro-sh/fabro/extra",
            "../fabro",
            "owner/repo/name",
        ] {
            assert!(RepositorySlug::try_from(value.to_string()).is_err());
        }
    }

    #[test]
    fn invalid_schedule_expression_is_rejected() {
        let draft: AutomationDraft = toml::from_str(&valid_draft_toml(
            "nightly",
            r#"
[[triggers]]
id = "nightly"
type = "schedule"
expression = "* * * * * *"
"#,
        ))
        .expect("draft should deserialize");
        assert!(
            Automation::assemble(
                draft.id.clone(),
                AutomationRevision::from_bytes(b""),
                draft.into_replace(),
            )
            .is_err()
        );
    }

    #[test]
    fn newtypes_have_toml_string_shape() {
        let replace: AutomationReplace = toml::from_str(
            r#"
name = "Nightly"
enabled = true

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "deps"

[[triggers]]
id = "api"
type = "api"
enabled = true
"#,
        )
        .expect("replace should deserialize");
        let target_toml = toml::to_string(&replace.target).expect("target should serialize");
        assert!(target_toml.contains("repository = \"fabro-sh/fabro\""));
        assert!(target_toml.contains("ref = \"main\""));
        assert!(target_toml.contains("workflow = \"deps\""));
    }

    #[test]
    fn invalid_ref_and_workflow_selectors_are_rejected() {
        assert!(GitRefSelector::try_from("-main".to_string()).is_err());
        assert!(GitRefSelector::try_from("feature..main".to_string()).is_err());
        assert!(WorkflowSlug::try_from("/tmp/workflow".to_string()).is_err());
        assert!(WorkflowSlug::try_from("../workflow".to_string()).is_err());
    }

    #[test]
    fn trigger_variant_type_is_api() {
        let trigger: AutomationTrigger = toml::from_str(
            r#"
id = "api"
type = "api"
enabled = true
"#,
        )
        .expect("api trigger should deserialize");
        assert!(matches!(trigger, AutomationTrigger::Api(_)));
    }
}
