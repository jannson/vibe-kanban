use std::sync::{Arc, Once, OnceLock};

use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::Client;
use serde::Serialize;
use tokio::sync::RwLock;
use utils;

use crate::services::config::{
    Config, NotificationConfig, RemoteNotificationsConfig, RemoteNotifierProjectFilter,
    RemoteNotifierTarget, ReviewReadyNotificationStrategy, SoundFile,
};

/// Service for handling cross-platform notifications including sound alerts and push notifications
#[derive(Debug, Clone)]
pub struct NotificationService {
    config: Arc<RwLock<Config>>,
    client: Client,
}

#[derive(Debug, Clone)]
pub struct ReviewReadyNotificationEvent {
    pub task_id: String,
    pub task_title: String,
    pub project_id: String,
    pub project_name: Option<String>,
    pub workspace_id: String,
    pub session_id: String,
    pub status: String,
    pub branch: Option<String>,
    pub executor: Option<String>,
    pub completed_at: DateTime<Utc>,
    pub local_title: String,
    pub local_message: String,
}

#[derive(Debug, Clone, Serialize)]
struct RemoteNotifierPayload<'a> {
    schema_version: &'static str,
    event: &'static str,
    task_id: &'a str,
    task_title: &'a str,
    project_id: &'a str,
    project_name: Option<&'a str>,
    workspace_id: &'a str,
    session_id: &'a str,
    status: &'a str,
    branch: Option<&'a str>,
    executor: Option<&'a str>,
    completed_at: DateTime<Utc>,
    delivery: RemoteDelivery<'a>,
}

#[derive(Debug, Clone, Serialize)]
struct RemoteDelivery<'a> {
    target_id: &'a str,
    sound_enabled: bool,
    desktop_enabled: bool,
}

/// Cache for WSL root path from PowerShell
static WSL_ROOT_PATH_CACHE: OnceLock<Option<String>> = OnceLock::new();
static TLS_PROVIDER_INIT: Once = Once::new();

impl NotificationService {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        TLS_PROVIDER_INIT.call_once(|| {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        });

