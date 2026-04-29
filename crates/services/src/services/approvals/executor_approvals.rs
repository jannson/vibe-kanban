use std::sync::Arc;

use async_trait::async_trait;
use db::{self, DBService};
use executors::approvals::{ExecutorApprovalError, ExecutorApprovalService};
use serde_json::Value;
use utils::approvals::{
    ApprovalOutcome, ApprovalRequest, ApprovalStatus, CreateApprovalRequest, QuestionStatus,
};
use uuid::Uuid;

use crate::services::{approvals::Approvals, notification::NotificationService};

pub struct ExecutorApprovalBridge {
    approvals: Approvals,
    db: DBService,
    notification_service: NotificationService,
    execution_process_id: Uuid,
}

impl ExecutorApprovalBridge {
    pub fn new(
        approvals: Approvals,
        db: DBService,
        notification_service: NotificationService,
        execution_process_id: Uuid,
    ) -> Arc<Self> {
        Arc::new(Self {
            approvals,
            db,
            notification_service,
            execution_process_id,
        })
    }
}

#[async_trait]
impl ExecutorApprovalService for ExecutorApprovalBridge {
    async fn request_tool_approval(
        &self,
        tool_name: &str,
        tool_input: Value,
        tool_call_id: &str,
    ) -> Result<ApprovalStatus, ExecutorApprovalError> {
        super::ensure_task_in_review(&self.db.pool, self.execution_process_id).await;

        let request = ApprovalRequest::from_create(
            CreateApprovalRequest {
                tool_name: tool_name.to_string(),
                tool_input,
                tool_call_id: tool_call_id.to_string(),
            },
            self.execution_process_id,
        );

        let (_, waiter) = self
            .approvals
            .create_with_waiter(request, false)
            .await
            .map_err(ExecutorApprovalError::request_failed)?;

        // Play notification sound when approval is needed
        self.notification_service
            .notify(
                "Approval Needed",
                &format!("Tool '{}' requires approval", tool_name),
            )
            .await;

        let status = waiter.clone().await;

        match status {
            ApprovalOutcome::Approved => Ok(ApprovalStatus::Approved),
            ApprovalOutcome::Denied { reason } => Ok(ApprovalStatus::Denied { reason }),
            ApprovalOutcome::TimedOut => Ok(ApprovalStatus::TimedOut),
            ApprovalOutcome::Answered { .. } => Err(ExecutorApprovalError::request_failed(
                "question response returned for tool approval",
            )),
        }
    }

    async fn request_question_answer(
        &self,
        tool_name: &str,
        question_count: usize,
        tool_call_id: &str,
    ) -> Result<QuestionStatus, ExecutorApprovalError> {
        super::ensure_task_in_review(&self.db.pool, self.execution_process_id).await;

        let request = ApprovalRequest::from_create(
            CreateApprovalRequest {
                tool_name: tool_name.to_string(),
                tool_input: serde_json::json!({
                    "question_count": question_count,
                }),
                tool_call_id: tool_call_id.to_string(),
            },
            self.execution_process_id,
        );

        let (_, waiter) = self
            .approvals
            .create_with_waiter(request, true)
            .await
            .map_err(ExecutorApprovalError::request_failed)?;

        self.notification_service
            .notify(
                "Question Asked",
                &format!("{} question(s) require answers", question_count),
            )
            .await;

        match waiter.clone().await {
            ApprovalOutcome::Answered { answers } => Ok(QuestionStatus::Answered { answers }),
            ApprovalOutcome::TimedOut => Ok(QuestionStatus::TimedOut),
            ApprovalOutcome::Approved | ApprovalOutcome::Denied { .. } => Err(
                ExecutorApprovalError::request_failed(
                    "tool approval returned for question request",
                ),
            ),
        }
    }
}
