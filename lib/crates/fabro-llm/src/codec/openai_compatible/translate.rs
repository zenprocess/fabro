//! Pure mapping between canonical types and the Chat Completions wire shapes.

use super::wire::{ChatFunction, ChatMessage, ChatToolCall};
use crate::types::{
    ContentPart, CostSource, FinishReason, Message, Request, ResponseFormat, ResponseFormatType,
    Role, ToolChoice, ToolDefinition,
};

// --- <think> prefix stripping (defensive fallback for reasoning models) ---
//
// Some `OpenAI`-compatible reasoning models (minimax, kimi/zai/glm/deepseek
// reasoning variants) emit their `<think>...</think>` block INLINE in the
// `content` stream instead of using a separate `reasoning_content` channel.
// This dialect has no first-class reasoning channel for that case, so the
// leading reasoning prefix is stripped from the visible assistant content
// and surfaced into `ContentPart::Thinking` (the existing reasoning slot)
// via the caller's existing accumulation. To stay safe against accidental
// matches in normal content, the prefix is only treated as reasoning when it
// appears at the very start of the assistant content (allowing leading
// whitespace); any `<think>` token that appears after visible text has been
// emitted is preserved verbatim.

/// Opening delimiter for the inline reasoning prefix.
const THINK_OPEN: &str = "<think>";
/// Closing delimiter for the inline reasoning prefix.
const THINK_CLOSE: &str = "</think>";

/// State machine for the prefix-only `<think>` stripper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StripState {
    /// Initial state. Looking for a leading `<think>` (after optional
    /// whitespace). As soon as any non-whitespace visible text is emitted,
    /// transition to [`Self::Outside`] permanently.
    Prefix,
    /// Pass-through: every byte, including any `<think>` token, is emitted as
    /// visible content.
    Outside,
    /// Inside a reasoning block. Captured bytes feed the reasoning buffer
    /// until `</think>` is found.
    Inside,
}

/// Stateful `<think>` prefix stripper that holds a small tail buffer so a
/// boundary that straddles two `process` calls is still classified correctly.
///
/// Reasoning text is captured internally and can be retrieved via
/// [`Self::into_reasoning`]. The visible text returned by [`Self::process`] is
/// always safe to surface as assistant output.
#[derive(Debug)]
pub(super) struct ThinkStrip {
    state:     StripState,
    pending:   String,
    reasoning: String,
}

