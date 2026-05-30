use std::any::{TypeId, type_name};

use fabro_api::types::{
    WorkerBootstrapGithubIntegration as ApiWorkerBootstrapGithubIntegration,
    WorkerBootstrapResponse as ApiWorkerBootstrapResponse,
    WorkerBootstrapSecret as ApiWorkerBootstrapSecret,
};
use fabro_types::settings::server::GithubIntegrationStrategy;
use fabro_types::{
    SecretType, WorkerBootstrapGithubIntegration, WorkerBootstrapResponse, WorkerBootstrapSecret,
};

#[test]
fn worker_bootstrap_family_reuses_domain_types() {
    assert_same_type::<ApiWorkerBootstrapResponse, WorkerBootstrapResponse>();
    assert_same_type::<ApiWorkerBootstrapSecret, WorkerBootstrapSecret>();
    assert_same_type::<ApiWorkerBootstrapGithubIntegration, WorkerBootstrapGithubIntegration>();
}

#[test]
fn worker_bootstrap_json_matches_openapi_shape() {
    let payload = WorkerBootstrapResponse {
        config_toml: "_version = 1\n".to_string(),
        secrets:     vec![WorkerBootstrapSecret {
            name:        "OPENAI_API_KEY".to_string(),
            value:       "sk-test".to_string(),
            secret_type: SecretType::Token,
            description: Some("OpenAI".to_string()),
        }],
        github:      WorkerBootstrapGithubIntegration {
            enabled:  true,
            strategy: GithubIntegrationStrategy::App,
            app_id:   Some("12345".to_string()),
            slug:     Some("fabro-dev".to_string()),
        },
    };

    let json = serde_json::to_value(&payload).expect("bootstrap payload should serialize");
    assert_eq!(json["config_toml"], "_version = 1\n");
    assert_eq!(json["secrets"][0]["name"], "OPENAI_API_KEY");
    assert_eq!(json["secrets"][0]["value"], "sk-test");
    assert_eq!(json["secrets"][0]["type"], "token");
    assert_eq!(json["secrets"][0]["description"], "OpenAI");
    assert_eq!(json["github"]["enabled"], true);
    assert_eq!(json["github"]["strategy"], "app");
    assert_eq!(json["github"]["app_id"], "12345");
    assert_eq!(json["github"]["slug"], "fabro-dev");

    let round_trip: ApiWorkerBootstrapResponse =
        serde_json::from_value(json).expect("bootstrap payload should deserialize");
    assert_eq!(round_trip, payload);
}

#[test]
fn worker_bootstrap_debug_redacts_secret_material() {
    let payload = WorkerBootstrapResponse {
        config_toml: "raw-secret-config".to_string(),
        secrets:     vec![WorkerBootstrapSecret {
            name:        "OPENAI_API_KEY".to_string(),
            value:       "sk-test".to_string(),
            secret_type: SecretType::Token,
            description: None,
        }],
        github:      WorkerBootstrapGithubIntegration {
            enabled:  false,
            strategy: GithubIntegrationStrategy::Token,
            app_id:   None,
            slug:     None,
        },
    };

    let secret_debug = format!("{:?}", payload.secrets[0]);
    assert!(secret_debug.contains("[REDACTED]"));
    assert!(!secret_debug.contains("sk-test"));

    let payload_debug = format!("{payload:?}");
    assert!(!payload_debug.contains("raw-secret-config"));
    assert!(!payload_debug.contains("sk-test"));
    assert!(payload_debug.contains("config_toml_bytes"));
    assert!(payload_debug.contains("secret_count"));
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
