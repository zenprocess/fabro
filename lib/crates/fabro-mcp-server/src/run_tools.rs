#![allow(
    dead_code,
    reason = "The MCP server skeleton defines the full first-slice contract before each tool body is implemented."
)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDate, Utc};
use fabro_api::types;
use fabro_client::Client;
use fabro_config::{CliLayer, RunLayer};
use fabro_manifest::{
    ManifestBuildInput, RunOverrideInput, build_run_manifest as build_canonical_run_manifest,
    build_sparse_run_overrides,
};
use fabro_server::manifest_validation;
use fabro_types::{EventEnvelope, Run, RunId, RunStatus};
use fabro_util::exit::{self, ExitClass};
use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time;

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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
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
            let Some(answer) = params.answer.as_ref() else {
                return Err(ToolError::message("answer is required for action answer"));
            };
            answer_to_submit_request(answer.clone())?;
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
        if params.timeout_seconds.is_some_and(|timeout| timeout > 600) {
            return Err(ToolError::message("timeout_seconds must be <= 600"));
        }
        if params
            .poll_interval_seconds
            .is_some_and(|interval| interval < 5)
        {
            return Err(ToolError::message("poll_interval_seconds must be >= 5"));
        }
        Ok(Self {
            run_ids:               params.run_ids,
            timeout_seconds:       params.timeout_seconds.unwrap_or(300),
            poll_interval_seconds: params.poll_interval_seconds.unwrap_or(15),
        })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct GatherRunsResult {
    pub(crate) runs:            Vec<RunSummaryResult>,
    pub(crate) timed_out:       bool,
    pub(crate) elapsed_seconds: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
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
        if let Some(direction) = params.direction.as_deref() {
            if !matches!(direction, "asc" | "desc") {
                return Err(ToolError::message("direction must be `asc` or `desc`"));
            }
        }
        if let Some(created_after) = params.created_after.as_deref() {
            parse_datetime_filter("created_after", created_after)?;
        }
        if let Some(created_before) = params.created_before.as_deref() {
            parse_datetime_filter("created_before", created_before)?;
        }
        if matches!(params.action, RunEventsAction::Details)
            && params.event_ids.as_ref().is_none_or(Vec::is_empty)
        {
            return Err(ToolError::message(
                "event_ids is required for details action",
            ));
        }
        if matches!(params.action, RunEventsAction::Search)
            && params
                .query
                .as_deref()
                .is_none_or(|query| query.trim().is_empty())
        {
            return Err(ToolError::message("query is required for search action"));
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
    user_settings_path: &Path,
    params: ValidatedCreateRuns,
) -> ToolResult<CreateRunsResult> {
    let mut created = Vec::with_capacity(params.runs.len());
    for spec in params.runs {
        let cwd = spec.cwd.clone().unwrap_or_else(|| base_cwd.to_path_buf());
        let manifest = build_mcp_run_manifest(&spec, &cwd, user_settings_path)?;
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
        let a_sort_time = a.timestamps.started_at.unwrap_or_else(|| a.id.created_at());
        let b_sort_time = b.timestamps.started_at.unwrap_or_else(|| b.id.created_at());
        b_sort_time.cmp(&a_sort_time).then_with(|| b.id.cmp(&a.id))
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
    client: Arc<Client>,
    params: ValidatedInteractRun,
) -> ToolResult<InteractRunResult> {
    let raw = params.raw;
    let run_id = client
        .resolve_run(&raw.run_id)
        .await
        .map_err(|err| ToolError::from_anyhow(&err))?
        .id;
    let result = match raw.action {
        RunInteractAction::Get => interact_get(&client, &run_id).await?,
        RunInteractAction::Start => {
            client
                .start_run(&run_id, false)
                .await
                .map_err(|err| ToolError::from_anyhow(&err))?;
            json!({ "summary": run_summary_result(&retrieve_run(&client, &run_id).await?) })
        }
        RunInteractAction::Message => {
            let message = raw
                .message
                .expect("validated message action has a message")
                .trim()
                .to_string();
            client
                .steer_run(&run_id, message.clone(), raw.interrupt.unwrap_or(false))
                .await
                .map_err(|err| ToolError::from_anyhow(&err))?;
            json!({ "message": message, "interrupt": raw.interrupt.unwrap_or(false) })
        }
        RunInteractAction::Cancel => {
            client
                .cancel_run(&run_id)
                .await
                .map_err(|err| ToolError::from_anyhow(&err))?;
            json!({ "summary": run_summary_result(&retrieve_run(&client, &run_id).await?) })
        }
        RunInteractAction::Archive => {
            client
                .archive_run(&run_id)
                .await
                .map_err(|err| ToolError::from_anyhow(&err))?;
            json!({ "summary": run_summary_result(&retrieve_run(&client, &run_id).await?) })
        }
        RunInteractAction::Unarchive => {
            client
                .unarchive_run(&run_id)
                .await
                .map_err(|err| ToolError::from_anyhow(&err))?;
            json!({ "summary": run_summary_result(&retrieve_run(&client, &run_id).await?) })
        }
        RunInteractAction::GetQuestions => {
            let questions = client
                .list_run_questions(&run_id)
                .await
                .map_err(|err| ToolError::from_anyhow(&err))?;
            json!({ "questions": questions })
        }
        RunInteractAction::Answer => {
            let question_id = raw
                .question_id
                .expect("validated answer action has a question_id");
            let body = answer_to_submit_request(
                raw.answer.expect("validated answer action has an answer"),
            )?;
            client
                .submit_run_answer(&run_id, &question_id, body)
                .await
                .map_err(|err| ToolError::from_anyhow(&err))?;
            json!({ "question_id": question_id, "submitted": true })
        }
    };

    Ok(InteractRunResult {
        run_id: run_id.to_string(),
        action: raw.action,
        result,
    })
}

pub(crate) async fn gather_runs(
    client: Arc<Client>,
    params: ValidatedGatherRuns,
) -> ToolResult<GatherRunsResult> {
    let start = Instant::now();
    let deadline = start + Duration::from_secs(params.timeout_seconds);
    let mut run_ids = Vec::with_capacity(params.run_ids.len());
    for selector in params.run_ids {
        run_ids.push(
            client
                .resolve_run(&selector)
                .await
                .map_err(|err| ToolError::from_anyhow(&err))?
                .id,
        );
    }

    loop {
        let mut summaries = Vec::with_capacity(run_ids.len());
        for run_id in &run_ids {
            summaries.push(retrieve_run(&client, run_id).await?);
        }
        if summaries
            .iter()
            .all(|run| run.lifecycle.status.is_terminal())
        {
            return Ok(GatherRunsResult {
                runs:            summaries.iter().map(run_summary_result).collect(),
                timed_out:       false,
                elapsed_seconds: start.elapsed().as_secs(),
            });
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(GatherRunsResult {
                runs:            summaries.iter().map(run_summary_result).collect(),
                timed_out:       true,
                elapsed_seconds: start.elapsed().as_secs(),
            });
        }
        let sleep_for = Duration::from_secs(params.poll_interval_seconds).min(deadline - now);
        time::sleep(sleep_for).await;
    }
}

pub(crate) async fn run_events(
    client: Arc<Client>,
    params: ValidatedRunEvents,
) -> ToolResult<RunEventsResult> {
    let raw = params.raw;
    let run_id = client
        .resolve_run(&raw.run_id)
        .await
        .map_err(|err| ToolError::from_anyhow(&err))?
        .id;
    let descending = raw.direction.as_deref() == Some("desc");
    let fetch_after = if descending { None } else { raw.after };
    let mut events = client
        .list_run_events(&run_id, fetch_after, event_fetch_limit(&raw))
        .await
        .map_err(|err| ToolError::from_anyhow(&err))?;
    if descending {
        if let Some(after) = raw.after {
            events.retain(|event| event.seq < after);
        }
    }
    filter_events(&mut events, &raw)?;
    if descending {
        events.reverse();
    }
    let offset = raw.offset.unwrap_or(0);
    let first = raw.first.or(raw.limit).unwrap_or(50).min(200);
    let page = events
        .into_iter()
        .skip(offset)
        .take(first)
        .collect::<Vec<_>>();
    let max_content_length = raw.max_content_length.unwrap_or(20_000);
    let results = page
        .iter()
        .map(|event| run_event_result(event, max_content_length))
        .collect::<ToolResult<Vec<_>>>()?;
    let next_cursor = page.last().map(|event| {
        if descending {
            event.seq
        } else {
            event.seq.saturating_add(1)
        }
    });

    Ok(RunEventsResult {
        run_id: run_id.to_string(),
        action: raw.action,
        events: results,
        next_cursor,
    })
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
    let mut rendered = format!("{err:#}");
    if exit::exit_class_for(err) == Some(ExitClass::AuthRequired)
        && !rendered.contains("fabro auth login")
    {
        rendered.push_str("\nRun `fabro auth login` to authenticate.");
    }
    rendered
}

async fn retrieve_run(client: &Client, run_id: &RunId) -> ToolResult<Run> {
    client
        .retrieve_run(run_id)
        .await
        .map_err(|err| ToolError::from_anyhow(&err))
}

async fn interact_get(client: &Client, run_id: &RunId) -> ToolResult<Value> {
    let summary = retrieve_run(client, run_id).await?;
    let projection = client
        .get_run_state(run_id)
        .await
        .map_err(|err| ToolError::from_anyhow(&err))?;
    Ok(json!({
        "summary": run_summary_result(&summary),
        "projection": projection,
    }))
}

fn answer_to_submit_request(answer: Value) -> ToolResult<types::SubmitAnswerRequest> {
    let payload = match answer {
        Value::Bool(true) => json!({ "kind": "yes" }),
        Value::Bool(false) => json!({ "kind": "no" }),
        Value::String(text) => json!({ "kind": "text", "text": text }),
        Value::Object(mut object) => {
            if let Some(option) = object.remove("option") {
                json!({ "kind": "selected", "option_key": option })
            } else if let Some(options) = object.remove("options") {
                json!({ "kind": "multi_selected", "option_keys": options })
            } else if let Some(text) = object.remove("text") {
                json!({ "kind": "text", "text": text })
            } else {
                return Err(ToolError::message(
                    "answer object must contain one of: option, options, text",
                ));
            }
        }
        other => {
            return Err(ToolError::message(format!(
                "unsupported answer value: {other}; expected boolean, string, or object",
            )));
        }
    };
    serde_json::from_value(payload)
        .map_err(|err| ToolError::message(format!("failed to build submit-answer request: {err}")))
}

fn event_fetch_limit(params: &FabroRunEventsParams) -> Option<usize> {
    let needs_full_scan = params.event_ids.is_some()
        || params.event_types.is_some()
        || params.categories.is_some()
        || params.created_after.is_some()
        || params.created_before.is_some()
        || params.direction.as_deref() == Some("desc")
        || matches!(
            params.action,
            RunEventsAction::Details | RunEventsAction::Search
        );
    if needs_full_scan {
        return None;
    }

    let requested = params
        .first
        .or(params.limit)
        .unwrap_or(50)
        .saturating_add(params.offset.unwrap_or(0));
    (requested <= 200).then_some(requested.max(1))
}

fn filter_events(events: &mut Vec<EventEnvelope>, params: &FabroRunEventsParams) -> ToolResult<()> {
    if let Some(event_ids) = params.event_ids.as_ref() {
        events.retain(|event| event_ids.contains(&event.event.id));
    }
    if let Some(event_types) = params.event_types.as_ref() {
        events.retain(|event| {
            event_types
                .iter()
                .any(|event_type| event_type == event.event.event_name())
        });
    }
    if let Some(categories) = params.categories.as_ref() {
        events.retain(|event| {
            let category = event
                .event
                .event_name()
                .split('.')
                .next()
                .unwrap_or_default();
            categories.iter().any(|candidate| candidate == category)
        });
    }
    if let Some(created_after) = params.created_after.as_deref() {
        let cutoff = parse_datetime_filter("created_after", created_after)?;
        events.retain(|event| event.event.ts >= cutoff);
    }
    if let Some(created_before) = params.created_before.as_deref() {
        let cutoff = parse_datetime_filter("created_before", created_before)?;
        events.retain(|event| event.event.ts <= cutoff);
    }
    if matches!(params.action, RunEventsAction::Search) {
        if let Some(query) = params.query.as_deref() {
            events.retain(|event| {
                serde_json::to_string(event).is_ok_and(|serialized| serialized.contains(query))
            });
        }
    }
    Ok(())
}

fn run_event_result(
    event: &EventEnvelope,
    max_content_length: usize,
) -> ToolResult<RunEventResult> {
    let mut serialized = serde_json::to_string(event)
        .map_err(|err| ToolError::message(format!("failed to serialize event: {err}")))?;
    let truncated = serialized.len() > max_content_length;
    let event_value = if truncated {
        serialized.truncate(max_content_length);
        Value::String(serialized)
    } else {
        serde_json::to_value(event)
            .map_err(|err| ToolError::message(format!("failed to serialize event: {err}")))?
    };
    Ok(RunEventResult {
        event_id: event.event.id.clone(),
        sequence: event.seq,
        event: event_value,
        truncated,
    })
}

fn build_mcp_run_manifest(
    spec: &CreateRunSpec,
    cwd: &Path,
    user_settings_path: &Path,
) -> ToolResult<types::RunManifest> {
    if let Some(run_id) = spec.run_id.as_deref() {
        run_id.parse::<RunId>().map_err(|err| {
            ToolError::message(format!("run_id must be a valid Fabro run id: {err}"))
        })?;
    }

    let built = build_canonical_run_manifest(ManifestBuildInput {
        workflow:           PathBuf::from(&spec.workflow),
        cwd:                cwd.to_path_buf(),
        run_overrides:      mcp_run_overrides(spec),
        cli_overrides:      Some(CliLayer::default()),
        input_overrides:    spec
            .inputs
            .iter()
            .map(|(key, value)| json_to_toml_value(key, value).map(|value| (key.clone(), value)))
            .collect::<ToolResult<HashMap<_, _>>>()?,
        args:               mcp_manifest_args(spec),
        run_id:             spec
            .run_id
            .as_deref()
            .map(str::parse::<RunId>)
            .transpose()
            .map_err(|err| {
                ToolError::message(format!("run_id must be a valid Fabro run id: {err}"))
            })?,
        user_settings_path: Some(user_settings_path.to_path_buf()),
    })
    .map_err(|err| ToolError::from_anyhow(&err))?;
    let validation = manifest_validation::validate_manifest(&RunLayer::default(), &built.manifest)
        .map_err(|err| ToolError::from_anyhow(&err))?;
    if !validation.ok {
        return Err(ToolError::message("workflow manifest validation failed"));
    }
    Ok(built.manifest)
}

fn mcp_manifest_args(spec: &CreateRunSpec) -> Option<types::ManifestArgs> {
    let mut input = spec
        .inputs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    input.sort();
    let mut label = spec
        .labels
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    label.sort();
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

fn mcp_run_overrides(spec: &CreateRunSpec) -> Option<RunLayer> {
    build_sparse_run_overrides(RunOverrideInput {
        goal:             spec.goal.as_deref(),
        model:            spec.model.as_deref(),
        provider:         spec.provider.as_deref(),
        sandbox:          spec.sandbox.as_deref(),
        preserve_sandbox: spec.preserve_sandbox,
        dry_run:          spec.dry_run,
        auto_approve:     spec.auto_approve,
        labels:           spec.labels.clone(),
    })
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

    #[test]
    fn answer_payloads_map_to_submit_answer_wire_json() {
        let cases = [
            (json!(true), json!({ "kind": "yes" })),
            (json!(false), json!({ "kind": "no" })),
            (json!("hello"), json!({ "kind": "text", "text": "hello" })),
            (
                json!({ "option": "a" }),
                json!({ "kind": "selected", "option_key": "a" }),
            ),
            (
                json!({ "options": ["a", "b"] }),
                json!({ "kind": "multi_selected", "option_keys": ["a", "b"] }),
            ),
            (
                json!({ "text": "hello" }),
                json!({ "kind": "text", "text": "hello" }),
            ),
        ];

        for (answer, expected) in cases {
            let request = answer_to_submit_request(answer).unwrap();
            assert_eq!(serde_json::to_value(request).unwrap(), expected);
        }
    }

    #[test]
    fn unsupported_answer_object_is_rejected() {
        let err = answer_to_submit_request(json!({ "value": "yes" })).unwrap_err();

        assert!(err.as_str().contains("option, options, text"));
    }

    #[test]
    fn interact_answer_validation_rejects_unsupported_json_before_api_calls() {
        let err = ValidatedInteractRun::try_from(FabroRunInteractParams {
            action:      RunInteractAction::Answer,
            run_id:      "run_123".to_string(),
            message:     None,
            interrupt:   None,
            question_id: Some("question-1".to_string()),
            answer:      Some(json!({ "value": "yes" })),
        })
        .unwrap_err();

        assert!(err.as_str().contains("option, options, text"));
    }

    #[test]
    fn mcp_manifest_args_preserve_input_provenance() {
        let args = mcp_manifest_args(&CreateRunSpec {
            workflow:         "simple".to_string(),
            run_id:           None,
            cwd:              None,
            goal:             None,
            inputs:           HashMap::from([
                ("count".to_string(), json!(3)),
                ("decision".to_string(), json!("approve")),
            ]),
            labels:           HashMap::new(),
            model:            None,
            provider:         None,
            sandbox:          None,
            dry_run:          None,
            auto_approve:     None,
            preserve_sandbox: None,
            start:            None,
        })
        .expect("input args should be present");

        assert_eq!(args.input, vec![r"count=3", r#"decision="approve""#]);
    }
}