impl ThinkStrip {
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            state:     StripState::Prefix,
            pending:   String::new(),
            reasoning: String::new(),
        }
    }

    /// Feed `chunk` and return any text safe to surface as visible assistant
    /// output. Reasoning bytes are appended to the internal buffer.
    pub(super) fn process(&mut self, chunk: &str) -> String {
        if chunk.is_empty() {
            return String::new();
        }
        self.pending.push_str(chunk);
        let mut visible = String::new();
        loop {
            match self.state {
                StripState::Prefix => {
                    if let Some(idx) = self.pending.find(THINK_OPEN) {
                        // Only treat `<think>` as the reasoning prefix when it
                        // appears at the start (with optional leading
                        // whitespace). Any non-whitespace text before it is
                        // visible content and disables the strip permanently.
                        let prefix_is_whitespace =
                            self.pending[..idx].chars().all(char::is_whitespace);
                        if prefix_is_whitespace {
                            self.pending.drain(..idx + THINK_OPEN.len());
                            self.state = StripState::Inside;
                        } else {
                            visible.push_str(&self.pending);
                            self.pending.clear();
                            self.state = StripState::Outside;
                        }
                    } else {
                        // No full opener yet. Split `pending` into the bytes we
                        // could commit now and the ambiguous tail (a proper
                        // prefix of `<think>`) that must be retained across the
                        // chunk boundary.
                        let hold = prefix_match_len(self.pending.as_bytes(), THINK_OPEN);
                        let drain_to = self.pending.len() - hold;
                        if self.pending[..drain_to].chars().all(char::is_whitespace) {
                            // Everything before the ambiguous tail is leading
                            // whitespace, so a `<think>` opener could still
                            // legitimately follow. Retain the whole buffer
                            // (whitespace + partial opener) and stay in
                            // `Prefix`: emitting the whitespace now would be
                            // wrong if the opener completes, since whitespace
                            // before the reasoning prefix is dropped rather
                            // than surfaced.
                            break;
                        }
                        // Non-whitespace visible text precedes the (possible)
                        // opener, which permanently disqualifies the prefix
                        // strip. Emit everything — including the tail, now just
                        // literal text — and switch to pass-through so any
                        // later `<think>` token passes through unchanged.
                        visible.push_str(&self.pending);
                        self.pending.clear();
                        self.state = StripState::Outside;
                        break;
                    }
                }
                StripState::Outside => {
                    visible.push_str(&self.pending);
                    self.pending.clear();
                    break;
                }
                StripState::Inside => {
                    if let Some(idx) = self.pending.find(THINK_CLOSE) {
                        self.reasoning.push_str(&self.pending[..idx]);
                        self.pending.drain(..idx + THINK_CLOSE.len());
                        self.state = StripState::Outside;
                        // Loop again: Outside will flush any remaining visible
                        // bytes (e.g. the newline that follows `</think>`).
                    } else {
                        let hold = prefix_match_len(self.pending.as_bytes(), THINK_CLOSE);
                        if hold < self.pending.len() {
                            let drain_to = self.pending.len() - hold;
                            self.reasoning.push_str(&self.pending[..drain_to]);
                            self.pending.drain(..drain_to);
                        }
                        break;
                    }
                }
            }
        }
        visible
    }

    /// Flush any bytes still held in the tail buffer. Returns the visible
    /// tail; bytes held while in [`StripState::Inside`] are appended to
    /// reasoning (covers an unclosed `<think>` from a truncated stream).
    pub(super) fn finish(&mut self) -> String {
        let remaining = std::mem::take(&mut self.pending);
        match self.state {
            StripState::Prefix | StripState::Outside => remaining,
            StripState::Inside => {
                self.reasoning.push_str(&remaining);
                String::new()
            }
        }
    }

    /// Take any reasoning captured so far, leaving an empty buffer behind.
    pub(super) fn take_reasoning(&mut self) -> String {
        std::mem::take(&mut self.reasoning)
    }

    /// Returns `true` if the stripper is holding bytes (in either reasoning
    /// or the tail buffer) that should be flushed by `finish()`. Used by the
    /// streaming decoder to decide whether to synthesise a `Finish` event
    /// for a stream that ended without `[DONE]` and without visible text.
    pub(super) fn has_pending(&self) -> bool {
        !self.reasoning.is_empty() || !self.pending.is_empty()
    }
}

/// Returns the length of the longest proper prefix of `pattern` that is a
/// suffix of `suffix`, capped at `pattern.len() - 1` (a full match is the
/// caller's job to handle). Used to bound how many bytes the stripper
/// holds back while waiting to see if the next chunk completes a delimiter.
fn prefix_match_len(suffix: &[u8], pattern: &str) -> usize {
    let pattern = pattern.as_bytes();
    let cap = suffix.len().min(pattern.len().saturating_sub(1));
    (1..=cap)
        .rev()
        .find(|&n| suffix[suffix.len() - n..] == pattern[..n])
        .unwrap_or(0)
}

/// One-shot helper for the non-streaming decode path. Splits `text` into
/// `(visible, reasoning)` using the same prefix-only rule as [`ThinkStrip`].
pub(super) fn strip_think_prefix(text: &str) -> (String, String) {
    let mut strip = ThinkStrip::new();
    let mut visible = strip.process(text);
    let tail = strip.finish();
    visible.push_str(&tail);
    (visible, strip.take_reasoning())
}

/// In-band cost (OpenRouter) is authoritative billing data; the client's
/// catalog estimate never overwrites it.
pub(super) fn authoritative_cost_source(cost_usd: Option<f64>) -> Option<CostSource> {
    cost_usd.is_some().then_some(CostSource::Authoritative)
}

pub(super) fn map_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop") | None => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("tool_calls") => FinishReason::ToolCalls,
        Some("content_filter") => FinishReason::ContentFilter,
        Some(other) => FinishReason::Other(other.to_string()),
    }
}