        Self {
            config,
            client: Client::builder().build().unwrap(),
        }
    }

    /// Send both sound and push notifications if enabled
    pub async fn notify(&self, title: &str, message: &str) {
        let config = self.config.read().await.notifications.clone();
        Self::send_notification(&config, title, message).await;
    }

    pub async fn notify_review_ready(&self, event: &ReviewReadyNotificationEvent) {
        let config = self.config.read().await.clone();

        match config.review_ready_notification_strategy {
            ReviewReadyNotificationStrategy::LocalOnly => {
                Self::send_notification(
                    &config.notifications,
                    &event.local_title,
                    &event.local_message,
                )
                .await;
            }
            ReviewReadyNotificationStrategy::RemoteOnly => {
                self.send_remote_notifications(&config.remote_notifications, event)
                    .await;
            }
            ReviewReadyNotificationStrategy::Both => {
                Self::send_notification(
                    &config.notifications,
                    &event.local_title,
                    &event.local_message,
                )
                .await;
                self.send_remote_notifications(&config.remote_notifications, event)
                    .await;
            }
        }
    }

    pub async fn test_remote_target(
        &self,
        target: &RemoteNotifierTarget,
        default_timeout_ms: u64,
    ) -> Result<(), String> {
        let event = Self::build_remote_test_event();
        let config = RemoteNotificationsConfig {
            enabled: true,
            targets: Vec::new(),
            default_timeout_ms,
        };

        self.send_remote_notification(target, &config, &event).await
    }

    /// Internal method to send notifications with a given config
    async fn send_notification(config: &NotificationConfig, title: &str, message: &str) {
        if config.sound_enabled {
            Self::play_sound_notification(&config.sound_file).await;
        }

        if config.push_enabled {
            Self::send_push_notification(title, message).await;
        }
    }

    /// Play a system sound notification across platforms
    async fn play_sound_notification(sound_file: &SoundFile) {
        let file_path = match sound_file.get_path().await {
            Ok(path) => path,
            Err(e) => {
                tracing::error!("Failed to create cached sound file: {}", e);
                return;
            }
        };

        // Use platform-specific sound notification
        // Note: spawn() calls are intentionally not awaited - sound notifications should be fire-and-forget
        if cfg!(target_os = "macos") {
            let _ = tokio::process::Command::new("afplay")
                .arg(&file_path)
                .spawn();
        } else if cfg!(target_os = "linux") && !utils::is_wsl2() {
            // Try different Linux audio players
            if tokio::process::Command::new("paplay")
                .arg(&file_path)
                .spawn()
                .is_ok()
            {
                // Success with paplay
            } else if tokio::process::Command::new("aplay")
                .arg(&file_path)
                .spawn()
                .is_ok()
            {
                // Success with aplay
            } else {
                // Try system bell as fallback
                let _ = tokio::process::Command::new("echo")
                    .arg("-e")
                    .arg("\\a")
                    .spawn();
            }
        } else if cfg!(target_os = "windows") || (cfg!(target_os = "linux") && utils::is_wsl2()) {
            // Convert WSL path to Windows path if in WSL2
            let file_path = if utils::is_wsl2() {
                if let Some(windows_path) = Self::wsl_to_windows_path(&file_path).await {
                    windows_path
                } else {
                    file_path.to_string_lossy().to_string()
                }
            } else {
                file_path.to_string_lossy().to_string()
            };

            let _ = tokio::process::Command::new("powershell.exe")
                .arg("-c")
                .arg(format!(
                    r#"(New-Object Media.SoundPlayer "{file_path}").PlaySync()"#
                ))
                .spawn();
        }
    }

    /// Send a cross-platform push notification
    async fn send_push_notification(title: &str, message: &str) {
        if cfg!(target_os = "macos") {
            Self::send_macos_notification(title, message).await;
        } else if cfg!(target_os = "linux") && !utils::is_wsl2() {
            Self::send_linux_notification(title, message).await;
        } else if cfg!(target_os = "windows") || (cfg!(target_os = "linux") && utils::is_wsl2()) {
            Self::send_windows_notification(title, message).await;
        }
    }

    /// Send macOS notification using osascript
    async fn send_macos_notification(title: &str, message: &str) {
        let script = format!(
            r#"display notification "{message}" with title "{title}" sound name "Glass""#,
            message = message.replace('"', r#"\""#),
            title = title.replace('"', r#"\""#)
        );

        let _ = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .spawn();
    }

    /// Send Linux notification using notify-rust
    async fn send_linux_notification(title: &str, message: &str) {
        use notify_rust::Notification;

        let title = title.to_string();
        let message = message.to_string();

        let _handle = tokio::task::spawn_blocking(move || {
            if let Err(e) = Notification::new()
                .summary(&title)
                .body(&message)
                .timeout(10000)
                .show()
            {
                tracing::error!("Failed to send Linux notification: {}", e);
            }
        });
        drop(_handle); // Don't await, fire-and-forget
    }

    /// Send Windows/WSL notification using PowerShell toast script
    async fn send_windows_notification(title: &str, message: &str) {
        let script_path = match utils::get_powershell_script().await {
            Ok(path) => path,
            Err(e) => {
                tracing::error!("Failed to get PowerShell script: {}", e);
                return;
            }
        };

        // Convert WSL path to Windows path if in WSL2
        let script_path_str = if utils::is_wsl2() {
            if let Some(windows_path) = Self::wsl_to_windows_path(&script_path).await {
                windows_path
            } else {
                script_path.to_string_lossy().to_string()
            }
        } else {
            script_path.to_string_lossy().to_string()
        };

        let _ = tokio::process::Command::new("powershell.exe")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(script_path_str)
            .arg("-Title")
            .arg(title)
            .arg("-Message")
            .arg(message)
            .spawn();
    }

    /// Get WSL root path via PowerShell (cached)
    async fn get_wsl_root_path() -> Option<String> {
        if let Some(cached) = WSL_ROOT_PATH_CACHE.get() {
            return cached.clone();
        }

        match tokio::process::Command::new("powershell.exe")
            .arg("-c")
            .arg("(Get-Location).Path -replace '^.*::', ''")
            .current_dir("/")
            .output()
            .await
        {
            Ok(output) => {
                match String::from_utf8(output.stdout) {
                    Ok(pwd_str) => {
                        let pwd = pwd_str.trim();
                        tracing::info!("WSL root path detected: {}", pwd);

                        // Cache the result
                        let _ = WSL_ROOT_PATH_CACHE.set(Some(pwd.to_string()));
                        return Some(pwd.to_string());
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse PowerShell pwd output as UTF-8: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to execute PowerShell pwd command: {}", e);
            }
        }

        // Cache the failure result
        let _ = WSL_ROOT_PATH_CACHE.set(None);
        None
    }

    /// Convert WSL path to Windows UNC path for PowerShell
    async fn wsl_to_windows_path(wsl_path: &std::path::Path) -> Option<String> {
        let path_str = wsl_path.to_string_lossy();

        // Relative paths work fine as-is in PowerShell
        if !path_str.starts_with('/') {
            tracing::debug!("Using relative path as-is: {}", path_str);
            return Some(path_str.to_string());
        }

        // Get cached WSL root path from PowerShell
        if let Some(wsl_root) = Self::get_wsl_root_path().await {
            // Simply concatenate WSL root with the absolute path - PowerShell doesn't mind /
            let windows_path = format!("{wsl_root}{path_str}");
            tracing::debug!("WSL path converted: {} -> {}", path_str, windows_path);
            Some(windows_path)
        } else {
            tracing::error!(
                "Failed to determine WSL root path for conversion: {}",
                path_str
            );
            None
        }
    }

    async fn send_remote_notifications(
        &self,
        config: &RemoteNotificationsConfig,
        event: &ReviewReadyNotificationEvent,
    ) {
        if !config.enabled {
            return;
        }

        for target in &config.targets {
            match Self::match_remote_target(target, event) {
                Ok(true) => {
                    if let Err(err) = self.send_remote_notification(target, config, event).await {
                        tracing::warn!(
                            target_id = %target.id,
                            task_id = %event.task_id,
                            project_id = %event.project_id,
                            error = %err,
                            "Remote notifier delivery failed"
                        );
                    }
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::warn!(
                        target_id = %target.id,
                        task_id = %event.task_id,
                        project_id = %event.project_id,
                        error = %err,
                        "Remote notifier target skipped due to invalid configuration"
                    );
                }
            }
        }
    }

    fn match_remote_target(
        target: &RemoteNotifierTarget,
        event: &ReviewReadyNotificationEvent,
    ) -> Result<bool, String> {
        if !target.enabled {
            return Ok(false);
        }

        let url = target.url.trim();
        if url.is_empty() {
            return Err("url is empty".to_string());
        }

        let project_matched = match &target.projects {
            RemoteNotifierProjectFilter::All => true,
            RemoteNotifierProjectFilter::ProjectIds(project_ids) => {
                project_ids.iter().any(|id| id == &event.project_id)
            }
        };

        if !project_matched {
            return Ok(false);
        }

        if let Some(pattern) = target.title_regex.as_deref()
            && !pattern.trim().is_empty()
        {
            let regex = Regex::new(pattern).map_err(|e| e.to_string())?;
            if !regex.is_match(&event.task_title) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    async fn send_remote_notification(
        &self,
        target: &RemoteNotifierTarget,
        config: &RemoteNotificationsConfig,
        event: &ReviewReadyNotificationEvent,
    ) -> Result<(), String> {
        let timeout_ms = target.timeout_ms.unwrap_or(config.default_timeout_ms);
        let payload = RemoteNotifierPayload {
            schema_version: "v1",
            event: "task_review_ready",
            task_id: &event.task_id,
            task_title: &event.task_title,
            project_id: &event.project_id,
            project_name: event.project_name.as_deref(),
            workspace_id: &event.workspace_id,
            session_id: &event.session_id,
            status: &event.status,
            branch: event.branch.as_deref(),
            executor: event.executor.as_deref(),
            completed_at: event.completed_at,
            delivery: RemoteDelivery {
                target_id: &target.id,
                sound_enabled: target.sound_enabled,
                desktop_enabled: target.desktop_enabled,
            },
        };

        let mut request = self
            .client
            .post(&target.url)
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .json(&payload);

        if let Some(token) = target.token.as_deref()
            && !token.trim().is_empty()
        {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("unexpected status {}", response.status()));
        }

        Ok(())
    }

    fn build_remote_test_event() -> ReviewReadyNotificationEvent {
        ReviewReadyNotificationEvent {
            task_id: "remote-notifier-test".to_string(),
            task_title: "Test Notification".to_string(),
            project_id: "settings".to_string(),
            project_name: Some("Vibe Kanban".to_string()),
            workspace_id: "settings".to_string(),
            session_id: "remote-notifier-test".to_string(),
            status: "inreview".to_string(),
            branch: Some("settings/remote-notifier-test".to_string()),
            executor: Some("system".to_string()),
            completed_at: Utc::now(),
            local_title: "Test Notification".to_string(),
            local_message: "Vibe Kanban remote notifier test".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;
    use tokio::sync::RwLock;

    use super::{NotificationService, ReviewReadyNotificationEvent};
    use crate::services::config::{
        Config, RemoteNotificationsConfig, RemoteNotifierProjectFilter, RemoteNotifierTarget,
        ReviewReadyNotificationStrategy,
    };

    fn base_event() -> ReviewReadyNotificationEvent {
        ReviewReadyNotificationEvent {
            task_id: "task-1".to_string(),
            task_title: "urgent: fix prod".to_string(),
            project_id: "project-1".to_string(),
            project_name: Some("Project One".to_string()),
            workspace_id: "workspace-1".to_string(),
            session_id: "session-1".to_string(),
            status: "inreview".to_string(),
            branch: Some("vk/task-1".to_string()),
            executor: Some("codex".to_string()),
            completed_at: Utc::now(),
            local_title: "Task Complete: urgent: fix prod".to_string(),
            local_message: "done".to_string(),
        }
    }

    fn service() -> NotificationService {
        NotificationService::new(Arc::new(RwLock::new(Config::default())))
    }

    #[test]
    fn builds_remote_test_event() {
        let event = NotificationService::build_remote_test_event();

        assert_eq!(event.task_title, "Test Notification");
        assert_eq!(event.project_name.as_deref(), Some("Vibe Kanban"));
        assert_eq!(event.status, "inreview");
        assert_eq!(event.branch.as_deref(), Some("settings/remote-notifier-test"));
    }

    #[test]
    fn matches_target_for_all_projects_without_regex() {
        let target = RemoteNotifierTarget {
            id: "target-a".to_string(),
            url: "http://127.0.0.1:43110/notify".to_string(),
            projects: RemoteNotifierProjectFilter::All,
            ..RemoteNotifierTarget::default()
        };

        let event = base_event();
        assert!(NotificationService::match_remote_target(&target, &event).unwrap());
    }

    #[test]
    fn skips_target_when_project_filter_does_not_match() {
        let target = RemoteNotifierTarget {
            id: "target-a".to_string(),
            url: "http://127.0.0.1:43110/notify".to_string(),
            projects: RemoteNotifierProjectFilter::ProjectIds(vec!["project-2".to_string()]),
            ..RemoteNotifierTarget::default()
        };

        let event = base_event();
        assert!(!NotificationService::match_remote_target(&target, &event).unwrap());
    }

    #[test]
    fn skips_target_when_title_regex_does_not_match() {
        let target = RemoteNotifierTarget {
            id: "target-a".to_string(),
            url: "http://127.0.0.1:43110/notify".to_string(),
            title_regex: Some("^feat:".to_string()),
            ..RemoteNotifierTarget::default()
        };

        let event = base_event();
        assert!(!NotificationService::match_remote_target(&target, &event).unwrap());
    }

    #[tokio::test]
    async fn notify_review_ready_ignores_disabled_remote_notifications() {
        let service = service();
        let event = base_event();
        let config = RemoteNotificationsConfig {
            enabled: false,
            targets: vec![RemoteNotifierTarget {
                id: "target-a".to_string(),
                url: "http://127.0.0.1:43110/notify".to_string(),
                ..RemoteNotifierTarget::default()
            }],
            default_timeout_ms: 1500,
        };

        service.send_remote_notifications(&config, &event).await;
    }

    #[tokio::test]
    async fn notify_review_ready_remote_only_skips_legacy_local_notifications() {
        let mut config = Config::default();
        config.notifications.sound_enabled = true;
        config.notifications.push_enabled = true;
        config.remote_notifications = RemoteNotificationsConfig {
            enabled: false,
            targets: Vec::new(),
            default_timeout_ms: 1500,
        };
        config.review_ready_notification_strategy = ReviewReadyNotificationStrategy::RemoteOnly;

        let service = NotificationService::new(Arc::new(RwLock::new(config)));
        service.notify_review_ready(&base_event()).await;
    }
}
