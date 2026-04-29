use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use deployment::Deployment;
use services::services::approvals::ApprovalError;
use utils::{
    approvals::{ApprovalOutcome, ApprovalResponse},
    response::ApiResponse,
};

use crate::{DeploymentImpl, error::ApiError};

pub async fn respond_to_approval(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<String>,
    Json(request): Json<ApprovalResponse>,
) -> Result<Json<ApiResponse<ApprovalOutcome>>, ApiError> {
    let service = deployment.approvals();

    match service.respond(&deployment.db().pool, &id, request).await {
        Ok((status, context)) => {
            deployment
                .track_if_analytics_allowed(
                    "approval_responded",
                    serde_json::json!({
                        "approval_id": &id,
                        "status": format!("{:?}", status),
                        "tool_name": context.tool_name,
                        "execution_process_id": context.execution_process_id.to_string(),
                    }),
                )
                .await;

            Ok(Json(ApiResponse::success(status)))
        }
        Err(e) => {
            tracing::error!("Failed to respond to approval: {:?}", e);
            Err(map_approval_error(e))
        }
    }
}

fn map_approval_error(error: ApprovalError) -> ApiError {
    match error {
        ApprovalError::NotFound => ApiError::BadRequest("Approval request not found".to_string()),
        ApprovalError::AlreadyCompleted => {
            ApiError::Conflict("Approval request already completed".to_string())
        }
        ApprovalError::NoToolUseEntry | ApprovalError::NoExecutorSession(_) => {
            ApiError::BadRequest(error.to_string())
        }
        ApprovalError::Custom(_) => ApiError::BadRequest(error.to_string()),
        ApprovalError::Sqlx(err) => ApiError::Database(err),
    }
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new().route("/approvals/{id}/respond", post(respond_to_approval))
}

#[cfg(test)]
mod tests {
    use super::map_approval_error;
    use crate::error::ApiError;
    use services::services::approvals::ApprovalError;

    #[test]
    fn custom_approval_errors_become_bad_requests() {
        let error = map_approval_error(ApprovalError::Custom(anyhow::anyhow!(
            "question approval requires answered/timed_out outcome"
        )));

        assert!(matches!(error, ApiError::BadRequest(_)));
    }
}