/// Build the content string from a message's parts, including fallback text
/// for unsupported content types (Audio, Document).
fn content_text_with_fallbacks(parts: &[ContentPart]) -> String {
    let mut segments: Vec<String> = Vec::new();
    for part in parts {
        match part {
            ContentPart::Text(text) => segments.push(text.clone()),
            ContentPart::Audio(_) => {
                segments.push("[Audio content not supported by this provider]".to_string());
            }
            ContentPart::Document(doc) => {
                let desc = doc.file_name.as_ref().map_or_else(
                    || "[Document content not supported by this provider]".to_string(),
                    |name| {
                        format!("[Document '{name}': content type not supported by this provider]")
                    },
                );
                segments.push(desc);
            }
            _ => {}
        }
    }
    segments.join("")
}

pub(super) fn translate_messages(messages: &[Message]) -> Vec<ChatMessage> {
    messages
        .iter()
        .flat_map(|msg| {
            // Tool messages must be split into one ChatMessage per ToolResult,
            // each with its own tool_call_id. The Chat Completions API requires
            // every tool_call_id from the assistant to have a matching tool message.
            if msg.role == Role::Tool {
                return msg
                    .content
                    .iter()
                    .filter_map(|part| {
                        if let ContentPart::ToolResult(tr) = part {
                            let output = tr
                                .content
                                .as_str()
                                .map_or_else(|| tr.content.to_string(), str::to_string);
                            Some(ChatMessage {
                                role:              "tool".to_string(),
                                content:           Some(output),
                                reasoning_content: None,
                                tool_call_id:      Some(tr.tool_call_id.clone()),
                                tool_calls:        None,
                            })
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
            }

            let role = match msg.role {
                Role::System | Role::Developer => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => unreachable!(
                    "Role::Tool is handled in the early-return branch above this match"
                ),
            };

            let mut tool_calls: Vec<ChatToolCall> = Vec::new();
            if msg.role == Role::Assistant {
                for part in &msg.content {
                    if let ContentPart::ToolCall(tc) = part {
                        let arguments = tc
                            .raw_arguments
                            .clone()
                            .unwrap_or_else(|| tc.arguments.to_string());
                        tool_calls.push(ChatToolCall {
                            id:       tc.id.clone(),
                            kind:     "function".to_string(),
                            function: ChatFunction {
                                name: tc.name.clone(),
                                arguments,
                            },
                        });
                    }
                }
            }

            let text = content_text_with_fallbacks(&msg.content);
            let content = if text.is_empty() { None } else { Some(text) };
            let tool_calls = if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            };

            // Extract reasoning/thinking content for assistant messages.
            let reasoning_content = if msg.role == Role::Assistant {
                let reasoning: String = msg
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Thinking(t) if !t.redacted => Some(t.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if reasoning.is_empty() {
                    None
                } else {
                    Some(reasoning)
                }
            } else {
                None
            };

            vec![ChatMessage {
                role: role.to_string(),
                content,
                reasoning_content,
                tool_call_id: msg.tool_call_id.clone(),
                tool_calls,
            }]
        })
        .collect()
}

pub(super) fn translate_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            })
        })
        .collect()
}

pub(super) fn translate_tool_choice(choice: &ToolChoice) -> serde_json::Value {
    match choice {
        ToolChoice::Auto => serde_json::json!("auto"),
        ToolChoice::None => serde_json::json!("none"),
        ToolChoice::Required => serde_json::json!("required"),
        ToolChoice::Named { tool_name } => {
            serde_json::json!({"type": "function", "function": {"name": tool_name}})
        }
    }
}

pub(super) fn custom_tool_names(request: &Request) -> Vec<String> {
    request
        .tools
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|tool| tool.is_custom())
        .map(|tool| tool.name.clone())
        .collect()
}

pub(super) fn parse_tool_arguments(
    tool_name: &str,
    raw_arguments: &str,
    custom_tool_names: &[String],
) -> serde_json::Value {
    match serde_json::from_str(raw_arguments) {
        Ok(arguments) => arguments,
        Err(_) if custom_tool_names.iter().any(|name| name == tool_name) => {
            serde_json::Value::String(raw_arguments.to_string())
        }
        Err(_) => serde_json::json!({}),
    }
}

