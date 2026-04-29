use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use codex_app_server_protocol::{
    CommandExecutionOutputDeltaNotification, CommandExecutionStatus as AppCommandExecutionStatus,
    DynamicToolCallStatus as AppDynamicToolCallStatus,
    ItemCompletedNotification as AppItemCompletedNotification,
    ItemStartedNotification as AppItemStartedNotification, JSONRPCNotification, JSONRPCRequest,
    JSONRPCResponse, ServerNotification, ServerRequest, ThreadForkResponse,
    ThreadItem as AppThreadItem, ThreadStartResponse, ThreadStartedNotification,
    ToolRequestUserInputQuestion,
};
use codex_protocol::{
    openai_models::ReasoningEffort,
    plan_tool::{StepStatus, UpdatePlanArgs},
    protocol::{
        AgentMessageDeltaEvent, AgentMessageEvent, AgentReasoningDeltaEvent, AgentReasoningEvent,
        AgentReasoningSectionBreakEvent, ApplyPatchApprovalRequestEvent, BackgroundEventEvent,
        ErrorEvent, EventMsg, ExecApprovalRequestEvent, ExecCommandBeginEvent, ExecCommandEndEvent,
        ExecCommandOutputDeltaEvent, ExecOutputStream, FileChange as CodexProtoFileChange,
        McpInvocation, McpToolCallBeginEvent, McpToolCallEndEvent, PatchApplyBeginEvent,
        PatchApplyEndEvent, StreamErrorEvent, TokenUsageInfo, ViewImageToolCallEvent, WarningEvent,
        WebSearchBeginEvent, WebSearchEndEvent,
    },
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use workspace_utils::{
    approvals::ApprovalStatus, diff::normalize_unified_diff, msg_store::MsgStore,
    path::make_path_relative,
};

use crate::{
    approvals::ToolCallMetadata,
    logs::{
        ActionType, CommandExitStatus, CommandRunResult, FileChange, NormalizedEntry,
        NormalizedEntryError, NormalizedEntryType, TodoItem, ToolResult, ToolResultValueType,
        ToolStatus,
        stderr_processor::normalize_stderr_logs,
        utils::{
            ConversationPatch, EntryIndexProvider,
            patch::{add_normalized_entry, replace_normalized_entry, upsert_normalized_entry},
        },
    },
};

trait ToNormalizedEntry {
    fn to_normalized_entry(&self) -> NormalizedEntry;
}

trait ToNormalizedEntryOpt {
    fn to_normalized_entry_opt(&self) -> Option<NormalizedEntry>;
}

const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
struct CodexNotificationParams {
    #[serde(rename = "msg")]
    msg: EventMsg,
}

#[derive(Default)]
struct StreamingText {
    index: usize,
    content: String,
}

#[derive(Default)]
struct CommandState {
    index: Option<usize>,
    command: String,
    stdout: BoundedOutput,
    stderr: BoundedOutput,
    formatted_output: Option<BoundedOutput>,
    status: ToolStatus,
    exit_code: Option<i32>,
    awaiting_approval: bool,
    call_id: String,
}

#[derive(Default)]
struct BoundedOutput {
    content: String,
    truncated_bytes: usize,
}

impl BoundedOutput {
    fn from_string(content: String) -> Self {
        let mut output = Self {
            content,
            truncated_bytes: 0,
        };
        output.enforce_limit();
        output
    }

    fn push_str(&mut self, chunk: &str) {
        self.content.push_str(chunk);
        self.enforce_limit();
    }

    fn as_labeled_section(&self, label: &str) -> Option<String> {
        let cleaned = self.content.trim();
        if cleaned.is_empty() {
            return None;
        }

        let mut section = String::new();
        if self.truncated_bytes > 0 {
            section.push_str(&format!(
                "[{label} truncated, omitted {} bytes]\n",
                self.truncated_bytes
            ));
        }
        section.push_str(label);
        section.push_str(":\n");
        section.push_str(cleaned);
        Some(section)
    }

    fn as_command_output(&self) -> Option<String> {
        let cleaned = self.content.trim();
        if cleaned.is_empty() {
            return None;
        }

        let mut output = String::new();
        if self.truncated_bytes > 0 {
            output.push_str(&format!(
                "[command output truncated, omitted {} bytes]\n",
                self.truncated_bytes
            ));
        }
        output.push_str(cleaned);
        Some(output)
    }

    fn enforce_limit(&mut self) {
        if self.content.len() <= MAX_COMMAND_OUTPUT_BYTES {
            return;
        }

        let keep_from = ceil_char_boundary(
            &self.content,
            self.content.len() - MAX_COMMAND_OUTPUT_BYTES,
        );
        self.content.drain(..keep_from);
        self.truncated_bytes = self.truncated_bytes.saturating_add(keep_from);
    }
}

impl From<String> for BoundedOutput {
    fn from(value: String) -> Self {
        Self::from_string(value)
    }
}

fn ceil_char_boundary(content: &str, offset: usize) -> usize {
    let mut boundary = offset.min(content.len());
    while boundary < content.len() && !content.is_char_boundary(boundary) {
        boundary += 1;
    }
    boundary
}

impl CommandState {
    fn push_stdout_chunk(&mut self, chunk: impl AsRef<str>) {
        self.stdout.push_str(chunk.as_ref());
    }

    fn push_stderr_chunk(&mut self, chunk: impl AsRef<str>) {
        self.stderr.push_str(chunk.as_ref());
    }

    fn set_formatted_output(&mut self, output: String) {
        self.formatted_output = Some(output.into());
    }

    fn apply_output_delta(&mut self, stream: ExecOutputStream, chunk: impl AsRef<str>) {
        match stream {
            ExecOutputStream::Stdout => self.push_stdout_chunk(chunk),
            ExecOutputStream::Stderr => self.push_stderr_chunk(chunk),
        }
    }
}

impl ToNormalizedEntry for CommandState {
    fn to_normalized_entry(&self) -> NormalizedEntry {
        let content = self.command.to_string();

        NormalizedEntry {
            timestamp: None,
            entry_type: NormalizedEntryType::ToolUse {
                tool_name: "bash".to_string(),
                action_type: ActionType::CommandRun {
                    command: self.command.clone(),
                    result: Some(CommandRunResult {
                        exit_status: self
                            .exit_code
                            .map(|code| CommandExitStatus::ExitCode { code }),
                        output: if let Some(formatted_output) = &self.formatted_output {
                            formatted_output.as_command_output()
                        } else {
                            build_command_output(
                                self.stdout.as_labeled_section("stdout"),
                                self.stderr.as_labeled_section("stderr"),
                            )
                        },
                    }),
                },
                status: self.status.clone(),
            },
            content,
            metadata: serde_json::to_value(ToolCallMetadata {
                tool_call_id: self.call_id.clone(),
            })
            .ok(),
        }
    }
}

struct McpToolState {
    index: Option<usize>,
    invocation: McpInvocation,
    result: Option<ToolResult>,
    status: ToolStatus,
}

struct DynamicToolState {
    index: Option<usize>,
    tool: String,
    arguments: Value,
    result: Option<ToolResult>,
    status: ToolStatus,
    call_id: String,
}

impl ToNormalizedEntry for DynamicToolState {
    fn to_normalized_entry(&self) -> NormalizedEntry {
        NormalizedEntry {
            timestamp: None,
            entry_type: NormalizedEntryType::ToolUse {
                tool_name: self.tool.clone(),
                action_type: ActionType::Tool {
                    tool_name: self.tool.clone(),
                    arguments: Some(self.arguments.clone()),
                    result: self.result.clone(),
                },
                status: self.status.clone(),
            },
            content: self.tool.clone(),
            metadata: serde_json::to_value(ToolCallMetadata {
                tool_call_id: self.call_id.clone(),
            })
            .ok(),
        }
    }
}

impl ToNormalizedEntry for McpToolState {
    fn to_normalized_entry(&self) -> NormalizedEntry {
        let tool_name = format!("mcp:{}:{}", self.invocation.server, self.invocation.tool);
        NormalizedEntry {
            timestamp: None,
            entry_type: NormalizedEntryType::ToolUse {
                tool_name: tool_name.clone(),
                action_type: ActionType::Tool {
                    tool_name,
                    arguments: self.invocation.arguments.clone(),
                    result: self.result.clone(),
                },
                status: self.status.clone(),
            },
            content: self.invocation.tool.clone(),
            metadata: None,
        }
    }
}

struct UserInputRequestState {
    index: Option<usize>,
    content: String,
    arguments: Value,
    result: Option<ToolResult>,
    status: ToolStatus,
    call_id: String,
}

impl ToNormalizedEntry for UserInputRequestState {
    fn to_normalized_entry(&self) -> NormalizedEntry {
        NormalizedEntry {
            timestamp: None,
            entry_type: NormalizedEntryType::ToolUse {
                tool_name: "question".to_string(),
                action_type: ActionType::Tool {
                    tool_name: "question".to_string(),
                    arguments: Some(self.arguments.clone()),
                    result: self.result.clone(),
                },
                status: self.status.clone(),
            },
            content: self.content.clone(),
            metadata: serde_json::to_value(ToolCallMetadata {
                tool_call_id: self.call_id.clone(),
            })
            .ok(),
        }
    }
}

#[derive(Default)]
struct WebSearchState {
    index: Option<usize>,
    query: Option<String>,
    status: ToolStatus,
}

impl WebSearchState {
    fn new() -> Self {
        Default::default()
    }
}

impl ToNormalizedEntry for WebSearchState {
    fn to_normalized_entry(&self) -> NormalizedEntry {
        NormalizedEntry {
            timestamp: None,
            entry_type: NormalizedEntryType::ToolUse {
                tool_name: "web_search".to_string(),
                action_type: ActionType::WebFetch {
                    url: self.query.clone().unwrap_or_else(|| "...".to_string()),
                },
                status: self.status.clone(),
            },
            content: self
                .query
                .clone()
                .unwrap_or_else(|| "Web search".to_string()),
            metadata: None,
        }
    }
}

#[derive(Default)]
struct PatchState {
    entries: Vec<PatchEntry>,
}

struct PatchEntry {
    index: Option<usize>,
    path: String,
    changes: Vec<FileChange>,
    status: ToolStatus,
    awaiting_approval: bool,
    call_id: String,
}

impl ToNormalizedEntry for PatchEntry {
    fn to_normalized_entry(&self) -> NormalizedEntry {
        let content = self.path.clone();

        NormalizedEntry {
            timestamp: None,
            entry_type: NormalizedEntryType::ToolUse {
                tool_name: "edit".to_string(),
                action_type: ActionType::FileEdit {
                    path: self.path.clone(),
                    changes: self.changes.clone(),
                },
                status: self.status.clone(),
            },
            content,
            metadata: serde_json::to_value(ToolCallMetadata {
                tool_call_id: self.call_id.clone(),
            })
            .ok(),
        }
    }
}

struct LogState {
    entry_index: EntryIndexProvider,
    assistant: Option<StreamingText>,
    thinking: Option<StreamingText>,
    commands: HashMap<String, CommandState>,
    mcp_tools: HashMap<String, McpToolState>,
    dynamic_tools: HashMap<String, DynamicToolState>,
    patches: HashMap<String, PatchState>,
    web_searches: HashMap<String, WebSearchState>,
    user_input_requests: HashMap<String, UserInputRequestState>,
    token_usage_info: Option<TokenUsageInfo>,
}

enum StreamingTextKind {
    Assistant,
    Thinking,
}

impl LogState {
    fn new(entry_index: EntryIndexProvider) -> Self {
        Self {
            entry_index,
            assistant: None,
            thinking: None,
            commands: HashMap::new(),
            mcp_tools: HashMap::new(),
            dynamic_tools: HashMap::new(),
            patches: HashMap::new(),
            web_searches: HashMap::new(),
            user_input_requests: HashMap::new(),
            token_usage_info: None,
        }
    }

    fn streaming_text_update(
        &mut self,
        content: String,
        type_: StreamingTextKind,
        mode: UpdateMode,
    ) -> (NormalizedEntry, usize, bool) {
        let index_provider = &self.entry_index;
        let entry = match type_ {
            StreamingTextKind::Assistant => &mut self.assistant,
            StreamingTextKind::Thinking => &mut self.thinking,
        };
        let is_new = entry.is_none();
        let (content, index) = if entry.is_none() {
            let index = index_provider.next();
            *entry = Some(StreamingText { index, content });
            (&entry.as_ref().unwrap().content, index)
        } else {
            let streaming_state = entry.as_mut().unwrap();
            match mode {
                UpdateMode::Append => streaming_state.content.push_str(&content),
                UpdateMode::Set => streaming_state.content = content,
            }
            (&streaming_state.content, streaming_state.index)
        };
        let normalized_entry = NormalizedEntry {
            timestamp: None,
            entry_type: match type_ {
                StreamingTextKind::Assistant => NormalizedEntryType::AssistantMessage,
                StreamingTextKind::Thinking => NormalizedEntryType::Thinking,
            },
            content: content.clone(),
            metadata: None,
        };
        (normalized_entry, index, is_new)
    }

    fn streaming_text_append(
        &mut self,
        content: String,
        type_: StreamingTextKind,
    ) -> (NormalizedEntry, usize, bool) {
        self.streaming_text_update(content, type_, UpdateMode::Append)
    }

    fn streaming_text_set(
        &mut self,
        content: String,
        type_: StreamingTextKind,
    ) -> (NormalizedEntry, usize, bool) {
        self.streaming_text_update(content, type_, UpdateMode::Set)
    }

    fn assistant_message_append(&mut self, content: String) -> (NormalizedEntry, usize, bool) {
        self.streaming_text_append(content, StreamingTextKind::Assistant)
    }

    fn thinking_append(&mut self, content: String) -> (NormalizedEntry, usize, bool) {
        self.streaming_text_append(content, StreamingTextKind::Thinking)
    }

    fn assistant_message(&mut self, content: String) -> (NormalizedEntry, usize, bool) {
        self.streaming_text_set(content, StreamingTextKind::Assistant)
    }

    fn thinking(&mut self, content: String) -> (NormalizedEntry, usize, bool) {
        self.streaming_text_set(content, StreamingTextKind::Thinking)
    }
}

enum UpdateMode {
    Append,
    Set,
}

fn normalize_file_changes(
    worktree_path: &str,
    changes: &HashMap<PathBuf, CodexProtoFileChange>,
) -> Vec<(String, Vec<FileChange>)> {
    changes
        .iter()
        .map(|(path, change)| {
            let path_str = path.to_string_lossy();
            let relative = make_path_relative(path_str.as_ref(), worktree_path);
            let file_changes = match change {
                CodexProtoFileChange::Add { content } => vec![FileChange::Write {
                    content: content.clone(),
                }],
                CodexProtoFileChange::Delete { .. } => vec![FileChange::Delete],
                CodexProtoFileChange::Update {
                    unified_diff,
                    move_path,
                } => {
                    let mut edits = Vec::new();
                    if let Some(dest) = move_path {
                        let dest_rel =
                            make_path_relative(dest.to_string_lossy().as_ref(), worktree_path);
                        edits.push(FileChange::Rename { new_path: dest_rel });
                    }
                    let diff = normalize_unified_diff(&relative, unified_diff);
                    edits.push(FileChange::Edit {
                        unified_diff: diff,
                        has_line_numbers: true,
                    });
                    edits
                }
            };
            (relative, file_changes)
        })
        .collect()
}

fn app_command_status_to_tool_status(status: &AppCommandExecutionStatus) -> ToolStatus {
    match status {
        AppCommandExecutionStatus::InProgress => ToolStatus::Created,
        AppCommandExecutionStatus::Completed => ToolStatus::Success,
        AppCommandExecutionStatus::Failed => ToolStatus::Failed,
        AppCommandExecutionStatus::Declined => ToolStatus::Denied { reason: None },
    }
}

fn app_dynamic_tool_status_to_tool_status(status: &AppDynamicToolCallStatus) -> ToolStatus {
    match status {
        AppDynamicToolCallStatus::InProgress => ToolStatus::Created,
        AppDynamicToolCallStatus::Completed => ToolStatus::Success,
        AppDynamicToolCallStatus::Failed => ToolStatus::Failed,
    }
}

fn question_request_content(questions: &[ToolRequestUserInputQuestion]) -> String {
    questions
        .iter()
        .map(|question| question.question.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn upsert_question_request_state(
    state: &mut LogState,
    msg_store: &Arc<MsgStore>,
    entry_index: &EntryIndexProvider,
    call_id: String,
    questions: &[ToolRequestUserInputQuestion],
) {
    let question_state =
        state
            .user_input_requests
            .entry(call_id.clone())
            .or_insert(UserInputRequestState {
                index: None,
                content: question_request_content(questions),
                arguments: serde_json::to_value(questions).unwrap_or(Value::Null),
                result: None,
                status: ToolStatus::Created,
                call_id,
            });
    let index = question_state.index.unwrap_or_else(|| {
        add_normalized_entry(msg_store, entry_index, question_state.to_normalized_entry())
    });
    question_state.index = Some(index);
    replace_normalized_entry(msg_store, index, question_state.to_normalized_entry());
}

fn upsert_dynamic_tool_state(
    state: &mut LogState,
    msg_store: &Arc<MsgStore>,
    entry_index: &EntryIndexProvider,
    call_id: String,
    tool: String,
    arguments: Value,
    status: ToolStatus,
    result: Option<ToolResult>,
) {
    let dynamic_state = state
        .dynamic_tools
        .entry(call_id.clone())
        .or_insert(DynamicToolState {
            index: None,
            tool,
            arguments,
            result: None,
            status: ToolStatus::Created,
            call_id,
        });
    dynamic_state.status = status;
    dynamic_state.result = result;
    let index = dynamic_state.index.unwrap_or_else(|| {
        add_normalized_entry(msg_store, entry_index, dynamic_state.to_normalized_entry())
    });
    dynamic_state.index = Some(index);
    replace_normalized_entry(msg_store, index, dynamic_state.to_normalized_entry());
}

fn handle_direct_item_started(
    notification: AppItemStartedNotification,
    state: &mut LogState,
    msg_store: &Arc<MsgStore>,
    entry_index: &EntryIndexProvider,
) {
    state.assistant = None;
    state.thinking = None;

    match notification.item {
        AppThreadItem::CommandExecution { id, command, .. } => {
            let mut command_state = state.commands.remove(&id).unwrap_or_default();
            command_state.command = command;
            command_state.status = ToolStatus::Created;
            command_state.awaiting_approval = false;
            command_state.call_id = id.clone();
            let index = command_state.index.unwrap_or_else(|| {
                add_normalized_entry(msg_store, entry_index, command_state.to_normalized_entry())
            });
            command_state.index = Some(index);
            replace_normalized_entry(msg_store, index, command_state.to_normalized_entry());
            state.commands.insert(id, command_state);
        }
        AppThreadItem::DynamicToolCall {
            id,
            tool,
            arguments,
            ..
        } => {
            upsert_dynamic_tool_state(
                state,
                msg_store,
                entry_index,
                id,
                tool,
                arguments,
                ToolStatus::Created,
                None,
            );
        }
        _ => {}
    }
}

fn handle_direct_item_completed(
    notification: AppItemCompletedNotification,
    state: &mut LogState,
    msg_store: &Arc<MsgStore>,
    entry_index: &EntryIndexProvider,
) {
    match notification.item {
        AppThreadItem::AgentMessage { text, .. } => {
            state.thinking = None;
            let (entry, index, is_new) = state.assistant_message(text);
            upsert_normalized_entry(msg_store, index, entry, is_new);
            state.assistant = None;
        }
        AppThreadItem::Reasoning { summary, .. } => {
            if !summary.is_empty() {
                state.assistant = None;
                let (entry, index, is_new) = state.thinking(summary.join("\n\n"));
                upsert_normalized_entry(msg_store, index, entry, is_new);
                state.thinking = None;
            }
        }
        AppThreadItem::CommandExecution {
            id,
            aggregated_output,
            exit_code,
            status,
            ..
        } => {
            if let Some(mut command_state) = state.commands.remove(&id) {
                command_state.formatted_output = aggregated_output.map(Into::into);
                command_state.exit_code = exit_code;
                command_state.awaiting_approval = false;
                command_state.status = app_command_status_to_tool_status(&status);
                if let Some(index) = command_state.index {
                    replace_normalized_entry(
                        msg_store,
                        index,
                        command_state.to_normalized_entry(),
                    );
                }
            }
        }
        AppThreadItem::DynamicToolCall {
            id,
            tool,
            arguments,
            status,
            content_items,
            success,
            ..
        } => {
            let tool_status = match success {
                Some(false) => ToolStatus::Failed,
                _ => app_dynamic_tool_status_to_tool_status(&status),
            };
            let result = content_items.map(|items| {
                ToolResult::json(serde_json::to_value(items).unwrap_or(Value::Null))
            });
            upsert_dynamic_tool_state(
                state,
                msg_store,
                entry_index,
                id,
                tool,
                arguments,
                tool_status,
                result,
            );
        }
        _ => {}
    }
}

fn handle_direct_request(
    request: ServerRequest,
    state: &mut LogState,
    msg_store: &Arc<MsgStore>,
    entry_index: &EntryIndexProvider,
) -> bool {
    match request {
        ServerRequest::ToolRequestUserInput { params, .. } => {
            upsert_question_request_state(
                state,
                msg_store,
                entry_index,
                params.item_id,
                &params.questions,
            );
            true
        }
        ServerRequest::DynamicToolCall { params, .. } => {
            upsert_dynamic_tool_state(
                state,
                msg_store,
                entry_index,
                params.call_id,
                params.tool,
                params.arguments,
                ToolStatus::Created,
                None,
            );
            true
        }
        _ => false,
    }
}

fn handle_direct_notification(
    notification: ServerNotification,
    state: &mut LogState,
    msg_store: &Arc<MsgStore>,
    entry_index: &EntryIndexProvider,
) -> bool {
    match notification {
        ServerNotification::ThreadStarted(notification) => {
            msg_store.push_session_id(notification.thread.id);
            true
        }
        ServerNotification::AgentMessageDelta(notification) => {
            state.thinking = None;
            let (entry, index, is_new) = state.assistant_message_append(notification.delta);
            upsert_normalized_entry(msg_store, index, entry, is_new);
            true
        }
        ServerNotification::ReasoningSummaryTextDelta(notification) => {
            state.assistant = None;
            let (entry, index, is_new) = state.thinking_append(notification.delta);
            upsert_normalized_entry(msg_store, index, entry, is_new);
            true
        }
        ServerNotification::CommandExecutionOutputDelta(
            CommandExecutionOutputDeltaNotification { item_id, delta, .. },
        ) => {
            if let Some(command_state) = state.commands.get_mut(&item_id) {
                command_state.stdout.push_str(&delta);
                if let Some(index) = command_state.index {
                    replace_normalized_entry(msg_store, index, command_state.to_normalized_entry());
                }
            }
            true
        }
        ServerNotification::ItemStarted(notification) => {
            handle_direct_item_started(notification, state, msg_store, entry_index);
            true
        }
        ServerNotification::ItemCompleted(notification) => {
            handle_direct_item_completed(notification, state, msg_store, entry_index);
            true
        }
        _ => false,
    }
}

fn format_todo_status(status: &StepStatus) -> String {
    match status {
        StepStatus::Pending => "pending",
        StepStatus::InProgress => "in_progress",
        StepStatus::Completed => "completed",
    }
    .to_string()
}

pub fn normalize_logs(msg_store: Arc<MsgStore>, worktree_path: &Path) {
    let entry_index = EntryIndexProvider::start_from(&msg_store);
    normalize_stderr_logs(msg_store.clone(), entry_index.clone());

    let worktree_path_str = worktree_path.to_string_lossy().to_string();
    tokio::spawn(async move {
        let mut state = LogState::new(entry_index.clone());
        let mut stdout_lines = msg_store.stdout_lines_stream();

        while let Some(Ok(line)) = stdout_lines.next().await {
            if let Ok(error) = serde_json::from_str::<Error>(&line) {
                add_normalized_entry(&msg_store, &entry_index, error.to_normalized_entry());
                continue;
            }

            if let Ok(approval) = serde_json::from_str::<Approval>(&line) {
                if let Some(entry) = approval.to_normalized_entry_opt() {
                    add_normalized_entry(&msg_store, &entry_index, entry);
                }
                continue;
            }

            if let Ok(response) = serde_json::from_str::<JSONRPCResponse>(&line) {
                handle_jsonrpc_response(response, &msg_store, &entry_index);
                continue;
            }

            if let Ok(server_notification) = serde_json::from_str::<ServerNotification>(&line)
                && handle_direct_notification(
                    server_notification,
                    &mut state,
                    &msg_store,
                    &entry_index,
                )
            {
                continue;
            }

            if let Some(server_request) = serde_json::from_str::<JSONRPCRequest>(&line)
                .ok()
                .and_then(|request| ServerRequest::try_from(request).ok())
            && handle_direct_request(server_request, &mut state, &msg_store, &entry_index)
            {
                continue;
            }

            let notification: JSONRPCNotification = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(_) => continue,
            };

            if notification.method == "thread/started" {
                if let Some(params) = notification
                    .params
                    .and_then(|p| serde_json::from_value::<ThreadStartedNotification>(p).ok())
                {
                    msg_store.push_session_id(params.thread.id);
                }
                continue;
            }

            if !notification.method.starts_with("codex/event") {
                continue;
            }

            let Some(params) = notification
                .params
                .and_then(|p| serde_json::from_value::<CodexNotificationParams>(p).ok())
            else {
                continue;
            };

            let event = params.msg;
            match event {
                EventMsg::SessionConfigured(payload) => {
                    msg_store.push_session_id(payload.session_id.to_string());
                    handle_model_params(
                        payload.model,
                        payload.reasoning_effort,
                        &msg_store,
                        &entry_index,
                    );
                }
                EventMsg::AgentMessageDelta(AgentMessageDeltaEvent { delta }) => {
                    state.thinking = None;
                    let (entry, index, is_new) = state.assistant_message_append(delta);
                    upsert_normalized_entry(&msg_store, index, entry, is_new);
                }
                EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent { delta }) => {
                    state.assistant = None;
                    let (entry, index, is_new) = state.thinking_append(delta);
                    upsert_normalized_entry(&msg_store, index, entry, is_new);
                }
                EventMsg::AgentMessage(AgentMessageEvent { message, .. }) => {
                    state.thinking = None;
                    let (entry, index, is_new) = state.assistant_message(message);
                    upsert_normalized_entry(&msg_store, index, entry, is_new);
                    state.assistant = None;
                }
                EventMsg::AgentReasoning(AgentReasoningEvent { text }) => {
                    state.assistant = None;
                    let (entry, index, is_new) = state.thinking(text);
                    upsert_normalized_entry(&msg_store, index, entry, is_new);
                    state.thinking = None;
                }
                EventMsg::AgentReasoningSectionBreak(AgentReasoningSectionBreakEvent {
                    item_id: _,
                    summary_index: _,
                }) => {
                    state.assistant = None;
                    state.thinking = None;
                }
                EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
                    call_id,
                    turn_id: _,
                    command,
                    cwd: _,
                    reason,
                    parsed_cmd: _,
                    proposed_execpolicy_amendment: _,
                    ..
                }) => {
                    state.assistant = None;
                    state.thinking = None;

                    let command_text = if command.is_empty() {
                        reason
                            .filter(|r| !r.is_empty())
                            .unwrap_or_else(|| "command execution".to_string())
                    } else {
                        command.join(" ")
                    };

                    let command_state = state.commands.entry(call_id.clone()).or_default();

                    if command_state.command.is_empty() {
                        command_state.command = command_text;
                    }
                    command_state.awaiting_approval = true;
                    if let Some(index) = command_state.index {
                        replace_normalized_entry(
                            &msg_store,
                            index,
                            command_state.to_normalized_entry(),
                        );
                    } else {
                        let index = add_normalized_entry(
                            &msg_store,
                            &entry_index,
                            command_state.to_normalized_entry(),
                        );
                        command_state.index = Some(index);
                    }
                }
                EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
                    call_id,
                    turn_id: _,
                    changes,
                    reason: _,
                    grant_root: _,
                }) => {
                    state.assistant = None;
                    state.thinking = None;

                    let normalized = normalize_file_changes(&worktree_path_str, &changes);
                    let patch_state = state.patches.entry(call_id.clone()).or_default();

                    for entry in patch_state.entries.drain(..) {
                        if let Some(index) = entry.index {
                            msg_store.push_patch(ConversationPatch::remove(index));
                        }
                    }

                    for (path, file_changes) in normalized {
                        let mut entry = PatchEntry {
                            index: None,
                            path,
                            changes: file_changes,
                            status: ToolStatus::Created,
                            awaiting_approval: true,
                            call_id: call_id.clone(),
                        };
                        let index = add_normalized_entry(
                            &msg_store,
                            &entry_index,
                            entry.to_normalized_entry(),
                        );
                        entry.index = Some(index);
                        patch_state.entries.push(entry);
                    }
                }
                EventMsg::ExecCommandBegin(ExecCommandBeginEvent {
                    call_id,
                    turn_id: _,
                    command,
                    cwd: _,
                    parsed_cmd: _,
                    source: _,
                    interaction_input: _,
                    process_id: _,
                }) => {
                    state.assistant = None;
                    state.thinking = None;
                    let command_text = command.join(" ");
                    if command_text.is_empty() {
                        continue;
                    }
                    state.commands.insert(
                        call_id.clone(),
                        CommandState {
                            index: None,
                            command: command_text,
                            stdout: String::new().into(),
                            stderr: String::new().into(),
                            formatted_output: None,
                            status: ToolStatus::Created,
                            exit_code: None,
                            awaiting_approval: false,
                            call_id: call_id.clone(),
                        },
                    );
                    let command_state = state.commands.get_mut(&call_id).unwrap();
                    let index = add_normalized_entry(
                        &msg_store,
                        &entry_index,
                        command_state.to_normalized_entry(),
                    );
                    command_state.index = Some(index)
                }
                EventMsg::ExecCommandOutputDelta(ExecCommandOutputDeltaEvent {
                    call_id,
                    stream,
                    chunk,
                }) => {
                    if let Some(command_state) = state.commands.get_mut(&call_id) {
                        let chunk = String::from_utf8_lossy(&chunk);
                        if chunk.is_empty() {
                            continue;
                        }
                        command_state.apply_output_delta(stream, &chunk);
                        continue;
                    }
                }
                EventMsg::ExecCommandEnd(ExecCommandEndEvent {
                    call_id,
                    turn_id: _,
                    command: _,
                    cwd: _,
                    parsed_cmd: _,
                    source: _,
                    interaction_input: _,
                    stdout: _,
                    stderr: _,
                    aggregated_output: _,
                    exit_code,
                    duration: _,
                    formatted_output,
                    process_id: _,
                    ..
                }) => {
                    if let Some(mut command_state) = state.commands.remove(&call_id) {
                        command_state.set_formatted_output(formatted_output);
                        command_state.exit_code = Some(exit_code);
                        command_state.awaiting_approval = false;
                        command_state.status = if exit_code == 0 {
                            ToolStatus::Success
                        } else {
                            ToolStatus::Failed
                        };
                        let Some(index) = command_state.index else {
                            tracing::error!("missing entry index for existing command state");
                            continue;
                        };
                        replace_normalized_entry(
                            &msg_store,
                            index,
                            command_state.to_normalized_entry(),
                        );
                    }
                }
                EventMsg::BackgroundEvent(BackgroundEventEvent { message }) => {
                    add_normalized_entry(
                        &msg_store,
                        &entry_index,
                        NormalizedEntry {
                            timestamp: None,
                            entry_type: NormalizedEntryType::SystemMessage,
                            content: format!("Background event: {message}"),
                            metadata: None,
                        },
                    );
                }
                EventMsg::StreamError(StreamErrorEvent {
                    message,
                    codex_error_info,
                    ..
                }) => {
                    add_normalized_entry(
                        &msg_store,
                        &entry_index,
                        NormalizedEntry {
                            timestamp: None,
                            entry_type: NormalizedEntryType::ErrorMessage {
                                error_type: NormalizedEntryError::Other,
                            },
                            content: format!("Stream error: {message} {codex_error_info:?}"),
                            metadata: None,
                        },
                    );
                }
                EventMsg::McpToolCallBegin(McpToolCallBeginEvent {
                    call_id,
                    invocation,
                    ..
                }) => {
                    state.assistant = None;
                    state.thinking = None;
                    state.mcp_tools.insert(
                        call_id.clone(),
                        McpToolState {
                            index: None,
                            invocation,
                            result: None,
                            status: ToolStatus::Created,
                        },
                    );
                    let mcp_tool_state = state.mcp_tools.get_mut(&call_id).unwrap();
                    let index = add_normalized_entry(
                        &msg_store,
                        &entry_index,
                        mcp_tool_state.to_normalized_entry(),
                    );
                    mcp_tool_state.index = Some(index);
                }
                EventMsg::McpToolCallEnd(McpToolCallEndEvent {
                    call_id, result, ..
                }) => {
                    if let Some(mut mcp_tool_state) = state.mcp_tools.remove(&call_id) {
                        match result {
                            Ok(value) => {
                                mcp_tool_state.status = if value.is_error.unwrap_or(false) {
                                    ToolStatus::Failed
                                } else {
                                    ToolStatus::Success
                                };
                                mcp_tool_state.result = Some(ToolResult {
                                    r#type: ToolResultValueType::Json,
                                    value: value.structured_content.unwrap_or_else(|| {
                                        serde_json::to_value(value.content).unwrap_or_default()
                                    }),
                                });
                            }
                            Err(err) => {
                                mcp_tool_state.status = ToolStatus::Failed;
                                mcp_tool_state.result = Some(ToolResult {
                                    r#type: ToolResultValueType::Markdown,
                                    value: Value::String(err),
                                });
                            }
                        };
                        let Some(index) = mcp_tool_state.index else {
                            tracing::error!("missing entry index for existing mcp tool state");
                            continue;
                        };
                        replace_normalized_entry(
                            &msg_store,
                            index,
                            mcp_tool_state.to_normalized_entry(),
                        );
                    }
                }
                EventMsg::PatchApplyBegin(PatchApplyBeginEvent {
                    call_id, changes, ..
                }) => {
                    state.assistant = None;
                    state.thinking = None;
                    let normalized = normalize_file_changes(&worktree_path_str, &changes);
                    if let Some(patch_state) = state.patches.get_mut(&call_id) {
                        let mut iter = normalized.into_iter();
                        for entry in &mut patch_state.entries {
                            if let Some((path, file_changes)) = iter.next() {
                                entry.path = path;
                                entry.changes = file_changes;
                            }
                            entry.status = ToolStatus::Created;
                            entry.awaiting_approval = false;
                            if let Some(index) = entry.index {
                                replace_normalized_entry(
                                    &msg_store,
                                    index,
                                    entry.to_normalized_entry(),
                                );
                            } else {
                                let index = add_normalized_entry(
                                    &msg_store,
                                    &entry_index,
                                    entry.to_normalized_entry(),
                                );
                                entry.index = Some(index);
                            }
                        }
                        for (path, file_changes) in iter {
                            let mut entry = PatchEntry {
                                index: None,
                                path,
                                changes: file_changes,
                                status: ToolStatus::Created,
                                awaiting_approval: false,
                                call_id: call_id.clone(),
                            };
                            let index = add_normalized_entry(
                                &msg_store,
                                &entry_index,
                                entry.to_normalized_entry(),
                            );
                            entry.index = Some(index);
                            patch_state.entries.push(entry);
                        }
                    } else {
                        let mut patch_state = PatchState::default();
                        for (path, file_changes) in normalized {
                            patch_state.entries.push(PatchEntry {
                                index: None,
                                path,
                                changes: file_changes,
                                status: ToolStatus::Created,
                                awaiting_approval: false,
                                call_id: call_id.clone(),
                            });
                            let patch_entry = patch_state.entries.last_mut().unwrap();
                            let index = add_normalized_entry(
                                &msg_store,
                                &entry_index,
                                patch_entry.to_normalized_entry(),
                            );
                            patch_entry.index = Some(index);
                        }
                        state.patches.insert(call_id, patch_state);
                    }
                }
                EventMsg::PatchApplyEnd(PatchApplyEndEvent {
                    call_id,
                    stdout: _,
                    stderr: _,
                    success,
                    ..
                }) => {
                    if let Some(patch_state) = state.patches.remove(&call_id) {
                        let status = if success {
                            ToolStatus::Success
                        } else {
                            ToolStatus::Failed
                        };
                        for mut entry in patch_state.entries {
                            entry.status = status.clone();
                            let Some(index) = entry.index else {
                                tracing::error!("missing entry index for existing patch entry");
                                continue;
                            };
                            replace_normalized_entry(
                                &msg_store,
                                index,
                                entry.to_normalized_entry(),
                            );
                        }
                    }
                }
                EventMsg::WebSearchBegin(WebSearchBeginEvent { call_id }) => {
                    state.assistant = None;
                    state.thinking = None;
                    state
                        .web_searches
                        .insert(call_id.clone(), WebSearchState::new());
                    let web_search_state = state.web_searches.get_mut(&call_id).unwrap();
                    let normalized_entry = web_search_state.to_normalized_entry();
                    let index = add_normalized_entry(&msg_store, &entry_index, normalized_entry);
                    web_search_state.index = Some(index);
                }
                EventMsg::WebSearchEnd(WebSearchEndEvent {
                    call_id, query, ..
                }) => {
                    state.assistant = None;
                    state.thinking = None;
                    if let Some(mut entry) = state.web_searches.remove(&call_id) {
                        entry.status = ToolStatus::Success;
                        entry.query = Some(query.clone());
                        let normalized_entry = entry.to_normalized_entry();
                        let Some(index) = entry.index else {
                            tracing::error!("missing entry index for existing websearch entry");
                            continue;
                        };
                        replace_normalized_entry(&msg_store, index, normalized_entry);
                    }
                }
                EventMsg::ViewImageToolCall(ViewImageToolCallEvent { call_id: _, path }) => {
                    state.assistant = None;
                    state.thinking = None;
                    let path_str = path.to_string_lossy().to_string();
                    let relative_path = make_path_relative(&path_str, &worktree_path_str);
                    add_normalized_entry(
                        &msg_store,
                        &entry_index,
                        NormalizedEntry {
                            timestamp: None,
                            entry_type: NormalizedEntryType::ToolUse {
                                tool_name: "view_image".to_string(),
                                action_type: ActionType::FileRead {
                                    path: relative_path.clone(),
                                },
                                status: ToolStatus::Success,
                            },
                            content: relative_path.to_string(),
                            metadata: None,
                        },
                    );
                }
                EventMsg::PlanUpdate(UpdatePlanArgs { plan, explanation }) => {
                    let todos: Vec<TodoItem> = plan
                        .iter()
                        .map(|item| TodoItem {
                            content: item.step.clone(),
                            status: format_todo_status(&item.status),
                            priority: None,
                        })
                        .collect();
                    let explanation = explanation
                        .as_ref()
                        .map(|text| text.trim())
                        .filter(|text| !text.is_empty())
                        .map(|text| text.to_string());
                    let content = explanation.clone().unwrap_or_else(|| {
                        if todos.is_empty() {
                            "Plan updated".to_string()
                        } else {
                            format!("Plan updated ({} steps)", todos.len())
                        }
                    });

                    add_normalized_entry(
                        &msg_store,
                        &entry_index,
                        NormalizedEntry {
                            timestamp: None,
                            entry_type: NormalizedEntryType::ToolUse {
                                tool_name: "plan".to_string(),
                                action_type: ActionType::TodoManagement {
                                    todos,
                                    operation: "update".to_string(),
                                },
                                status: ToolStatus::Success,
                            },
                            content,
                            metadata: None,
                        },
                    );
                }
                EventMsg::Warning(WarningEvent { message }) => {
                    add_normalized_entry(
                        &msg_store,
                        &entry_index,
                        NormalizedEntry {
                            timestamp: None,
                            entry_type: NormalizedEntryType::ErrorMessage {
                                error_type: NormalizedEntryError::Other,
                            },
                            content: message,
                            metadata: None,
                        },
                    );
                }
                EventMsg::Error(ErrorEvent {
                    message,
                    codex_error_info,
                }) => {
                    add_normalized_entry(
                        &msg_store,
                        &entry_index,
                        NormalizedEntry {
                            timestamp: None,
                            entry_type: NormalizedEntryType::ErrorMessage {
                                error_type: NormalizedEntryError::Other,
                            },
                            content: format!("Error: {message} {codex_error_info:?}"),
                            metadata: None,
                        },
                    );
                }
                EventMsg::TokenCount(payload) => {
                    if let Some(info) = payload.info {
                        state.token_usage_info = Some(info);
                    }
                }
                EventMsg::ContextCompacted(..) => {
                    add_normalized_entry(
                        &msg_store,
                        &entry_index,
                        NormalizedEntry {
                            timestamp: None,
                            entry_type: NormalizedEntryType::SystemMessage,
                            content: "Context compacted".to_string(),
                            metadata: None,
                        },
                    );
                }
                EventMsg::AgentReasoningRawContent(..)
                | EventMsg::AgentReasoningRawContentDelta(..)
                | EventMsg::TurnStarted(..)
                | EventMsg::UserMessage(..)
                | EventMsg::TurnDiff(..)
                | EventMsg::GetHistoryEntryResponse(..)
                | EventMsg::McpListToolsResponse(..)
                | EventMsg::McpStartupComplete(..)
                | EventMsg::McpStartupUpdate(..)
                | EventMsg::DeprecationNotice(..)
                | EventMsg::UndoCompleted(..)
                | EventMsg::UndoStarted(..)
                | EventMsg::RawResponseItem(..)
                | EventMsg::ItemStarted(..)
                | EventMsg::ItemCompleted(..)
                | EventMsg::AgentMessageContentDelta(..)
                | EventMsg::ReasoningContentDelta(..)
                | EventMsg::ReasoningRawContentDelta(..)
                | EventMsg::TurnAborted(..)
                | EventMsg::ShutdownComplete
                | EventMsg::EnteredReviewMode(..)
                | EventMsg::ExitedReviewMode(..)
                | EventMsg::TerminalInteraction(..)
                | EventMsg::ElicitationRequest(..)
                | EventMsg::TurnComplete(..) => {}
                _ => {}
            }
        }
    });
}

