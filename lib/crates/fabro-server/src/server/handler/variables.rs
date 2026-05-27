use std::sync::Arc;

use fabro_types::Variable;

use super::super::{
    ApiError, AppState, CreateVariableRequest, IntoResponse, Json, Path, RequiredUser, Response,
    Router, State, StatusCode, UpdateVariableRequest, VariableError, VariableListResponse,
    VariableStore, get,
};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/variables", get(list_variables).post(create_variable))
        .route(
            "/variables/{name}",
            get(get_variable)
                .put(update_variable)
                .delete(delete_variable),
        )
}

async fn list_variables(_auth: RequiredUser, State(state): State<Arc<AppState>>) -> Response {
    let data = state.variables.list().await;
    (StatusCode::OK, Json(VariableListResponse { data })).into_response()
}

async fn create_variable(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateVariableRequest>,
) -> Response {
    let name = body.name;
    let value = body.value;
    let description = body.description;
    let result = state
        .variables
        .set(&name, &value, description.as_deref())
        .await;
    variable_write_response(result)
}

async fn get_variable(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    if let Err(VariableError::InvalidName(_)) = VariableStore::validate_name(&name) {
        return ApiError::bad_request("invalid variable name").into_response();
    }
    match state.variables.get(&name).await {
        Some(variable) => (StatusCode::OK, Json(variable)).into_response(),
        None => ApiError::not_found(format!("variable not found: {name}")).into_response(),
    }
}

async fn update_variable(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<UpdateVariableRequest>,
) -> Response {
    let value = body.value;
    let description = body.description;
    let result = state
        .variables
        .update_existing(&name, &value, description.as_deref())
        .await;
    variable_write_response(result)
}

async fn delete_variable(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    let result = state.variables.remove(&name).await;

    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => variable_error_response(err),
    }
}

fn variable_write_response(result: Result<Variable, VariableError>) -> Response {
    match result {
        Ok(variable) => (StatusCode::OK, Json(variable)).into_response(),
        Err(err) => variable_error_response(err),
    }
}

fn variable_error_response(err: VariableError) -> Response {
    match err {
        VariableError::InvalidName(_) => {
            ApiError::bad_request("invalid variable name").into_response()
        }
        VariableError::NotFound(name) => {
            ApiError::not_found(format!("variable not found: {name}")).into_response()
        }
        VariableError::Io(err) => {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
        VariableError::Serde(err) => {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    }
}
