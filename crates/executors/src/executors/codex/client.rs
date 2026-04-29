use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;
use codex_app_server_protocol::{
    ClientInfo, ClientNotification, ClientRequest, CommandExecutionApprovalDecision,
    CommandExecutionRequestApprovalResponse, DynamicToolCallOutputContentItem,
    DynamicToolCallResponse, FileChangeApprovalDecision, FileChangeRequestApprovalResponse,
    GetAccountParams, GetAccountResponse, InitializeCapabilities, InitializeParams,
    InitializeResponse, JSONRPCError, JSONRPCNotification, JSONRPCRequest, JSONRPCResponse,
    RequestId, ServerRequest, ThreadForkParams, ThreadForkResponse, ThreadStartParams,
    ThreadStartResponse, ToolRequestUserInputAnswer, ToolRequestUserInputQuestion,
    ToolRequestUserInputResponse, TurnCompletedNotification, TurnStartParams, TurnStartResponse,
    TurnStatus, UserInput,
};
use codex_protocol::config_types::{CollaborationMode, ModeKind, Settings};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt, BufWriter},
    sync::Mutex,
};
use workspace_utils::approvals::{ApprovalStatus, QuestionStatus};

use super::jsonrpc::{JsonRpcCallbacks, JsonRpcPeer};
use crate::{
    approvals::{ExecutorApprovalError, ExecutorApprovalService},
    executors::{
        ExecutorError,
        codex::normalize_logs::{Approval, Error},
    },
};

pub struct AppServerClient {
    rpc: OnceLock<JsonRpcPeer>,
    log_writer: LogWriter,
    approvals: Option<Arc<dyn ExecutorApprovalService>>,
    thread_id: Mutex<Option<String>>,
    pending_feedback: Mutex<VecDeque<String>>,
    auto_approve: bool,
    plan_mode: bool,
    resolved_model: OnceLock<String>,
}

