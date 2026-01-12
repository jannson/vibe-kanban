use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Row, Sqlite, SqlitePool, sqlite::SqliteRow};
use thiserror::Error;
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum RepoError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("Repository not found")]
    NotFound,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct Repo {
    pub id: Uuid,
    pub path: PathBuf,
    pub name: String,
    pub display_name: String,
    #[ts(type = "Date")]
    pub created_at: DateTime<Utc>,
    #[ts(type = "Date")]
    pub updated_at: DateTime<Utc>,
}

impl Repo {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let path: String = row.try_get("path")?;
        Ok(Self {
            id: row.try_get("id")?,
            path: PathBuf::from(path),
            name: row.try_get("name")?,
            display_name: row.try_get("display_name")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }

    /// Get repos that still have the migration sentinel as their name.
    /// Used by the startup backfill to fix repo names.
    pub async fn list_needing_name_fix(pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(
            r#"SELECT id,
                      path,
                      name,
                      display_name,
                      created_at,
                      updated_at
               FROM repos
               WHERE name = '__NEEDS_BACKFILL__'"#,
        )
        .fetch_all(pool)
        .await?;
        rows.into_iter().map(|row| Self::from_row(&row)).collect()
    }

    pub async fn update_name(
        pool: &SqlitePool,
        id: Uuid,
        name: &str,
        display_name: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE repos SET name = $1, display_name = $2, updated_at = datetime('now', 'subsec') WHERE id = $3",
        )
        .bind(name)
        .bind(display_name)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query(
            r#"SELECT id,
                      path,
                      name,
                      display_name,
                      created_at,
                      updated_at
               FROM repos
               WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        row.map(|row| Self::from_row(&row)).transpose()
    }

    pub async fn find_by_path(
        pool: &SqlitePool,
        path: &Path,
    ) -> Result<Option<Self>, sqlx::Error> {
        let path_str = path.to_string_lossy().to_string();
        let row = sqlx::query(
            r#"SELECT id,
                      path,
                      name,
                      display_name,
                      created_at,
                      updated_at
               FROM repos
               WHERE path = $1"#,
        )
        .bind(path_str)
        .fetch_optional(pool)
        .await?;
        row.map(|row| Self::from_row(&row)).transpose()
    }

    pub async fn find_or_create<'e, E>(
        executor: E,
        path: &Path,
        display_name: &str,
    ) -> Result<Self, sqlx::Error>
    where
        E: Executor<'e, Database = Sqlite>,
    {
        let path_str = path.to_string_lossy().to_string();
        let id = Uuid::new_v4();
        let repo_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| id.to_string());

        // Use INSERT OR IGNORE + SELECT to handle race conditions atomically
        let row = sqlx::query(
            r#"INSERT INTO repos (id, path, name, display_name)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT(path) DO UPDATE SET updated_at = updated_at
               RETURNING id,
                         path,
                         name,
                         display_name,
                         created_at,
                         updated_at"#,
        )
        .bind(id)
        .bind(path_str)
        .bind(repo_name)
        .bind(display_name)
        .fetch_one(executor)
        .await?;
        Self::from_row(&row)
    }

    pub async fn delete_orphaned(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            r#"DELETE FROM repos
               WHERE id NOT IN (SELECT repo_id FROM project_repos)
                 AND id NOT IN (SELECT repo_id FROM workspace_repos)"#,
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}
