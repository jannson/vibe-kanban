use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, SqlitePool};
use ts_rs::TS;
use utils::log_msg::LogMsg;
use uuid::Uuid;

const EXECUTION_LOG_DELETE_BATCH_SIZE: i64 = 10_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct ExecutionLogCleanupStats {
    pub deleted_rows: i64,
    pub deleted_bytes: i64,
    pub dropped_rows: i64,
    pub dropped_bytes: i64,
    pub retained_rows: i64,
    pub retained_bytes: i64,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct ExecutionProcessLogs {
    pub execution_id: Uuid,
    pub logs: String, // JSONL format
    pub byte_size: i64,
    pub inserted_at: DateTime<Utc>,
}

impl ExecutionProcessLogs {
    async fn delete_dropped_logs_in_batches(pool: &SqlitePool) -> Result<(), sqlx::Error> {
        loop {
            let deleted = sqlx::query(
                r#"DELETE FROM execution_process_logs
                   WHERE rowid IN (
                       SELECT epl.rowid
                         FROM execution_process_logs epl
                         JOIN execution_processes ep ON ep.id = epl.execution_id
                        WHERE ep.dropped = TRUE
                        LIMIT ?
                   )"#,
            )
            .bind(EXECUTION_LOG_DELETE_BATCH_SIZE)
            .execute(pool)
            .await?
            .rows_affected();

            if deleted == 0 {
                break;
            }
        }

        Ok(())
    }

    async fn delete_retained_logs_in_batches(
        pool: &SqlitePool,
        retention_days: u32,
    ) -> Result<(), sqlx::Error> {
        loop {
            let deleted = sqlx::query(
                r#"DELETE FROM execution_process_logs
                   WHERE rowid IN (
                       SELECT epl.rowid
                         FROM execution_process_logs epl
                         JOIN execution_processes ep ON ep.id = epl.execution_id
                        WHERE ep.dropped = FALSE
                          AND ep.completed_at IS NOT NULL
                          AND datetime(ep.completed_at) < datetime('now', printf('-%d days', ?))
                        LIMIT ?
                   )"#,
            )
            .bind(retention_days as i64)
            .bind(EXECUTION_LOG_DELETE_BATCH_SIZE)
            .execute(pool)
            .await?
            .rows_affected();

            if deleted == 0 {
                break;
            }
        }

        Ok(())
    }

    async fn delete_dropped_logs_for_session_in_batches(
        pool: &SqlitePool,
        session_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        loop {
            let deleted = sqlx::query(
                r#"DELETE FROM execution_process_logs
                   WHERE rowid IN (
                       SELECT epl.rowid
                         FROM execution_process_logs epl
                         JOIN execution_processes ep ON ep.id = epl.execution_id
                        WHERE ep.session_id = $1
                          AND ep.dropped = TRUE
                        LIMIT ?
                   )"#,
            )
            .bind(session_id)
            .bind(EXECUTION_LOG_DELETE_BATCH_SIZE)
            .execute(pool)
            .await?
            .rows_affected();

            if deleted == 0 {
                break;
            }
        }

        Ok(())
    }

    /// Find logs by execution process ID
    pub async fn find_by_execution_id(
        pool: &SqlitePool,
        execution_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            ExecutionProcessLogs,
            r#"SELECT 
                execution_id as "execution_id!: Uuid",
                logs,
                byte_size,
                inserted_at as "inserted_at!: DateTime<Utc>"
               FROM execution_process_logs 
               WHERE execution_id = $1
               ORDER BY inserted_at ASC"#,
            execution_id
        )
        .fetch_all(pool)
        .await
    }

    /// Parse JSONL logs back into Vec<LogMsg>
    pub fn parse_logs(records: &[Self]) -> Result<Vec<LogMsg>, serde_json::Error> {
        let mut messages = Vec::new();
        for line in records.iter().flat_map(|record| record.logs.lines()) {
            if !line.trim().is_empty() {
                let msg: LogMsg = serde_json::from_str(line)?;
                messages.push(msg);
            }
        }
        Ok(messages)
    }

    /// Append a JSONL line to the logs for an execution process
    pub async fn append_log_line(
        pool: &SqlitePool,
        execution_id: Uuid,
        jsonl_line: &str,
    ) -> Result<(), sqlx::Error> {
        let byte_size = jsonl_line.len() as i64;
        sqlx::query!(
            r#"INSERT INTO execution_process_logs (execution_id, logs, byte_size, inserted_at)
               VALUES ($1, $2, $3, datetime('now', 'subsec'))"#,
            execution_id,
            jsonl_line,
            byte_size
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn cleanup_with_policy(
        pool: &SqlitePool,
        retention_days: u32,
        cleanup_dropped_logs: bool,
    ) -> Result<ExecutionLogCleanupStats, sqlx::Error> {
        let mut stats = ExecutionLogCleanupStats::default();

        if cleanup_dropped_logs {
            let row = sqlx::query(
                r#"SELECT
                        COUNT(*) as count,
                        COALESCE(SUM(epl.byte_size), 0) as bytes
                   FROM execution_process_logs epl
                   JOIN execution_processes ep ON ep.id = epl.execution_id
                  WHERE ep.dropped = TRUE"#,
            )
            .fetch_one(pool)
            .await?;
            let count = row.get::<i64, _>("count");
            let bytes = row.get::<i64, _>("bytes");

            if count > 0 {
                Self::delete_dropped_logs_in_batches(pool).await?;
            }

            stats.dropped_rows = count;
            stats.dropped_bytes = bytes;
        }

        if retention_days > 0 {
            let row = sqlx::query(
                r#"SELECT
                        COUNT(*) as count,
                        COALESCE(SUM(epl.byte_size), 0) as bytes
                   FROM execution_process_logs epl
                   JOIN execution_processes ep ON ep.id = epl.execution_id
                  WHERE ep.dropped = FALSE
                    AND ep.completed_at IS NOT NULL
                    AND datetime(ep.completed_at) < datetime('now', printf('-%d days', ?))"#,
            )
            .bind(retention_days as i64)
            .fetch_one(pool)
            .await?;
            let count = row.get::<i64, _>("count");
            let bytes = row.get::<i64, _>("bytes");

            if count > 0 {
                Self::delete_retained_logs_in_batches(pool, retention_days).await?;
            }

            stats.retained_rows = count;
            stats.retained_bytes = bytes;
        }

        stats.deleted_rows = stats.dropped_rows + stats.retained_rows;
        stats.deleted_bytes = stats.dropped_bytes + stats.retained_bytes;

        Ok(stats)
    }

    pub async fn cleanup_dropped_for_session(
        pool: &SqlitePool,
        session_id: Uuid,
    ) -> Result<ExecutionLogCleanupStats, sqlx::Error> {
        let row = sqlx::query(
            r#"SELECT
                    COUNT(*) as count,
                    COALESCE(SUM(epl.byte_size), 0) as bytes
               FROM execution_process_logs epl
               JOIN execution_processes ep ON ep.id = epl.execution_id
              WHERE ep.session_id = $1
                AND ep.dropped = TRUE"#,
        )
        .bind(session_id)
        .fetch_one(pool)
        .await?;
        let count = row.get::<i64, _>("count");
        let bytes = row.get::<i64, _>("bytes");

        if count > 0 {
            Self::delete_dropped_logs_for_session_in_batches(pool, session_id).await?;
        }

        Ok(ExecutionLogCleanupStats {
            deleted_rows: count,
            deleted_bytes: bytes,
            dropped_rows: count,
            dropped_bytes: bytes,
            retained_rows: 0,
            retained_bytes: 0,
        })
    }
}
