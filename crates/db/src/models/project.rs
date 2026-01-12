use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Sqlite, SqlitePool};
use thiserror::Error;
use ts_rs::TS;
use uuid::Uuid;

use super::project_repo::CreateProjectRepo;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("Project not found")]
    ProjectNotFound,
    #[error("Failed to create project: {0}")]
    CreateFailed(String),
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub workspace_root: Option<String>,
    pub dev_script: Option<String>,
    pub dev_script_working_dir: Option<String>,
    pub default_agent_working_dir: Option<String>,
    pub remote_project_id: Option<Uuid>,
    #[ts(type = "Date")]
    pub created_at: DateTime<Utc>,
    #[ts(type = "Date")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct CreateProject {
    pub name: String,
    pub workspace_root: Option<String>,
    pub repositories: Vec<CreateProjectRepo>,
}

#[derive(Debug, Deserialize, TS)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub dev_script: Option<String>,
    pub dev_script_working_dir: Option<String>,
    pub default_agent_working_dir: Option<String>,
}

#[derive(Debug, Serialize, TS)]
pub struct SearchResult {
    pub path: String,
    pub is_file: bool,
    pub match_type: SearchMatchType,
}

#[derive(Debug, Clone, Serialize, TS)]
pub enum SearchMatchType {
    FileName,
    DirectoryName,
    FullPath,
}

impl Project {
    pub async fn count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM projects")
            .fetch_one(pool)
            .await
    }

    pub async fn find_all(pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, Project>(
            r#"SELECT id,
                      name,
                      workspace_root,
                      dev_script,
                      dev_script_working_dir,
                      default_agent_working_dir,
                      remote_project_id,
                      created_at,
                      updated_at
               FROM projects
               ORDER BY created_at DESC"#,
        )
        .fetch_all(pool)
        .await
    }

    /// Find the most actively used projects based on recent task activity
    pub async fn find_most_active(pool: &SqlitePool, limit: i32) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, Project>(
            r#"
            SELECT p.id,
                   p.name,
                   p.workspace_root,
                   p.dev_script,
                   p.dev_script_working_dir,
                   p.default_agent_working_dir,
                   p.remote_project_id,
                   p.created_at,
                   p.updated_at
            FROM projects p
            WHERE p.id IN (
                SELECT DISTINCT t.project_id
                FROM tasks t
                INNER JOIN workspaces w ON w.task_id = t.id
                ORDER BY w.updated_at DESC
            )
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Project>(
            r#"SELECT id,
                      name,
                      workspace_root,
                      dev_script,
                      dev_script_working_dir,
                      default_agent_working_dir,
                      remote_project_id,
                      created_at,
                      updated_at
               FROM projects
               WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_rowid(pool: &SqlitePool, rowid: i64) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Project>(
            r#"SELECT id,
                      name,
                      workspace_root,
                      dev_script,
                      dev_script_working_dir,
                      default_agent_working_dir,
                      remote_project_id,
                      created_at,
                      updated_at
               FROM projects
               WHERE rowid = $1"#,
        )
        .bind(rowid)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_remote_project_id(
        pool: &SqlitePool,
        remote_project_id: Uuid,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Project>(
            r#"SELECT id,
                      name,
                      workspace_root,
                      dev_script,
                      dev_script_working_dir,
                      default_agent_working_dir,
                      remote_project_id,
                      created_at,
                      updated_at
               FROM projects
               WHERE remote_project_id = $1
               LIMIT 1"#,
        )
        .bind(remote_project_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn create(
        executor: impl Executor<'_, Database = Sqlite>,
        data: &CreateProject,
        project_id: Uuid,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, Project>(
            r#"INSERT INTO projects (
                    id,
                    name,
                    workspace_root
                ) VALUES (
                    $1, $2, $3
                )
                RETURNING id,
                          name,
                          workspace_root,
                          dev_script,
                          dev_script_working_dir,
                          default_agent_working_dir,
                          remote_project_id,
                          created_at,
                          updated_at"#,
        )
        .bind(project_id)
        .bind(&data.name)
        .bind(&data.workspace_root)
        .fetch_one(executor)
        .await
    }

    pub async fn update(
        pool: &SqlitePool,
        id: Uuid,
        payload: &UpdateProject,
    ) -> Result<Self, sqlx::Error> {
        let existing = Self::find_by_id(pool, id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;

        let name = payload.name.clone().unwrap_or(existing.name);
        let dev_script = payload.dev_script.clone();
        let dev_script_working_dir = payload.dev_script_working_dir.clone();
        let default_agent_working_dir = payload.default_agent_working_dir.clone();

        sqlx::query_as::<_, Project>(
            r#"UPDATE projects
               SET name = $2, dev_script = $3, dev_script_working_dir = $4, default_agent_working_dir = $5
               WHERE id = $1
               RETURNING id,
                         name,
                         workspace_root,
                         dev_script,
                         dev_script_working_dir,
                         default_agent_working_dir,
                         remote_project_id,
                         created_at,
                         updated_at"#,
        )
        .bind(id)
        .bind(name)
        .bind(dev_script)
        .bind(dev_script_working_dir)
        .bind(default_agent_working_dir)
        .fetch_one(pool)
        .await
    }

    pub async fn clear_default_agent_working_dir(
        pool: &SqlitePool,
        id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE projects
               SET default_agent_working_dir = ''
               WHERE id = $1"#,
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn set_remote_project_id(
        pool: &SqlitePool,
        id: Uuid,
        remote_project_id: Option<Uuid>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE projects
               SET remote_project_id = $2
               WHERE id = $1"#,
        )
        .bind(id)
        .bind(remote_project_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn set_workspace_root(
        pool: &SqlitePool,
        id: Uuid,
        workspace_root: Option<String>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE projects
               SET workspace_root = $2
               WHERE id = $1"#,
        )
        .bind(id)
        .bind(workspace_root)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Transaction-compatible version of set_remote_project_id
    pub async fn set_remote_project_id_tx<'e, E>(
        executor: E,
        id: Uuid,
        remote_project_id: Option<Uuid>,
    ) -> Result<(), sqlx::Error>
    where
        E: Executor<'e, Database = Sqlite>,
    {
        sqlx::query(
            r#"UPDATE projects
               SET remote_project_id = $2
               WHERE id = $1"#,
        )
        .bind(id)
        .bind(remote_project_id)
        .execute(executor)
        .await?;

        Ok(())
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}
