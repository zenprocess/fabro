use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use fabro_automation::{
    AutomationDraft, AutomationId, AutomationPatch, AutomationReplace, AutomationRevision,
    AutomationStoreError,
};

use super::super::AppState;
use crate::error::ApiError;
use crate::principal_middleware::RequiredUser;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/automations", get(list_automations).post(create_automation))
        .route(
            "/automations/{id}",
            get(get_automation)
                .put(replace_automation)
                .patch(patch_automation)
                .delete(delete_automation),
        )
}

async fn list_automations(_auth: RequiredUser, State(state): State<Arc<AppState>>) -> Response {
    let mut automations = state.automation_store().list().await;
    automations.sort_by(|left, right| left.id.cmp(&right.id));
    let total = automations.len();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "data": automations,
            "meta": { "total": total }
        })),
    )
        .into_response()
}

async fn create_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> Response {
    let draft = match parse_domain_json::<AutomationDraft>(&body) {
        Ok(draft) => draft,
        Err(err) => return err.into_response(),
    };

    match state.automation_store().create(draft).await {
        Ok(automation) => (StatusCode::CREATED, Json(automation)).into_response(),
        Err(err) => automation_store_error(err).into_response(),
    }
}

async fn get_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let id = match parse_automation_id(id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };

    match state.automation_store().get(&id).await {
        Some(automation) => automation_response(StatusCode::OK, automation),
        None => ApiError::not_found("Automation not found.").into_response(),
    }
}

async fn replace_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let id = match parse_automation_id(id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    let expected = match parse_if_match(&headers) {
        Ok(revision) => revision,
        Err(err) => return err.into_response(),
    };
    let draft = match parse_domain_json::<AutomationReplace>(&body) {
        Ok(draft) => draft,
        Err(err) => return err.into_response(),
    };

    match state.automation_store().replace(&id, &expected, draft).await {
        Ok(automation) => automation_response(StatusCode::OK, automation),
        Err(err) => automation_store_error(err).into_response(),
    }
}

async fn patch_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let id = match parse_automation_id(id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    let expected = match parse_if_match(&headers) {
        Ok(revision) => revision,
        Err(err) => return err.into_response(),
    };
    let patch = match parse_domain_json::<AutomationPatch>(&body) {
        Ok(patch) => patch,
        Err(err) => return err.into_response(),
    };

    match state.automation_store().patch(&id, &expected, patch).await {
        Ok(automation) => automation_response(StatusCode::OK, automation),
        Err(err) => automation_store_error(err).into_response(),
    }
}

async fn delete_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let id = match parse_automation_id(id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    let expected = match parse_if_match(&headers) {
        Ok(revision) => revision,
        Err(err) => return err.into_response(),
    };

    match state.automation_store().delete(&id, &expected).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => automation_store_error(err).into_response(),
    }
}

fn automation_response(status: StatusCode, automation: fabro_automation::Automation) -> Response {
    let etag = format!("\"{}\"", automation.revision.as_str());
    let etag = HeaderValue::from_str(&etag).expect("automation revisions are valid header values");
    (status, [(header::ETAG, etag)], Json(automation)).into_response()
}

fn parse_automation_id(id: String) -> Result<AutomationId, ApiError> {
    AutomationId::try_from(id).map_err(|err| ApiError::bad_request(err.to_string()))
}

fn parse_if_match(headers: &HeaderMap) -> Result<AutomationRevision, ApiError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Err(ApiError::new(
            StatusCode::PRECONDITION_REQUIRED,
            "Missing If-Match revision.",
        ));
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::bad_request("Invalid If-Match revision."))?
        .trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
        .trim();
    if unquoted.is_empty() {
        return Err(ApiError::new(
            StatusCode::PRECONDITION_REQUIRED,
            "Missing If-Match revision.",
        ));
    }
    Ok(AutomationRevision::new(unquoted))
}

fn parse_domain_json<T>(body: &[u8]) -> Result<T, ApiError>
where
    T: serde::de::DeserializeOwned,
{
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|err| ApiError::bad_request(err.to_string()))?;
    serde_json::from_value(value).map_err(|err| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Invalid automation definition: {err}"),
        )
    })
}

fn automation_store_error(err: AutomationStoreError) -> ApiError {
    match err {
        AutomationStoreError::NotFound(_) => ApiError::not_found("Automation not found."),
        AutomationStoreError::AlreadyExists(_) => {
            ApiError::new(StatusCode::CONFLICT, "Automation already exists.")
        }
        AutomationStoreError::MissingRevision => ApiError::new(
            StatusCode::PRECONDITION_REQUIRED,
            "Missing If-Match revision.",
        ),
        AutomationStoreError::RevisionMismatch => {
            ApiError::new(StatusCode::CONFLICT, "Automation revision mismatch.")
        }
        AutomationStoreError::Validation(err) => {
            ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, err.to_string())
        }
        AutomationStoreError::Parse { .. }
        | AutomationStoreError::InvalidFilename { .. }
        | AutomationStoreError::Io { .. }
        | AutomationStoreError::Serialize(_) => {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Automation store error.")
        }
    }
}