/// Translate unified `ResponseFormat` to Chat Completions `response_format`.
pub(super) fn translate_response_format(format: &ResponseFormat) -> serde_json::Value {
    match format.kind {
        ResponseFormatType::Text => serde_json::json!({"type": "text"}),
        ResponseFormatType::JsonObject => serde_json::json!({"type": "json_object"}),
        ResponseFormatType::JsonSchema => {
            let mut json_schema = serde_json::json!({
                "name": "response",
                "strict": format.strict,
            });
            if let Some(schema) = &format.json_schema {
                json_schema["schema"] = schema.clone();
            }
            serde_json::json!({
                "type": "json_schema",
                "json_schema": json_schema,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AudioData, ContentPart, DocumentData, Message, Role, ToolCall};

    #[test]
    fn translate_assistant_message_with_tool_calls_only() {
        let msg = Message {
            role:         Role::Assistant,
            content:      vec![ContentPart::ToolCall(ToolCall::new(
                "call_1",
                "get_weather",
                serde_json::json!({"city": "SF"}),
            ))],
            name:         None,
            tool_call_id: None,
        };
        let translated = translate_messages(&[msg]);
        assert_eq!(translated.len(), 1);
        assert_eq!(translated[0].role, "assistant");
        assert!(translated[0].content.is_none());
        let tool_calls = translated[0].tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].kind, "function");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert_eq!(tool_calls[0].function.arguments, r#"{"city":"SF"}"#);
    }

    #[test]
    fn translate_assistant_message_with_text_and_tool_calls() {
        let msg = Message {
            role:         Role::Assistant,
            content:      vec![
                ContentPart::text("Let me check the weather"),
                ContentPart::ToolCall(ToolCall::new(
                    "call_2",
                    "get_weather",
                    serde_json::json!({"city": "NYC"}),
                )),
            ],
            name:         None,
            tool_call_id: None,
        };
        let translated = translate_messages(&[msg]);
        assert_eq!(
            translated[0].content.as_deref(),
            Some("Let me check the weather")
        );
        let tool_calls = translated[0].tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "get_weather");
    }

    #[test]
    fn translate_assistant_message_with_raw_arguments() {
        let mut tc = ToolCall::new("call_3", "search", serde_json::json!({"q": "rust"}));
        tc.raw_arguments = Some(r#"{"q": "rust"}"#.to_string());
        let msg = Message {
            role:         Role::Assistant,
            content:      vec![ContentPart::ToolCall(tc)],
            name:         None,
            tool_call_id: None,
        };
        let translated = translate_messages(&[msg]);
        let tool_calls = translated[0].tool_calls.as_ref().unwrap();
        // Should prefer raw_arguments over serializing arguments
        assert_eq!(tool_calls[0].function.arguments, r#"{"q": "rust"}"#);
    }

    #[test]
    fn translate_tool_message_has_tool_call_id() {
        let msg = Message::tool_result(
            "call_1",
            serde_json::Value::String("72F and sunny".into()),
            false,
        );
        let translated = translate_messages(&[msg]);
        assert_eq!(translated[0].role, "tool");
        assert_eq!(translated[0].tool_call_id.as_deref(), Some("call_1"));
        assert!(translated[0].tool_calls.is_none());
    }

    #[test]
    fn translate_user_message_has_no_tool_calls() {
        let msg = Message::user("Hello");
        let translated = translate_messages(&[msg]);
        assert_eq!(translated[0].role, "user");
        assert_eq!(translated[0].content.as_deref(), Some("Hello"));
        assert!(translated[0].tool_calls.is_none());
    }

    #[test]
    fn assistant_tool_calls_serialize_correctly() {
        let msg = Message {
            role:         Role::Assistant,
            content:      vec![ContentPart::ToolCall(ToolCall::new(
                "call_1",
                "get_weather",
                serde_json::json!({"city": "SF"}),
            ))],
            name:         None,
            tool_call_id: None,
        };
        let translated = translate_messages(&[msg]);
        let json = serde_json::to_value(&translated[0]).unwrap();
        assert!(json.get("content").is_none());
        assert!(json.get("tool_call_id").is_none());
        let tool_calls = json["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["type"], "function");
        assert_eq!(tool_calls[0]["id"], "call_1");
        assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
    }

    #[test]
    fn audio_content_produces_text_fallback() {
        let msg = Message {
            role:         Role::User,
            content:      vec![ContentPart::Audio(AudioData {
                url:        Some("https://example.com/audio.wav".to_string()),
                data:       None,
                media_type: None,
            })],
            name:         None,
            tool_call_id: None,
        };
        let translated = translate_messages(&[msg]);
        assert_eq!(
            translated[0].content.as_deref(),
            Some("[Audio content not supported by this provider]")
        );
    }

    #[test]
    fn document_content_produces_text_fallback_with_filename() {
        let msg = Message {
            role:         Role::User,
            content:      vec![ContentPart::Document(DocumentData {
                url:        Some("https://example.com/doc.pdf".to_string()),
                data:       None,
                media_type: None,
                file_name:  Some("report.pdf".to_string()),
            })],
            name:         None,
            tool_call_id: None,
        };
        let translated = translate_messages(&[msg]);
        assert_eq!(
            translated[0].content.as_deref(),
            Some("[Document 'report.pdf': content type not supported by this provider]")
        );
    }

    #[test]
    fn document_content_produces_text_fallback_without_filename() {
        let msg = Message {
            role:         Role::User,
            content:      vec![ContentPart::Document(DocumentData {
                url:        None,
                data:       Some(vec![1, 2, 3]),
                media_type: None,
                file_name:  None,
            })],
            name:         None,
            tool_call_id: None,
        };
        let translated = translate_messages(&[msg]);
        assert_eq!(
            translated[0].content.as_deref(),
            Some("[Document content not supported by this provider]")
        );
    }

    #[test]
    fn mixed_text_and_audio_content_concatenates() {
        let msg = Message {
            role:         Role::User,
            content:      vec![
                ContentPart::text("Check this: "),
                ContentPart::Audio(AudioData {
                    url:        None,
                    data:       Some(vec![1, 2]),
                    media_type: None,
                }),
            ],
            name:         None,
            tool_call_id: None,
        };
        let translated = translate_messages(&[msg]);
        assert_eq!(
            translated[0].content.as_deref(),
            Some("Check this: [Audio content not supported by this provider]")
        );
    }
}

// --- think_strip tests ---

#[cfg(test)]
mod think_strip_tests {
    use super::{strip_think_prefix, ThinkStrip};

    /// Drive `strip` with a list of chunk boundaries and join the visible
    /// output across calls. Mirrors how the streaming decoder calls
    /// `process` once per `delta.content` arrival.
    fn run_strip(chunks: &[&str]) -> (String, String) {
        let mut strip = ThinkStrip::new();
        let mut visible = String::new();
        for chunk in chunks {
            visible.push_str(&strip.process(chunk));
        }
        let tail = strip.finish();
        visible.push_str(&tail);
        (visible, strip.take_reasoning())
    }

    #[test]
    fn no_think_passthrough() {
        let (visible, reasoning) = run_strip(&["Hello world"]);
        assert_eq!(visible, "Hello world");
        assert_eq!(reasoning, "");
    }

    #[test]
    fn think_then_visible_answer() {
        let (visible, reasoning) =
            run_strip(&["<think>reasoning</think>\nThe answer"]);
        assert_eq!(visible, "\nThe answer");
        assert_eq!(reasoning, "reasoning");
    }

    #[test]
    fn multi_delta_think_prefix() {
        // Observed live: `<think>` opens in delta 1, the reasoning text and
        // closing tag straddle later deltas, and the visible answer arrives
        // after the closing tag.
        let (visible, reasoning) = run_strip(&[
            "<think>\nThe user",
            " said hello</think>",
            "\n\nThe answer is 42.",
        ]);
        assert_eq!(visible, "\n\nThe answer is 42.");
        assert_eq!(reasoning, "\nThe user said hello");
    }

    #[test]
    fn unclosed_think_is_captured_as_reasoning() {
        let (visible, reasoning) = run_strip(&["<think>never closed"]);
        assert_eq!(visible, "");
        assert_eq!(reasoning, "never closed");
    }

    #[test]
    fn mid_content_literal_think_is_not_stripped() {
        // A normal response that happens to contain the literal token
        // `<think>` after some visible text must pass through unchanged.
        let (visible, reasoning) =
            run_strip(&["Here is how you write <think> in plain text"]);
        assert_eq!(visible, "Here is how you write <think> in plain text");
        assert_eq!(reasoning, "");
    }

    #[test]
    fn think_split_across_two_deltas() {
        // `<think>` straddles the boundary between two deltas — the strip
        // must hold the tail back so the opening tag is recognised. Combined
        // string is `<think>secret</think>ok`.
        let (visible, reasoning) = run_strip(&["<thi", "nk>secret</think>ok"]);
        assert_eq!(visible, "ok");
        assert_eq!(reasoning, "secret");
    }

    #[test]
    fn think_opener_one_byte_per_delta() {
        // Case (a): the opener arrives one byte at a time. Each partial
        // opener is the whole buffer (`hold == pending.len()`), so it must be
        // retained until `<think>` completes.
        let (visible, reasoning) =
            run_strip(&["<", "t", "h", "i", "n", "k", ">", "hidden</think>shown"]);
        assert_eq!(visible, "shown");
        assert_eq!(reasoning, "hidden");
    }

    #[test]
    fn partial_opener_then_nonmatch_flushes_as_visible() {
        // Case (b): "<thi" is a partial opener, but the next byte makes it
        // "<this" — a proven non-match. The held partial must flush as
        // visible content, not be swallowed.
        let (visible, reasoning) = run_strip(&["<thi", "s is text"]);
        assert_eq!(visible, "<this is text");
        assert_eq!(reasoning, "");
    }

    #[test]
    fn think_split_with_leading_whitespace_is_stripped() {
        // Case (c): a leading newline/space precedes a `<think>` opener that
        // is itself split across the delta boundary. The whitespace must NOT
        // trigger a premature switch to pass-through; the opener must still be
        // recognised across the boundary and the reasoning stripped. This is
        // the case that exercises the `0 < hold < pending.len()` Prefix branch
        // where the drained bytes are whitespace — the bug the fix targets.
        let (visible, reasoning) = run_strip(&[" <thi", "nk>secret</think>ok"]);
        assert_eq!(visible, "ok");
        assert_eq!(reasoning, "secret");
    }

    #[test]
    fn think_all_delimiters_split_across_deltas() {
        // Case (d): content, opener, closer, and answer are each fragmented
        // across delta boundaries — the maximal-split reasoning flow.
        let (visible, reasoning) =
            run_strip(&["<thi", "nk>rea", "soning</thi", "nk>ans", "wer"]);
        assert_eq!(visible, "answer");
        assert_eq!(reasoning, "reasoning");
    }

    #[test]
    fn leading_whitespace_before_think_is_still_treated_as_prefix() {
        let (visible, reasoning) =
            run_strip(&["\n <think>plan</think>\ndo it"]);
        assert_eq!(visible, "\ndo it");
        assert_eq!(reasoning, "plan");
    }

    #[test]
    fn visible_text_then_later_think_stays_literal() {
        let (visible, reasoning) =
            run_strip(&["hello ", "<think> should not trigger"]);
        assert_eq!(visible, "hello <think> should not trigger");
        assert_eq!(reasoning, "");
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let (visible, reasoning) = run_strip(&[""]);
        assert_eq!(visible, "");
        assert_eq!(reasoning, "");
    }

    #[test]
    fn non_streaming_helper_matches_stateful_path() {
        let input = "<think>\nstep 1\nstep 2</think>\nFinal answer.";
        let (visible, reasoning) = strip_think_prefix(input);
        assert_eq!(visible, "\nFinal answer.");
        assert_eq!(reasoning, "\nstep 1\nstep 2");
    }
}