fn handle_jsonrpc_response(
    response: JSONRPCResponse,
    msg_store: &Arc<MsgStore>,
    entry_index: &EntryIndexProvider,
) {
    if let Ok(response) = serde_json::from_value::<ThreadStartResponse>(response.result.clone()) {
        msg_store.push_session_id(response.thread.id);
        handle_model_params(
            response.model,
            response.reasoning_effort,
            msg_store,
            entry_index,
        );
        return;
    }

    if let Ok(response) = serde_json::from_value::<ThreadForkResponse>(response.result.clone()) {
        msg_store.push_session_id(response.thread.id);
        handle_model_params(
            response.model,
            response.reasoning_effort,
            msg_store,
            entry_index,
        );
    }
}

fn handle_model_params(
    model: String,
    reasoning_effort: Option<ReasoningEffort>,
    msg_store: &Arc<MsgStore>,
    entry_index: &EntryIndexProvider,
) {
    let mut params = vec![];
    params.push(format!("model: {model}"));
    if let Some(reasoning_effort) = reasoning_effort {
        params.push(format!("reasoning effort: {reasoning_effort}"));
    }

    add_normalized_entry(
        msg_store,
        entry_index,
        NormalizedEntry {
            timestamp: None,
            entry_type: NormalizedEntryType::SystemMessage,
            content: params.join("  ").to_string(),
            metadata: None,
        },
    );
}

