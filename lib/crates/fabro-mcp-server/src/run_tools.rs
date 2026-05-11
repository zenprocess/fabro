#![allow(
    dead_code,
    reason = "The MCP server skeleton defines the full first-slice contract before each tool body is implemented."
)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use fabro_api::types;
use fabro_client::Client;
use fabro_types::{Run, RunId, RunStatus};
use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;
use tokio::task::yield_now;

#[derive(Debug)]
pub(crate) struct ToolError {
    message: String,
}

impl ToolError {
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub(crate) fn from_anyhow(err: &anyhow::Error) -> Self {
        Self::message(format_tool_error(err))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.message
    }
}

pub(crate) type ToolResult<T> = Result<T, ToolError>;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FabroRunCreateParams {
    pub(crate) runs: Vec<CreateRunSpec>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CreateRunSpec {
    pub(crate) workflow:         String,
    pub(crate) cwd:              Option<PathBuf>,
    pub(crate) run_id:           Option<String>,
    pub(crate) goal:             Option<String>,
    #[serde(default)]
    pub(crate) inputs:           HashMap<String, Value>,
    #[serde(default)]
    pub(crate) labels:           HashMap<String, String>,
    pub(crate) dry_run:          Option<bool>,
    pub(crate) auto_approve:     Option<bool>,
    pub(crate) model:            Option<String>,
    pub(crate) provider:         Option<String>,
    pub(crate) sandbox:          Option<String>,
    pub(crate) preserve_sandbox: Option<bool>,
    pub(crate) start:            Option<bool>,
}

#[derive(Debug)]
pub(crate) struct ValidatedCreateRuns {
    pub(crate) runs: Vec<CreateRunSpec>,
}

impl TryFrom<FabroRunCreateParams> for ValidatedCreateRuns {
    type Error = ToolError;

    fn try_from(params: FabroRunCreateParams) -> Result<Self, Self::Error> {
        validate_len("runs", params.runs.len(), 1, 50)?;
        for spec in &params.runs {
            for (key, value) in &spec.inputs {
                json_to_toml_value(key, value)?;
            }
        }
        Ok(Self { runs: params.runs })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct CreateRunsResult {
    pub(crate) runs: Vec<CreatedRunResult>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct CreatedRunResult {
    pub(crate) run_id:   String,
    pub(crate) workflow: String,
    pub(crate) started:  bool,
    pub(crate) status:   String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FabroRunSearchParams {
    pub(crate) run_ids:        Option<Vec<String>>,
    pub(crate) workflow:       Option<String>,
    pub(crate) labels:         Option<HashMap<String, String>>,
    pub(crate) status:         Option<Vec<String>>,
    pub(crate) archived:       Option<bool>,
    pub(crate) created_after:  Option<String>,
    pub(crate) created_before: Option<String>,
    pub(crate) first:          Option<usize>,
    pub(crate) after:          Option<String>,
}

#[derive(Debug)]
pub(crate) struct ValidatedSearchRuns {
    pub(crate) raw: FabroRunSearchParams,
}

impl TryFrom<FabroRunSearchParams> for ValidatedSearchRuns {
    type Error = ToolError;

    fn try_from(params: FabroRunSearchParams) -> Result<Self, Self::Error> {
        if params.first.is_some_and(|first| first > 100) {
            return Err(ToolError::message("first must be <= 100"));
        }
        if let Some(created_after) = params.created_after.as_deref() {
            parse_datetime_filter("created_after", created_after)?;
        }
        if let Some(created_before) = params.created_before.as_deref() {
            parse_datetime_filter("created_before", created_before)?;
        }
        Ok(Self { raw: params })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct SearchRunsResult {
    pub(crate) runs:        Vec<RunSummaryResult>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct RunSummaryResult {
    pub(crate) run_id:           String,
    pub(crate) workflow_name:    String,
    pub(crate) workflow_slug:    Option<String>,
    pub(crate) status:           String,
    pub(crate) archived:         bool,
    pub(crate) created_at:       String,
    pub(crate) started_at:       Option<String>,
    pub(crate) completed_at:     Option<String>,
    pub(crate) labels:           HashMap<String, String>,
    pub(crate) source_directory: Option<String>,
    pub(crate) repo_origin_url:  Option<String>,
    pub(crate) goal:             String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunInteractAction {
    Get,
    Start,
    Message,
    Cancel,
    Archive,
    Unarchive,
    GetQuestions,
    Answer,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FabroRunInteractParams {
    pub(crate) action:      RunInteractAction,
    pub(crate) run_id:      String,
    pub(crate) message:     Option<String>,
    pub(crate) interrupt:   Option<bool>,
    pub(crate) question_id: Option<String>,
    pub(crate) answer:      Option<Value>,
}

#[derive(Debug)]
pub(crate) struct ValidatedInteractRun {
    pub(crate) raw: FabroRunInteractParams,
}

impl TryFrom<FabroRunInteractParams> for ValidatedInteractRun {
    type Error = ToolError;

    fn try_from(params: FabroRunInteractParams) -> Result<Self, Self::Error> {
        if params.run_id.trim().is_empty() {
            return Err(ToolError::message("run_id is required"));
        }
        if matches!(params.action, RunInteractAction::Message)
            && params
                .message
                .as_deref()
                .is_none_or(|message| message.trim().is_empty())
        {
            return Err(ToolError::message("message is required for action message"));
        }
        if matches!(params.action, RunInteractAction::Answer) {
            if params.question_id.as_deref().is_none_or(str::is_empty) {
                return Err(ToolError::message(
                    "question_id is required for action answer",
                ));
            }
            if params.answer.is_none() {
                return Err(ToolError::message("answer is required for action answer"));
            }
        }
        Ok(Self { raw: params })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct InteractRunResult {
    pub(crate) run_id: String,
    pub(crate) action: RunInteractAction,
    pub(crate) result: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FabroRunGatherParams {
    pub(crate) run_ids:               Vec<String>,
    pub(crate) timeout_seconds:       Option<u64>,
    pub(crate) poll_interval_seconds: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct ValidatedGatherRuns {
    pub(crate) run_ids:               Vec<String>,
    pub(crate) timeout_seconds:       u64,
    pub(crate) poll_interval_seconds: u64,
}

impl TryFrom<FabroRunGatherParams> for ValidatedGatherRuns {
    type Error = ToolError;

    fn try_from(params: FabroRunGatherParams) -> Result<Self, Self::Error> {
        validate_len("run_ids", params.run_ids.len(), 1, 50)?;
        Ok(Self {
            run_ids:               params.run_ids,
            timeout_seconds:       params.timeout_seconds.unwrap_or(300).min(600),
            poll_interval_seconds: params.poll_interval_seconds.unwrap_or(15).max(5),
        })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct GatherRunsResult {
    pub(crate) runs:            Vec<RunSummaryResult>,
    pub(crate) timed_out:       bool,
    pub(crate) elapsed_seconds: u64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunEventsAction {
    List,
    Details,
    Search,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FabroRunEventsParams {
    pub(crate) action:             RunEventsAction,
    pub(crate) run_id:             String,
    pub(crate) event_types:        Option<Vec<String>>,
    pub(crate) categories:         Option<Vec<String>>,
    pub(crate) direction:          Option<String>,
    pub(crate) created_after:      Option<String>,
    pub(crate) created_before:     Option<String>,
    pub(crate) first:              Option<usize>,
    pub(crate) after:              Option<u32>,
    pub(crate) event_ids:          Option<Vec<String>>,
    pub(crate) offset:             Option<usize>,
    pub(crate) limit:              Option<usize>,
    pub(crate) max_content_length: Option<usize>,
    pub(crate) query:              Option<String>,
}

#[derive(Debug)]
pub(crate) struct ValidatedRunEvents {
    pub(crate) raw: FabroRunEventsParams,
}

impl TryFrom<FabroRunEventsParams> for ValidatedRunEvents {
    type Error = ToolError;

    fn try_from(params: FabroRunEventsParams) -> Result<Self, Self::Error> {
        if params.run_id.trim().is_empty() {
            return Err(ToolError::message("run_id is required"));
        }
        let first = params.first.or(params.limit).unwrap_or(50);
        if first > 200 {
            return Err(ToolError::message("first must be <= 200"));
        }
        Ok(Self { raw: params })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct RunEventsResult {
    pub(crate) run_id:      String,
    pub(crate) action:      RunEventsAction,
    pub(crate) events:      Vec<RunEventResult>,
    pub(crate) next_cursor: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct RunEventResult {
    pub(crate) event_id:  String,
    pub(crate) sequence:  u32,
    pub(crate) event:     Value,
    pub(crate) truncated: bool,
}

pub(crate) async fn create_runs(
    client: Arc<Client>,
    base_cwd: &Path,
    params: ValidatedCreateRuns,
) -> ToolResult<CreateRunsResult> {
    let mut created = Vec::with_capacity(params.runs.len());
    for spec in params.runs {
        let cwd = spec.cwd.clone().unwrap_or_else(|| base_cwd.to_path_buf());
        let manifest = build_run_manifest(&spec, &cwd).await?;
        let run_id = client
            .create_run_from_manifest(manifest)
            .await
            .map_err(|err| ToolError::from_anyhow(&err))?;
        let started = spec.start.unwrap_or(true);
        if started {
            client
                .start_run(&run_id, false)
                .await
                .map_err(|err| ToolError::from_anyhow(&err))?;
        }
        let summary = client
            .retrieve_run(&run_id)
            .await
            .map_err(|err| ToolError::from_anyhow(&err))?;
        created.push(CreatedRunResult {
            run_id: summary.id.to_string(),
            workflow: spec.workflow,
            started,
            status: run_status_kind(summary.lifecycle.status).to_string(),
        });
    }
    Ok(CreateRunsResult { runs: created })
}

pub(crate) async fn search_runs(
    client: Arc<Client>,
    params: ValidatedSearchRuns,
) -> ToolResult<SearchRunsResult> {
    let raw = params.raw;
    let mut runs = client
        .list_store_runs()
        .await
        .map_err(|err| ToolError::from_anyhow(&err))?;
    runs.sort_by(|a, b| {
        b.timestamps
            .created_at
            .cmp(&a.timestamps.created_at)
            .then_with(|| b.id.to_string().cmp(&a.id.to_string()))
    });

    if let Some(after) = raw.after.as_deref() {
        if let Some(position) = runs.iter().position(|run| run.id.to_string() == after) {
            runs = runs.into_iter().skip(position + 1).collect();
        }
    }

    if let Some(run_ids) = raw.run_ids.as_ref() {
        runs.retain(|run| run_ids.iter().any(|id| id == &run.id.to_string()));
    }
    if let Some(workflow) = raw.workflow.as_deref() {
        runs.retain(|run| {
            run.workflow.name == workflow || run.workflow.slug.as_deref() == Some(workflow)
        });
    }
    if let Some(labels) = raw.labels.as_ref() {
        runs.retain(|run| {
            labels
                .iter()
                .all(|(key, value)| run.labels.get(key) == Some(value))
        });
    }
    if let Some(status) = raw.status.as_ref() {
        runs.retain(|run| {
            status
                .iter()
                .any(|status| status == run_status_kind(run.lifecycle.status))
        });
    }
    if let Some(archived) = raw.archived {
        runs.retain(|run| run.lifecycle.archived == archived);
    }
    if let Some(created_after) = raw.created_after.as_deref() {
        let cutoff = parse_datetime_filter("created_after", created_after)?;
        runs.retain(|run| run.timestamps.created_at >= cutoff);
    }
    if let Some(created_before) = raw.created_before.as_deref() {
        let cutoff = parse_datetime_filter("created_before", created_before)?;
        runs.retain(|run| run.timestamps.created_at <= cutoff);
    }

    let first = raw.first.unwrap_or(20).min(100);
    let has_more = runs.len() > first;
    let page = runs.into_iter().take(first).collect::<Vec<_>>();
    let next_cursor = has_more
        .then(|| page.last().map(|run| run.id.to_string()))
        .flatten();
    Ok(SearchRunsResult {
        runs: page.iter().map(run_summary_result).collect(),
        next_cursor,
    })
}

pub(crate) async fn interact_run(
    _client: Arc<Client>,
    _params: ValidatedInteractRun,
) -> ToolResult<InteractRunResult> {
    yield_now().await;
    Err(ToolError::message(
        "fabro_run_interact is not implemented yet",
    ))
}

pub(crate) async fn gather_runs(
    _client: Arc<Client>,
    _params: ValidatedGatherRuns,
) -> ToolResult<GatherRunsResult> {
    yield_now().await;
    Err(ToolError::message(
        "fabro_run_gather is not implemented yet",
    ))
}

pub(crate) async fn run_events(
    _client: Arc<Client>,
    _params: ValidatedRunEvents,
) -> ToolResult<RunEventsResult> {
    yield_now().await;
    Err(ToolError::message(
        "fabro_run_events is not implemented yet",
    ))
}

pub(crate) fn success_result<T: Serialize>(
    value: &T,
    text: impl Into<String>,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let structured_content = serde_json::to_value(value).map_err(|err| {
        rmcp::ErrorData::internal_error(
            format!("failed to serialize Fabro MCP tool result: {err}"),
            None,
        )
    })?;
    let mut result = CallToolResult::structured(structured_content);
    result.content = vec![Content::text(text.into())];
    Ok(result)
}

pub(crate) fn error_result(err: ToolError) -> CallToolResult {
    CallToolResult::error(vec![Content::text(err.message)])
}

pub(crate) fn create_runs_text(result: &CreateRunsResult) -> String {
    let started = result.runs.iter().filter(|run| run.started).count();
    format!(
        "created {} Fabro run(s), started {started}",
        result.runs.len()
    )
}

pub(crate) fn search_runs_text(result: &SearchRunsResult) -> String {
    format!("found {} Fabro run(s)", result.runs.len())
}

pub(crate) fn interact_run_text(result: &InteractRunResult) -> String {
    format!(
        "completed {:?} for Fabro run {}",
        result.action, result.run_id
    )
}

pub(crate) fn gather_runs_text(result: &GatherRunsResult) -> String {
    format!(
        "gathered {} Fabro run(s), timed_out={}",
        result.runs.len(),
        result.timed_out
    )
}

pub(crate) fn run_events_text(result: &RunEventsResult) -> String {
    format!("returned {} Fabro event(s)", result.events.len())
}

fn validate_len(name: &str, len: usize, min: usize, max: usize) -> ToolResult<()> {
    if len < min {
        return Err(ToolError::message(format!(
            "{name} must contain at least {min} item(s)"
        )));
    }
    if len > max {
        return Err(ToolError::message(format!(
            "{name} must contain no more than {max} item(s)"
        )));
    }
    Ok(())
}

fn format_tool_error(err: &anyhow::Error) -> String {
    format!("{err:#}")
}

async fn build_run_manifest(spec: &CreateRunSpec, cwd: &Path) -> ToolResult<types::RunManifest> {
    if let Some(run_id) = spec.run_id.as_deref() {
        run_id.parse::<RunId>().map_err(|err| {
            ToolError::message(format!("run_id must be a valid Fabro run id: {err}"))
        })?;
    }
    let workflow_path = resolve_workflow_path(&spec.workflow, cwd);
    let manifest_cwd = manifest_cwd_for_workflow(cwd, &workflow_path);
    let workflow_key = workflow_path
        .strip_prefix(&manifest_cwd)
        .unwrap_or(&workflow_path)
        .display()
        .to_string();
    let source = fs::read_to_string(&workflow_path).await.map_err(|err| {
        ToolError::message(format!(
            "failed to read workflow {}: {err}",
            workflow_path.display()
        ))
    })?;
    let workflows = HashMap::from([(workflow_key.clone(), types::ManifestWorkflow {
        config: None,
        files: HashMap::new(),
        source,
    })]);
    Ok(types::RunManifest {
        args: mcp_manifest_args(spec),
        configs: Vec::new(),
        cwd: manifest_cwd.display().to_string(),
        git: None,
        goal: Some(types::ManifestGoal {
            path:  None,
            text:  spec
                .goal
                .clone()
                .unwrap_or_else(|| "Run the Fabro workflow.".to_string()),
            type_: types::ManifestGoalType::Value,
        }),
        run_id: spec.run_id.clone(),
        target: types::ManifestTarget {
            identifier: spec.workflow.clone(),
            path:       workflow_key,
        },
        title: None,
        version: 1,
        workflows,
    })
}

fn resolve_workflow_path(workflow: &str, cwd: &Path) -> PathBuf {
    let path = PathBuf::from(workflow);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn manifest_cwd_for_workflow(cwd: &Path, workflow_path: &Path) -> PathBuf {
    if workflow_path.strip_prefix(cwd).is_ok() {
        cwd.to_path_buf()
    } else {
        workflow_path
            .parent()
            .map_or_else(|| cwd.to_path_buf(), Path::to_path_buf)
    }
}

fn mcp_manifest_args(spec: &CreateRunSpec) -> Option<types::ManifestArgs> {
    let label = spec
        .labels
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    let input = spec
        .inputs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    let payload = types::ManifestArgs {
        auto_approve: spec.auto_approve.filter(|value| *value),
        docker_image: None,
        dry_run: spec.dry_run.filter(|value| *value),
        input,
        label,
        model: spec.model.clone(),
        preserve_sandbox: spec.preserve_sandbox.filter(|value| *value),
        provider: spec.provider.clone(),
        sandbox: spec.sandbox.clone(),
        verbose: None,
    };
    (!mcp_manifest_args_is_empty(&payload)).then_some(payload)
}

fn mcp_manifest_args_is_empty(args: &types::ManifestArgs) -> bool {
    args.auto_approve.is_none()
        && args.docker_image.is_none()
        && args.dry_run.is_none()
        && args.input.is_empty()
        && args.label.is_empty()
        && args.model.is_none()
        && args.preserve_sandbox.is_none()
        && args.provider.is_none()
        && args.sandbox.is_none()
        && args.verbose.is_none()
}

fn json_to_toml_value(key: &str, value: &Value) -> ToolResult<toml::Value> {
    match value {
        Value::Null => Err(ToolError::message(format!(
            "input `{key}` cannot be null; use a string, boolean, number, array, or object"
        ))),
        Value::Bool(value) => Ok(toml::Value::Boolean(*value)),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Ok(toml::Value::Integer(integer))
            } else if let Some(float) = value.as_f64() {
                Ok(toml::Value::Float(float))
            } else {
                Err(ToolError::message(format!(
                    "input `{key}` contains a number outside TOML's supported range"
                )))
            }
        }
        Value::String(value) => Ok(toml::Value::String(value.clone())),
        Value::Array(values) => values
            .iter()
            .map(|value| json_to_toml_value(key, value))
            .collect::<ToolResult<Vec<_>>>()
            .map(toml::Value::Array),
        Value::Object(values) => {
            let mut table = toml::Table::new();
            for (child_key, child_value) in values {
                table.insert(child_key.clone(), json_to_toml_value(key, child_value)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

fn run_summary_result(run: &Run) -> RunSummaryResult {
    RunSummaryResult {
        run_id:           run.id.to_string(),
        workflow_name:    run.workflow.name.clone(),
        workflow_slug:    run.workflow.slug.clone(),
        status:           run_status_kind(run.lifecycle.status).to_string(),
        archived:         run.lifecycle.archived,
        created_at:       run.timestamps.created_at.to_rfc3339(),
        started_at:       run
            .timestamps
            .started_at
            .map(|timestamp| timestamp.to_rfc3339()),
        completed_at:     run
            .timestamps
            .completed_at
            .map(|timestamp| timestamp.to_rfc3339()),
        labels:           run.labels.clone(),
        source_directory: run.source_directory.clone(),
        repo_origin_url:  run
            .repository
            .as_ref()
            .and_then(|repository| repository.origin_url.clone()),
        goal:             run.goal.clone(),
    }
}

fn parse_datetime_filter(name: &str, raw: &str) -> ToolResult<DateTime<Utc>> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(raw) {
        return Ok(timestamp.with_timezone(&Utc));
    }
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|err| {
        ToolError::message(format!("{name} must be RFC3339 or YYYY-MM-DD: {err}"))
    })?;
    let datetime = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| ToolError::message(format!("{name} contains an invalid date")))?;
    Ok(DateTime::from_naive_utc_and_offset(datetime, Utc))
}

fn run_status_kind(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Submitted => "submitted",
        RunStatus::Queued => "queued",
        RunStatus::Starting => "starting",
        RunStatus::Running => "running",
        RunStatus::Blocked { .. } => "blocked",
        RunStatus::Paused { .. } => "paused",
        RunStatus::Removing => "removing",
        RunStatus::Succeeded { .. } => "succeeded",
        RunStatus::Failed { .. } => "failed",
        RunStatus::Dead => "dead",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn json_inputs_convert_to_toml_values() {
        let cases = [
            (json!("hello"), toml::Value::String("hello".to_string())),
            (json!(true), toml::Value::Boolean(true)),
            (json!(42), toml::Value::Integer(42)),
            (json!(0.5), toml::Value::Float(0.5)),
            (
                json!(["a", 1]),
                toml::Value::Array(vec![
                    toml::Value::String("a".to_string()),
                    toml::Value::Integer(1),
                ]),
            ),
            (
                json!({ "enabled": true, "count": 2 }),
                toml::Value::Table(toml::Table::from_iter([
                    ("enabled".to_string(), toml::Value::Boolean(true)),
                    ("count".to_string(), toml::Value::Integer(2)),
                ])),
            ),
        ];

        for (json, expected) in cases {
            assert_eq!(json_to_toml_value("input", &json).unwrap(), expected);
        }
    }

    #[test]
    fn json_input_null_is_rejected_with_key_name() {
        let err = json_to_toml_value("goal", &Value::Null).unwrap_err();

        assert!(err.as_str().contains("goal"));
        assert!(err.as_str().contains("null"));
    }
}
