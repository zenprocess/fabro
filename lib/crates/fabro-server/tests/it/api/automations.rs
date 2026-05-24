use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use crate::helpers::{api, checked_response, response_json};

fn automation_request() -> serde_json::Value {
    serde_json::json!({
        "id": "nightly-deps",
        "name": "Nightly dependency update",
        "description": "Open a PR for dependency updates.",
        "target": {
            "repository": "fabro-sh/fabro",
            "ref": "main",
            "workflow": "dependency-update"
        },
        "triggers": [
            { "id": "api", "type": "api", "enabled": true },
            { "id": "nightly", "type": "schedule", "enabled": true, "expression": "0 3 * * *" }
        ]
    })
}

fn replace_request(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
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
    })
}

fn test_app() -> (axum::Router, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let active_config_path = temp.path().join("settings.toml");
    let state = fabro_server::test_support::TestAppStateBuilder::new()
        .active_config_path(active_config_path)
        .build();
    (fabro_server::test_support::build_test_router(state), temp)
}

async fn json_request(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: serde_json::Value,
    if_match: Option<&str>,
    expected: StatusCode,
) -> serde_json::Value {
    let mut builder = Request::builder()
        .method(method)
        .uri(api(path))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(revision) = if_match {
        builder = builder.header(header::IF_MATCH, revision);
    }
    let request = builder
        .body(Body::from(body.to_string()))
        .expect("request should build");
    response_json(
        app.clone().oneshot(request).await.unwrap(),
        expected,
        format!("{method} /api/v1{path}"),
    )
    .await
}

async fn empty_request(
    app: &axum::Router,
    method: &str,
    path: &str,
    if_match: Option<&str>,
    expected: StatusCode,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(api(path));
    if let Some(revision) = if_match {
        builder = builder.header(header::IF_MATCH, revision);
    }
    let request = builder.body(Body::empty()).expect("request should build");
    checked_response(
        app.clone().oneshot(request).await.unwrap(),
        expected,
        format!("{method} /api/v1{path}"),
    )
    .await
}

#[tokio::test]
async fn automations_crud_lifecycle_persists_files_and_etags() {
    let (app, temp) = test_app();

    let list = empty_request(&app, "GET", "/automations", None, StatusCode::OK).await;
    let list = crate::helpers::body_json(list.into_body()).await;
    assert_eq!(list, serde_json::json!({ "data": [], "meta": { "total": 0 } }));

    let created = json_request(
        &app,
        "POST",
        "/automations",
        automation_request(),
        None,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(created["id"], "nightly-deps");
    assert_eq!(created["enabled"], true);
    assert_eq!(created["triggers"][0]["type"], "api");
    assert!(temp.path().join("automations/nightly-deps.toml").is_file());

    let duplicate = json_request(
        &app,
        "POST",
        "/automations",
        automation_request(),
        None,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(duplicate["errors"][0]["status"], "409");

    let get_response = empty_request(&app, "GET", "/automations/nightly-deps", None, StatusCode::OK).await;
    let etag = get_response
        .headers()
        .get(header::ETAG)
        .expect("ETag header")
        .to_str()
        .expect("ETag should be valid")
        .to_string();
    let fetched = crate::helpers::body_json(get_response.into_body()).await;
    assert_eq!(fetched["revision"], created["revision"]);

    let replaced = json_request(
        &app,
        "PUT",
        "/automations/nightly-deps",
        replace_request("Renamed automation"),
        Some(&etag),
        StatusCode::OK,
    )
    .await;
    assert_eq!(replaced["name"], "Renamed automation");
    assert_ne!(replaced["revision"], created["revision"]);

    let stale = json_request(
        &app,
        "PUT",
        "/automations/nightly-deps",
        replace_request("Stale update"),
        Some(&etag),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(stale["errors"][0]["status"], "409");

    let missing_if_match = json_request(
        &app,
        "PATCH",
        "/automations/nightly-deps",
        serde_json::json!({ "enabled": false }),
        None,
        StatusCode::PRECONDITION_REQUIRED,
    )
    .await;
    assert_eq!(missing_if_match["errors"][0]["status"], "428");

    let current_etag = format!("\"{}\"", replaced["revision"].as_str().unwrap());
    let patched = json_request(
        &app,
        "PATCH",
        "/automations/nightly-deps",
        serde_json::json!({ "description": null }),
        Some(&current_etag),
        StatusCode::OK,
    )
    .await;
    assert_eq!(patched["description"], serde_json::Value::Null);
    assert_eq!(patched["name"], "Renamed automation");

    let delete_etag = format!("\"{}\"", patched["revision"].as_str().unwrap());
    empty_request(
        &app,
        "DELETE",
        "/automations/nightly-deps",
        Some(&delete_etag),
        StatusCode::NO_CONTENT,
    )
    .await;
    assert!(!temp.path().join("automations/nightly-deps.toml").exists());

    empty_request(&app, "GET", "/automations/nightly-deps", None, StatusCode::NOT_FOUND).await;
}

#[tokio::test]
async fn automations_validation_errors_return_422() {
    let (app, _temp) = test_app();

    let cases = [
        (
            "invalid trigger id",
            serde_json::json!({
                "id": "nightly-deps",
                "name": "Nightly dependency update",
                "target": { "repository": "fabro-sh/fabro", "ref": "main", "workflow": "dependency-update" },
                "triggers": [{ "id": "Bad", "type": "api", "enabled": true }]
            }),
        ),
        (
            "duplicate trigger ids",
            serde_json::json!({
                "id": "nightly-deps",
                "name": "Nightly dependency update",
                "target": { "repository": "fabro-sh/fabro", "ref": "main", "workflow": "dependency-update" },
                "triggers": [
                    { "id": "api", "type": "api", "enabled": true },
                    { "id": "api", "type": "schedule", "enabled": true, "expression": "0 3 * * *" }
                ]
            }),
        ),
        (
            "two api triggers",
            serde_json::json!({
                "id": "nightly-deps",
                "name": "Nightly dependency update",
                "target": { "repository": "fabro-sh/fabro", "ref": "main", "workflow": "dependency-update" },
                "triggers": [
                    { "id": "api", "type": "api", "enabled": true },
                    { "id": "api2", "type": "api", "enabled": true }
                ]
            }),
        ),
        (
            "invalid schedule expression",
            serde_json::json!({
                "id": "nightly-deps",
                "name": "Nightly dependency update",
                "target": { "repository": "fabro-sh/fabro", "ref": "main", "workflow": "dependency-update" },
                "triggers": [{ "id": "nightly", "type": "schedule", "enabled": true, "expression": "not a cron" }]
            }),
        ),
        (
            "unknown trigger type",
            serde_json::json!({
                "id": "nightly-deps",
                "name": "Nightly dependency update",
                "target": { "repository": "fabro-sh/fabro", "ref": "main", "workflow": "dependency-update" },
                "triggers": [{ "id": "api", "type": "event", "enabled": true }]
            }),
        ),
    ];

    for (name, body) in cases {
        let response = json_request(
            &app,
            "POST",
            "/automations",
            body,
            None,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
        assert_eq!(response["errors"][0]["status"], "422", "{name}");
    }
}