impl AppServerClient {
    pub fn new(
        log_writer: LogWriter,
        approvals: Option<Arc<dyn ExecutorApprovalService>>,
        auto_approve: bool,
        plan_mode: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            rpc: OnceLock::new(),
            log_writer,
            approvals,
            auto_approve,
            plan_mode,
            thread_id: Mutex::new(None),
            pending_feedback: Mutex::new(VecDeque::new()),
            resolved_model: OnceLock::new(),
        })
    }

    pub fn connect(&self, peer: JsonRpcPeer) {
        let _ = self.rpc.set(peer);
    }

    pub fn set_resolved_model(&self, model: String) {
        let _ = self.resolved_model.set(model);
    }

    fn rpc(&self) -> &JsonRpcPeer {
        self.rpc.get().expect("Codex RPC peer not attached")
    }

    pub async fn initialize(&self) -> Result<(), ExecutorError> {
        let request = ClientRequest::Initialize {
            request_id: self.next_request_id(),
            params: InitializeParams {
                client_info: ClientInfo {
                    name: "vibe-codex-executor".to_string(),
                    title: None,
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                capabilities: Some(InitializeCapabilities {
                    experimental_api: true,
                    ..Default::default()
                }),
            },
        };

        self.send_request::<InitializeResponse>(request, "initialize")
            .await?;
        self.send_message(&ClientNotification::Initialized).await
    }

    pub async fn thread_start(
        &self,
        params: ThreadStartParams,
    ) -> Result<ThreadStartResponse, ExecutorError> {
        let request = ClientRequest::ThreadStart {
            request_id: self.next_request_id(),
            params,
        };
        self.send_request(request, "thread/start").await
    }

    pub async fn thread_fork(
        &self,
        params: ThreadForkParams,
    ) -> Result<ThreadForkResponse, ExecutorError> {
        let request = ClientRequest::ThreadFork {
            request_id: self.next_request_id(),
            params,
        };
        self.send_request(request, "thread/fork").await
    }

    pub async fn turn_start_with_mode(
        &self,
        thread_id: String,
        input: Vec<UserInput>,
        collaboration_mode: Option<CollaborationMode>,
    ) -> Result<TurnStartResponse, ExecutorError> {
        let request = ClientRequest::TurnStart {
            request_id: self.next_request_id(),
            params: TurnStartParams {
                thread_id,
                input,
                collaboration_mode,
                ..Default::default()
            },
        };
        self.send_request(request, "turn/start").await
    }

    fn collaboration_mode(&self, mode: ModeKind) -> Result<CollaborationMode, ExecutorError> {
        let model = self.resolved_model.get().cloned().ok_or_else(|| {
            ExecutorError::Io(io::Error::other(
                "resolved model not available for collaboration mode",
            ))
        })?;
        Ok(CollaborationMode {
            mode,
            settings: Settings {
                model,
                reasoning_effort: None,
                developer_instructions: None,
            },
        })
    }

    pub fn initial_collaboration_mode(&self) -> Result<CollaborationMode, ExecutorError> {
        if self.plan_mode {
            self.collaboration_mode(ModeKind::Plan)
        } else {
            self.collaboration_mode(ModeKind::Default)
        }
    }

    pub async fn get_account(&self) -> Result<GetAccountResponse, ExecutorError> {
        let request = ClientRequest::GetAccount {
            request_id: self.next_request_id(),
            params: GetAccountParams {
                refresh_token: false,
            },
        };
        self.send_request(request, "account/read").await
    }

    async fn handle_server_request(
        &self,
        peer: &JsonRpcPeer,
        request: ServerRequest,
    ) -> Result<(), ExecutorError> {
        match request {
            ServerRequest::CommandExecutionRequestApproval { request_id, params } => {
                let input = serde_json::to_value(&params)
                    .map_err(|err| ExecutorError::Io(io::Error::other(err.to_string())))?;
                let status = self
                    .request_tool_approval("bash", input, &params.item_id)
                    .await
                    .unwrap_or_else(|err| {
                        tracing::error!(
                            "failed to request command approval for item_id={}: {err}",
                            params.item_id
                        );
                        ApprovalStatus::Denied {
                            reason: Some("approval service error".to_string()),
                        }
                    });
                self.log_writer
                    .log_raw(
                        &Approval::approval_response(
                            params.item_id.clone(),
                            "codex.exec_command".to_string(),
                            status.clone(),
                        )
                        .raw(),
                    )
                    .await?;
                let (decision, feedback) = self.command_execution_decision(&status);
                let response = CommandExecutionRequestApprovalResponse { decision };
                send_server_response(peer, request_id, response).await?;
                if let Some(message) = feedback {
                    self.enqueue_feedback(message).await;
                }
                Ok(())
            }
            ServerRequest::FileChangeRequestApproval { request_id, params } => {
                let input = serde_json::to_value(&params)
                    .map_err(|err| ExecutorError::Io(io::Error::other(err.to_string())))?;
                let status = self
                    .request_tool_approval("edit", input, &params.item_id)
                    .await
                    .unwrap_or_else(|err| {
                        tracing::error!(
                            "failed to request file-change approval for item_id={}: {err}",
                            params.item_id
                        );
                        ApprovalStatus::Denied {
                            reason: Some("approval service error".to_string()),
                        }
                    });
                self.log_writer
                    .log_raw(
                        &Approval::approval_response(
                            params.item_id.clone(),
                            "codex.apply_patch".to_string(),
                            status.clone(),
                        )
                        .raw(),
                    )
                    .await?;
                let (decision, feedback) = self.file_change_decision(&status);
                let response = FileChangeRequestApprovalResponse { decision };
                send_server_response(peer, request_id, response).await?;
                if let Some(message) = feedback {
                    self.enqueue_feedback(message).await;
                }
                Ok(())
            }
            ServerRequest::ToolRequestUserInput { request_id, params } => {
                let status = match self
                    .request_question_answer(params.questions.len(), &params.item_id)
                    .await
                {
                    Ok(status) => status,
                    Err(err) => {
                        let message = question_request_error_message(&params.item_id, &err);
                        tracing::error!("{message}");
                        self.log_writer
                            .log_raw(&Error::launch_error(message).raw())
                            .await?;
                        QuestionStatus::TimedOut
                    }
                };
                let response = match &status {
                    QuestionStatus::Answered { answers } => {
                        let answers_map: HashMap<String, Vec<String>> = answers
                            .iter()
                            .map(|qa| (qa.question.clone(), qa.answer.clone()))
                            .collect();
                        answers_to_codex_format(&params.questions, &answers_map)
                    }
                    QuestionStatus::TimedOut => ToolRequestUserInputResponse {
                        answers: HashMap::new(),
                    },
                };
                send_server_response(peer, request_id, response).await
            }
            ServerRequest::DynamicToolCall { request_id, params } => {
                tracing::warn!(
                    "received unsupported dynamic tool call: tool={} call_id={}",
                    params.tool,
                    params.call_id
                );
                let response = DynamicToolCallResponse {
                    content_items: vec![DynamicToolCallOutputContentItem::InputText {
                        text: format!("Unsupported dynamic tool call: {}", params.tool),
                    }],
                    success: false,
                };
                send_server_response(peer, request_id, response).await
            }
            other => {
                tracing::debug!("unhandled v2 server request: {:?}", other);
                let response = JSONRPCResponse {
                    id: other.id().clone(),
                    result: Value::Null,
                };
                peer.send(&response).await
            }
        }
    }

    async fn request_tool_approval(
        &self,
        tool_name: &str,
        tool_input: Value,
        tool_call_id: &str,
    ) -> Result<ApprovalStatus, ExecutorError> {
        if self.auto_approve {
            return Ok(ApprovalStatus::Approved);
        }

        Ok(self
            .approvals
            .as_ref()
            .ok_or(ExecutorApprovalError::ServiceUnavailable)?
            .request_tool_approval(tool_name, tool_input, tool_call_id)
            .await?)
    }

    async fn request_question_answer(
        &self,
        question_count: usize,
        tool_call_id: &str,
    ) -> Result<QuestionStatus, ExecutorError> {
        Ok(self
            .approvals
            .as_ref()
            .ok_or(ExecutorApprovalError::ServiceUnavailable)?
            .request_question_answer("question", question_count, tool_call_id)
            .await?)
    }

    pub async fn register_session(&self, thread_id: &str) -> Result<(), ExecutorError> {
        {
            let mut guard = self.thread_id.lock().await;
            guard.replace(thread_id.to_string());
        }
        self.flush_pending_feedback().await;
        Ok(())
    }

    async fn send_message<M>(&self, message: &M) -> Result<(), ExecutorError>
    where
        M: Serialize + Sync,
    {
        self.rpc().send(message).await
    }

    async fn send_request<R>(&self, request: ClientRequest, label: &str) -> Result<R, ExecutorError>
    where
        R: DeserializeOwned + std::fmt::Debug,
    {
        let request_id = request_id(&request);
        self.rpc().request(request_id, &request, label).await
    }

    fn next_request_id(&self) -> RequestId {
        self.rpc().next_request_id()
    }

    fn command_execution_decision(
        &self,
        status: &ApprovalStatus,
    ) -> (CommandExecutionApprovalDecision, Option<String>) {
        if self.auto_approve {
            return (CommandExecutionApprovalDecision::AcceptForSession, None);
        }

        match status {
            ApprovalStatus::Approved => (CommandExecutionApprovalDecision::Accept, None),
            ApprovalStatus::Denied { reason } => {
                let feedback = normalize_feedback(reason);
                if feedback.is_some() {
                    (CommandExecutionApprovalDecision::Cancel, feedback)
                } else {
                    (CommandExecutionApprovalDecision::Decline, None)
                }
            }
            ApprovalStatus::TimedOut | ApprovalStatus::Pending => {
                (CommandExecutionApprovalDecision::Decline, None)
            }
        }
    }

    fn file_change_decision(
        &self,
        status: &ApprovalStatus,
    ) -> (FileChangeApprovalDecision, Option<String>) {
        if self.auto_approve {
            return (FileChangeApprovalDecision::AcceptForSession, None);
        }

        match status {
            ApprovalStatus::Approved => (FileChangeApprovalDecision::Accept, None),
            ApprovalStatus::Denied { reason } => {
                let feedback = normalize_feedback(reason);
                if feedback.is_some() {
                    (FileChangeApprovalDecision::Cancel, feedback)
                } else {
                    (FileChangeApprovalDecision::Decline, None)
                }
            }
            ApprovalStatus::TimedOut | ApprovalStatus::Pending => {
                (FileChangeApprovalDecision::Decline, None)
            }
        }
    }

    async fn enqueue_feedback(&self, message: String) {
        if message.trim().is_empty() {
            return;
        }
        let mut guard = self.pending_feedback.lock().await;
        guard.push_back(message);
    }

    async fn flush_pending_feedback(&self) {
        let messages: Vec<String> = {
            let mut guard = self.pending_feedback.lock().await;
            guard.drain(..).collect()
        };

        if messages.is_empty() {
            return;
        }

        let Some(thread_id) = self.thread_id.lock().await.clone() else {
            tracing::warn!(
                "pending Codex feedback but thread id unavailable; dropping {} messages",
                messages.len()
            );
            return;
        };

        for message in messages {
            let trimmed = message.trim();
            if trimmed.is_empty() {
                continue;
            }
            self.spawn_feedback_message(thread_id.clone(), trimmed.to_string());
        }
    }

    fn spawn_feedback_message(&self, thread_id: String, feedback: String) {
        let peer = self.rpc().clone();
        let request = ClientRequest::TurnStart {
            request_id: peer.next_request_id(),
            params: TurnStartParams {
                thread_id,
                input: vec![UserInput::Text {
                    text: format!("User feedback: {feedback}"),
                    text_elements: vec![],
                }],
                ..Default::default()
            },
        };
        tokio::spawn(async move {
            if let Err(err) = peer
                .request::<TurnStartResponse, _>(request_id(&request), &request, "turn/start")
                .await
            {
                tracing::error!("failed to send feedback follow-up message: {err}");
            }
        });
    }
}