fn build_command_output(stdout: Option<String>, stderr: Option<String>) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(out) = stdout {
        sections.push(out);
    }
    if let Some(err) = stderr {
        sections.push(err);
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Error {
    LaunchError { error: String },
    AuthRequired { error: String },
}

impl Error {
    pub fn launch_error(error: String) -> Self {
        Self::LaunchError { error }
    }
    pub fn auth_required(error: String) -> Self {
        Self::AuthRequired { error }
    }

    pub fn raw(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

impl ToNormalizedEntry for Error {
    fn to_normalized_entry(&self) -> NormalizedEntry {
        match self {
            Error::LaunchError { error } => NormalizedEntry {
                timestamp: None,
                entry_type: NormalizedEntryType::ErrorMessage {
                    error_type: NormalizedEntryError::Other,
                },
                content: error.clone(),
                metadata: None,
            },
            Error::AuthRequired { error } => NormalizedEntry {
                timestamp: None,
                entry_type: NormalizedEntryType::ErrorMessage {
                    error_type: NormalizedEntryError::SetupRequired,
                },
                content: error.clone(),
                metadata: None,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Approval {
    ApprovalResponse {
        call_id: String,
        tool_name: String,
        approval_status: ApprovalStatus,
    },
}

impl Approval {
    pub fn approval_response(
        call_id: String,
        tool_name: String,
        approval_status: ApprovalStatus,
    ) -> Self {
        Self::ApprovalResponse {
            call_id,
            tool_name,
            approval_status,
        }
    }

    pub fn raw(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn display_tool_name(&self) -> String {
        let Self::ApprovalResponse { tool_name, .. } = self;
        match tool_name.as_str() {
            "codex.exec_command" => "Exec Command".to_string(),
            "codex.apply_patch" => "Edit".to_string(),
            other => other.to_string(),
        }
    }
}

impl ToNormalizedEntryOpt for Approval {
    fn to_normalized_entry_opt(&self) -> Option<NormalizedEntry> {
        let Self::ApprovalResponse {
            call_id: _,
            tool_name: _,
            approval_status,
        } = self;
        let tool_name = self.display_tool_name();

        match approval_status {
            ApprovalStatus::Pending => None,
            ApprovalStatus::Approved => None,
            ApprovalStatus::Denied { reason } => Some(NormalizedEntry {
                timestamp: None,
                entry_type: NormalizedEntryType::UserFeedback {
                    denied_tool: tool_name.clone(),
                },
                content: reason
                    .clone()
                    .unwrap_or_else(|| "User denied this tool use request".to_string())
                    .trim()
                    .to_string(),
                metadata: None,
            }),
            ApprovalStatus::TimedOut => Some(NormalizedEntry {
                timestamp: None,
                entry_type: NormalizedEntryType::ErrorMessage {
                    error_type: NormalizedEntryError::Other,
                },
                content: format!("Approval timed out for tool {tool_name}"),
                metadata: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use codex_app_server_protocol::JSONRPCNotification;
    use codex_protocol::protocol::{
        EventMsg, ExecCommandBeginEvent, ExecCommandEndEvent, ExecCommandOutputDeltaEvent,
        ExecCommandSource, ExecCommandStatus,
    };
    use crate::logs::utils::patch::extract_normalized_entry_from_patch;
    use serde_json::json;
    use workspace_utils::{log_msg::LogMsg, msg_store::MsgStore};

    use super::{
        ActionType, BoundedOutput, CommandState, ExecOutputStream, NormalizedEntryType,
        ToNormalizedEntry, ToolStatus,
    };

    fn command_output(state: &CommandState) -> String {
        let entry = state.to_normalized_entry();
        match entry.entry_type {
            NormalizedEntryType::ToolUse {
                action_type: ActionType::CommandRun {
                    result: Some(result), ..
                },
                ..
            } => result.output.unwrap_or_default(),
            other => panic!("unexpected entry type: {other:?}"),
        }
    }

    #[test]
    fn command_output_is_bounded_while_streaming() {
        let mut state = CommandState {
            command: "npm run dev".to_string(),
            status: ToolStatus::Created,
            ..Default::default()
        };

        state.push_stdout_chunk("a".repeat(80_000));
        state.push_stdout_chunk("b".repeat(80_000));

        let output = command_output(&state);

        assert!(output.len() < 100_000);
        assert!(output.contains("[stdout truncated"));
        assert!(output.contains(&"b".repeat(4_096)));
        assert!(!output.contains(&"a".repeat(20_000)));
    }

    #[test]
    fn final_formatted_output_is_bounded() {
        let mut state = CommandState {
            command: "npm run build".to_string(),
            status: ToolStatus::Success,
            ..Default::default()
        };

        state.formatted_output = Some(format!(
            "stdout:\n{}\n\nstderr:\n{}",
            "x".repeat(120_000),
            "y".repeat(40_000)
        ).into());

        let output = command_output(&state);

        assert!(output.len() < 100_000);
        assert!(output.contains("[command output truncated"));
        assert!(output.contains(&"y".repeat(4_096)));
    }

    #[test]
    fn compact_mode_accumulates_output_without_requesting_live_replace() {
        let mut state = CommandState {
            command: "npm run dev".to_string(),
            status: ToolStatus::Created,
            ..Default::default()
        };

        state.apply_output_delta(ExecOutputStream::Stdout, "x".repeat(32_000));
        assert!(matches!(state.stdout, BoundedOutput { .. }));
        assert!(command_output(&state).contains(&"x".repeat(4_096)));
    }

    #[test]
    fn normal_mode_command_output_deltas_do_not_request_live_replace() {
        let mut state = CommandState {
            command: "npm run dev".to_string(),
            status: ToolStatus::Created,
            ..Default::default()
        };

        state.apply_output_delta(ExecOutputStream::Stdout, "streamed-output");
        assert!(command_output(&state).contains("streamed-output"));
    }

    fn notification_line(msg: EventMsg) -> String {
        format!(
            "{}\n",
            serde_json::to_string(&JSONRPCNotification {
            method: "codex/event/test".to_string(),
            params: Some(serde_json::json!({ "msg": msg })),
        })
        .unwrap()
        )
    }

    fn direct_notification_line(value: serde_json::Value) -> String {
        format!("{}\n", serde_json::to_string(&value).unwrap())
    }

    fn direct_request_line(value: serde_json::Value) -> String {
        format!("{}\n", serde_json::to_string(&value).unwrap())
    }

    #[tokio::test]
    async fn command_output_deltas_only_emit_begin_and_final_summary_patches() {
        let msg_store = Arc::new(MsgStore::new());

        msg_store.push_stdout(notification_line(EventMsg::ExecCommandBegin(
            ExecCommandBeginEvent {
                call_id: "call-1".to_string(),
                process_id: None,
                turn_id: "turn-1".to_string(),
                command: vec!["npm".to_string(), "run".to_string(), "dev".to_string()],
                cwd: "/tmp/worktree".try_into().unwrap(),
                parsed_cmd: Vec::new(),
                source: ExecCommandSource::Agent,
                interaction_input: None,
            },
        )));

        for chunk in ["first", "second", "third"] {
            msg_store.push_stdout(notification_line(EventMsg::ExecCommandOutputDelta(
                ExecCommandOutputDeltaEvent {
                    call_id: "call-1".to_string(),
                    stream: ExecOutputStream::Stdout,
                    chunk: chunk.as_bytes().to_vec(),
                },
            )));
        }

        msg_store.push_stdout(notification_line(EventMsg::ExecCommandEnd(
            ExecCommandEndEvent {
                call_id: "call-1".to_string(),
                process_id: None,
                turn_id: "turn-1".to_string(),
                command: vec!["npm".to_string(), "run".to_string(), "dev".to_string()],
                cwd: "/tmp/worktree".try_into().unwrap(),
                parsed_cmd: Vec::new(),
                source: ExecCommandSource::Agent,
                interaction_input: None,
                stdout: "firstsecondthird".to_string(),
                stderr: String::new(),
                aggregated_output: "firstsecondthird".to_string(),
                exit_code: 0,
                duration: Duration::from_secs(1),
                formatted_output: "stdout:\nfirstsecondthird".to_string(),
                status: ExecCommandStatus::Completed,
            },
        )));
        msg_store.push_finished();

        let mut rx = msg_store.get_receiver();
        super::normalize_logs(msg_store.clone(), PathBuf::from("/tmp/worktree").as_path());
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let mut live_patches = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            if let LogMsg::JsonPatch(patch) = msg {
                live_patches.push(patch);
            }
        }

        let patches = msg_store
            .get_history()
            .into_iter()
            .filter_map(|msg| match msg {
                LogMsg::JsonPatch(patch) => Some(patch),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(live_patches.len(), 2);
        assert_eq!(patches.len(), 1);

        let first = serde_json::to_value(&live_patches[0]).unwrap();
        let second = serde_json::to_value(&live_patches[1]).unwrap();
        assert_eq!(
            first[0].get("op").and_then(|value| value.as_str()),
            Some("add")
        );
        assert_eq!(
            second[0].get("op").and_then(|value| value.as_str()),
            Some("replace")
        );

        let output = patches
            .last()
            .and_then(extract_normalized_entry_from_patch)
            .map(|(_, entry)| match entry.entry_type {
                NormalizedEntryType::ToolUse {
                    action_type: ActionType::CommandRun {
                        result: Some(result), ..
                    },
                    ..
                } => result.output.unwrap_or_default(),
                other => panic!("unexpected entry type: {other:?}"),
            })
            .unwrap();

        assert!(output.contains("firstsecondthird"));
    }

    #[tokio::test]
    async fn direct_item_completed_agent_message_emits_assistant_entry() {
        let msg_store = Arc::new(MsgStore::new());
        msg_store.push_stdout(direct_notification_line(json!({
            "jsonrpc": "2.0",
            "method": "item/completed",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "item": {
                    "type": "agentMessage",
                    "id": "msg-1",
                    "text": "/projects/workspace-linkease-ubuntu/linkease-github/vibe-kanban"
                }
            }
        })));
        msg_store.push_finished();

        super::normalize_logs(msg_store.clone(), PathBuf::from("/tmp/worktree").as_path());
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let output = msg_store
            .get_history()
            .into_iter()
            .filter_map(|msg| match msg {
                LogMsg::JsonPatch(patch) => extract_normalized_entry_from_patch(&patch).map(|(_, entry)| entry),
                _ => None,
            })
            .find_map(|entry| match entry.entry_type {
                NormalizedEntryType::AssistantMessage => Some(entry.content),
                _ => None,
            })
            .unwrap();

        assert_eq!(
            output,
            "/projects/workspace-linkease-ubuntu/linkease-github/vibe-kanban"
        );
    }

    #[tokio::test]
    async fn direct_request_user_input_emits_question_tool_entry() {
        let msg_store = Arc::new(MsgStore::new());
        msg_store.push_stdout(direct_request_line(json!({
            "jsonrpc": "2.0",
            "id": "request-1",
            "method": "item/tool/requestUserInput",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "itemId": "question-1",
                "questions": [{
                    "id": "q1",
                    "header": "Branch",
                    "question": "Choose a branch",
                    "options": [],
                    "isOther": false,
                    "isSecret": false
                }]
            }
        })));
        msg_store.push_finished();

        super::normalize_logs(msg_store.clone(), PathBuf::from("/tmp/worktree").as_path());
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let entry = msg_store
            .get_history()
            .into_iter()
            .filter_map(|msg| match msg {
                LogMsg::JsonPatch(patch) => {
                    extract_normalized_entry_from_patch(&patch).map(|(_, entry)| entry)
                }
                _ => None,
            })
            .find(|entry| entry.content.contains("Choose a branch"))
            .unwrap();

        match entry.entry_type {
            NormalizedEntryType::ToolUse { tool_name, .. } => assert_eq!(tool_name, "question"),
            other => panic!("unexpected entry type: {other:?}"),
        }
    }

    #[tokio::test]
    async fn direct_dynamic_tool_completion_emits_tool_result() {
        let msg_store = Arc::new(MsgStore::new());
        msg_store.push_stdout(direct_notification_line(json!({
            "jsonrpc": "2.0",
            "method": "item/completed",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "item": {
                    "type": "dynamicToolCall",
                    "id": "dynamic-1",
                    "tool": "custom.lookup",
                    "arguments": { "query": "abc" },
                    "status": "completed",
                    "success": true,
                    "contentItems": [
                        {
                            "type": "inputText",
                            "text": "ok"
                        }
                    ]
                }
            }
        })));
        msg_store.push_finished();

        super::normalize_logs(msg_store.clone(), PathBuf::from("/tmp/worktree").as_path());
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let entry = msg_store
            .get_history()
            .into_iter()
            .filter_map(|msg| match msg {
                LogMsg::JsonPatch(patch) => {
                    extract_normalized_entry_from_patch(&patch).map(|(_, entry)| entry)
                }
                _ => None,
            })
            .find(|entry| entry.content == "custom.lookup")
            .unwrap();

        match entry.entry_type {
            NormalizedEntryType::ToolUse {
                action_type: ActionType::Tool { result: Some(result), .. },
                ..
            } => assert_eq!(
                result.value,
                json!([{ "type": "inputText", "text": "ok" }])
            ),
            other => panic!("unexpected entry type: {other:?}"),
        }
    }
}
