use std::any::{TypeId, type_name};
use std::str::FromStr as _;

use fabro_api::types::{
    Automation as ApiAutomation, AutomationTarget as ApiAutomationTarget,
    AutomationTrigger as ApiAutomationTrigger, CreateAutomationRequest, PatchAutomationRequest,
    ReplaceAutomationRequest,
};
use fabro_automation::{
    ApiTrigger, Automation, AutomationDraft, AutomationPatch, AutomationReplace,
    AutomationRevision, AutomationTarget, AutomationTrigger, AutomationTriggerId, GitRefSelector,
    RepositorySlug, ScheduleTrigger, WorkflowSlug,
};
use serde_json::json;

#[test]
fn automation_api_reuses_domain_types() {
    assert_same_type::<ApiAutomation, Automation>();
    assert_same_type::<ApiAutomationTarget, AutomationTarget>();
    assert_same_type::<ApiAutomationTrigger, AutomationTrigger>();
    assert_same_type::<CreateAutomationRequest, AutomationDraft>();
    assert_same_type::<ReplaceAutomationRequest, AutomationReplace>();
    assert_same_type::<PatchAutomationRequest, AutomationPatch>();
}

#[test]
fn automation_json_matches_openapi_shape() {
    let automation = Automation {
        id:          "nightly-deps".parse().unwrap(),
        revision:    AutomationRevision::from_raw("abc123"),
        name:        "Nightly dependency update".to_string(),
        description: Some("Open a PR for dependency updates.".to_string()),
        enabled:     true,
        target:      target(),
        triggers:    vec![
            AutomationTrigger::Api(ApiTrigger {
                id:      "api".parse().unwrap(),
                enabled: false,
            }),
            AutomationTrigger::Schedule(ScheduleTrigger {
                id:         "nightly".parse().unwrap(),
                enabled:    true,
                expression: "0 3 * * *".to_string(),
            }),
        ],
    };

    assert_eq!(
        serde_json::to_value(automation).unwrap(),
        json!({
            "id": "nightly-deps",
            "revision": "abc123",
            "name": "Nightly dependency update",
            "description": "Open a PR for dependency updates.",
            "enabled": true,
            "target": {
                "repository": "fabro-sh/fabro",
                "ref": "main",
                "workflow": "dependency-update"
            },
            "triggers": [
                { "id": "api", "type": "api", "enabled": false },
                { "id": "nightly", "type": "schedule", "enabled": true, "expression": "0 3 * * *" }
            ]
        })
    );
}

#[test]
fn automation_request_json_matches_openapi_shape() {
    let create = AutomationDraft {
        id:          "nightly-deps".parse().unwrap(),
        name:        "Nightly dependency update".to_string(),
        description: None,
        enabled:     None,
        target:      target(),
        triggers:    vec![AutomationTrigger::Api(ApiTrigger {
            id:      "api".parse().unwrap(),
            enabled: true,
        })],
    };
    assert_eq!(
        serde_json::to_value(create).unwrap(),
        json!({
            "id": "nightly-deps",
            "name": "Nightly dependency update",
            "target": {
                "repository": "fabro-sh/fabro",
                "ref": "main",
                "workflow": "dependency-update"
            },
            "triggers": [
                { "id": "api", "type": "api", "enabled": true }
            ]
        })
    );

    let patch = AutomationPatch {
        name:        None,
        description: Some(None),
        enabled:     None,
        target:      None,
        triggers:    None,
    };
    assert_eq!(
        serde_json::to_value(patch).unwrap(),
        json!({ "description": null })
    );
}

fn target() -> AutomationTarget {
    AutomationTarget {
        repository: RepositorySlug::from_str("fabro-sh/fabro").unwrap(),
        ref_:       GitRefSelector::from_str("main").unwrap(),
        workflow:   WorkflowSlug::from_str("dependency-update").unwrap(),
    }
}

#[test]
fn trigger_id_json_shape_is_string() {
    let id = AutomationTriggerId::from_str("api_1").unwrap();
    assert_eq!(serde_json::to_value(id).unwrap(), json!("api_1"));
}

fn assert_same_type<T: 'static, U: 'static>() {
    assert_eq!(
        TypeId::of::<T>(),
        TypeId::of::<U>(),
        "{} should be the same type as {}",
        type_name::<T>(),
        type_name::<U>()
    );
}