#[async_trait]
impl JsonRpcCallbacks for AppServerClient {
    async fn on_request(
        &self,
        peer: &JsonRpcPeer,
        raw: &str,
        request: JSONRPCRequest,
    ) -> Result<(), ExecutorError> {
        self.log_writer.log_raw(raw).await?;
        match ServerRequest::try_from(request.clone()) {
            Ok(server_request) => self.handle_server_request(peer, server_request).await,
            Err(err) => {
                tracing::debug!("Unhandled server request `{}`: {err}", request.method);
                let response = JSONRPCResponse {
                    id: request.id,
                    result: Value::Null,
                };
                peer.send(&response).await
            }
        }
    }

    async fn on_response(
        &self,
        _peer: &JsonRpcPeer,
        raw: &str,
        _response: &JSONRPCResponse,
    ) -> Result<(), ExecutorError> {
        self.log_writer.log_raw(raw).await
    }

    async fn on_error(
        &self,
        _peer: &JsonRpcPeer,
        raw: &str,
        _error: &JSONRPCError,
    ) -> Result<(), ExecutorError> {
        self.log_writer.log_raw(raw).await
    }

    async fn on_notification(
        &self,
        _peer: &JsonRpcPeer,
        raw: &str,
        notification: JSONRPCNotification,
    ) -> Result<bool, ExecutorError> {
        self.log_writer.log_raw(raw).await?;

        if notification.method == "turn/completed" {
            if let Some(params) = notification.params
                && let Ok(completed) = serde_json::from_value::<TurnCompletedNotification>(params)
                && completed.turn.status == TurnStatus::Interrupted
            {
                self.flush_pending_feedback().await;
                return Ok(false);
            }

            return Ok(true);
        }

        Ok(false)
    }

