use std::any::{TypeId, type_name};

use fabro_api::types::{
    Automation as ApiAutomation, AutomationApiTrigger as ApiAutomationApiTrigger,
    AutomationScheduleTrigger as ApiAutomationScheduleTrigger,
    AutomationTarget as ApiAutomationTarget, AutomationTrigger as ApiAutomationTrigger,
    CreateAutomationRequest as ApiCreateAutomationRequest,
    PatchAutomationRequest as ApiPatchAutomationRequest,
    ReplaceAutomationRequest as ApiReplaceAutomationRequest,
};
use fabro_automation::{
    ApiTrigger, Automation, AutomationDraft, AutomationPatch, AutomationReplace, AutomationTarget,
    AutomationTrigger, ScheduleTrigger,
};
use serde_json::json;

#[test]
fn automation_api_reuses_domain_types() {
    assert_same_type::<ApiAutomation, Automation>();
    assert_same_type::<ApiAutomationTarget, AutomationTarget>();
    assert_same_type::<ApiAutomationTrigger, AutomationTrigger>();
    assert_same_type::<ApiAutomationApiTrigger, ApiTrigger>();
    assert_same_type::<ApiAutomationScheduleTrigger, ScheduleTrigger>();
    assert_same_type::<ApiCreateAutomationRequest, AutomationDraft>();
    assert_same_type::<ApiReplaceAutomationRequest, AutomationReplace>();
    assert_same_type::<ApiPatchAutomationRequest, AutomationPatch>();
}

#[test]
fn automation_response_round_trips_json_shape() {
    let value = json!({
        "id": "nightly-deps",
        "revision": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
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
    });

    let automation: ApiAutomation = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&automation).unwrap(), value);
}

#[test]
fn create_automation_request_round_trips_json_shape() {
    let value = json!({
        "id": "nightly-deps",
        "name": "Nightly dependency update",
        "description": "Open a PR for dependency updates.",
        "enabled": true,
        "target": {
            "repository": "fabro-sh/fabro",
            "ref": "main",
            "workflow": "dependency-update"
        },
        "triggers": [
            { "id": "api", "type": "api", "enabled": true }
        ]
    });

    let request: ApiCreateAutomationRequest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&request).unwrap(), value);
}

#[test]
fn replace_automation_request_round_trips_json_shape() {
    let value = json!({
        "name": "Nightly dependency update",
        "description": "Open a PR for dependency updates.",
        "enabled": false,
        "target": {
            "repository": "fabro-sh/fabro",
            "ref": "main",
            "workflow": "dependency-update"
        },
        "triggers": [
            { "id": "api", "type": "api", "enabled": true }
        ]
    });

    let request: ApiReplaceAutomationRequest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&request).unwrap(), value);
}

#[test]
fn patch_automation_request_preserves_null_description() {
    let value = json!({
        "description": null,
        "enabled": true
    });

    let request: ApiPatchAutomationRequest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&request).unwrap(), value);
}

#[test]
fn create_automation_request_defaults_optional_enabled_fields() {
    let value = json!({
        "id": "nightly-deps",
        "name": "Nightly dependency update",
        "target": {
            "repository": "fabro-sh/fabro",
            "ref": "main",
            "workflow": "dependency-update"
        },
        "triggers": [
            { "id": "api", "type": "api" }
        ]
    });

    let request: ApiCreateAutomationRequest = serde_json::from_value(value).unwrap();
    assert_eq!(request.enabled, None);
    let AutomationTrigger::Api(trigger) = &request.triggers[0] else {
        panic!("expected api trigger");
    };
    assert!(trigger.enabled);
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