    async fn on_non_json(&self, raw: &str) -> Result<(), ExecutorError> {
        self.log_writer.log_raw(raw).await?;
        Ok(())
    }
}

async fn send_server_response<T>(
    peer: &JsonRpcPeer,
    request_id: RequestId,
    response: T,
) -> Result<(), ExecutorError>
where
    T: Serialize,
{
    let payload = JSONRPCResponse {
        id: request_id,
        result: serde_json::to_value(response)
            .map_err(|err| ExecutorError::Io(io::Error::other(err.to_string())))?,
    };

    peer.send(&payload).await
}

fn normalize_feedback(reason: &Option<String>) -> Option<String> {
    reason
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn question_request_error_message(item_id: &str, err: &ExecutorError) -> String {
    format!("Question approval failed for item_id={item_id}: {err}")
}

fn request_id(request: &ClientRequest) -> RequestId {
    match request {
        ClientRequest::Initialize { request_id, .. }
        | ClientRequest::ThreadStart { request_id, .. }
        | ClientRequest::ThreadFork { request_id, .. }
        | ClientRequest::TurnStart { request_id, .. }
        | ClientRequest::GetAccount { request_id, .. } => request_id.clone(),
        _ => unreachable!("request_id called for unsupported request variant"),
    }
}

fn answers_to_codex_format(
    questions: &[ToolRequestUserInputQuestion],
    answers: &HashMap<String, Vec<String>>,
) -> ToolRequestUserInputResponse {
    let codex_answers = questions
        .iter()
        .filter_map(|question| {
            answers.get(&question.question).map(|answer_vec| {
                (
                    question.id.clone(),
                    ToolRequestUserInputAnswer {
                        answers: answer_vec.clone(),
                    },
                )
            })
        })
        .collect();

    ToolRequestUserInputResponse {
        answers: codex_answers,
    }
}

#[derive(Clone)]
pub struct LogWriter {
    writer: Arc<Mutex<BufWriter<Box<dyn AsyncWrite + Send + Unpin>>>>,
}

impl LogWriter {
    pub fn new(writer: impl AsyncWrite + Send + Unpin + 'static) -> Self {
        Self {
            writer: Arc::new(Mutex::new(BufWriter::new(Box::new(writer)))),
        }
    }

    pub async fn log_raw(&self, raw: &str) -> Result<(), ExecutorError> {
        let mut guard = self.writer.lock().await;
        guard
            .write_all(raw.as_bytes())
            .await
            .map_err(ExecutorError::Io)?;
        guard.write_all(b"\n").await.map_err(ExecutorError::Io)?;
        guard.flush().await.map_err(ExecutorError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use codex_app_server_protocol::ToolRequestUserInputQuestion;
    use codex_protocol::config_types::ModeKind;
    use tokio::io::sink;

    use crate::approvals::ExecutorApprovalError;

    use super::{
        AppServerClient, LogWriter, answers_to_codex_format, question_request_error_message,
    };

    #[test]
    fn answers_to_codex_format_maps_question_texts_to_ids() {
        let questions = vec![
            ToolRequestUserInputQuestion {
                id: "q1".to_string(),
                header: "One".to_string(),
                question: "Pick one".to_string(),
                options: None,
                is_other: false,
                is_secret: false,
            },
            ToolRequestUserInputQuestion {
                id: "q2".to_string(),
                header: "Two".to_string(),
                question: "Pick two".to_string(),
                options: None,
                is_other: false,
                is_secret: false,
            },
        ];
        let answers = HashMap::from([
            ("Pick one".to_string(), vec!["A".to_string()]),
            ("Pick two".to_string(), vec!["B".to_string(), "C".to_string()]),
        ]);

        let response = answers_to_codex_format(&questions, &answers);

        assert_eq!(response.answers["q1"].answers, vec!["A"]);
        assert_eq!(response.answers["q2"].answers, vec!["B", "C"]);
    }

    #[test]
    fn initial_collaboration_mode_uses_plan_when_enabled() {
        let client = AppServerClient::new(LogWriter::new(sink()), None, false, true);
        client.set_resolved_model("gpt-5.4".to_string());

        let collaboration_mode = client
            .initial_collaboration_mode()
            .expect("collaboration mode should resolve");

        assert_eq!(collaboration_mode.mode, ModeKind::Plan);
    }

    #[test]
    fn question_request_error_message_includes_context() {
        let message = question_request_error_message(
            "item-123",
            &ExecutorApprovalError::ServiceUnavailable.into(),
        );

        assert!(message.contains("item-123"));
        assert!(message.contains("approval service unavailable"));
    }
}
