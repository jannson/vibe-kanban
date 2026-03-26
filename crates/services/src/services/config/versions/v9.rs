use anyhow::Error;
use executors::{executors::BaseCodingAgent, profile::ExecutorProfileId};
use regex::Regex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::Url;
pub use v8::{
    EditorConfig, EditorType, GitHubConfig, NotificationConfig, ShowcaseState, SoundFile,
    ThemeMode, UiLanguage,
};

use crate::services::config::{ConfigError, versions::v8};

fn default_git_branch_prefix() -> String {
    "vk".to_string()
}

fn default_pr_auto_description_enabled() -> bool {
    true
}

fn default_use_original_repos() -> bool {
    false
}

fn default_auto_commit_enabled() -> bool {
    false
}

fn default_remote_notification_timeout_ms() -> u64 {
    1500
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct Config {
    pub config_version: String,
    pub theme: ThemeMode,
    pub executor_profile: ExecutorProfileId,
    pub disclaimer_acknowledged: bool,
    pub onboarding_acknowledged: bool,
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub remote_notifications: RemoteNotificationsConfig,
    pub editor: EditorConfig,
    pub github: GitHubConfig,
    pub analytics_enabled: bool,
    pub last_app_version: Option<String>,
    pub show_release_notes: bool,
    #[serde(default)]
    pub language: UiLanguage,
    #[serde(default = "default_git_branch_prefix")]
    pub git_branch_prefix: String,
    #[serde(default = "default_use_original_repos")]
    pub default_use_original_repos: bool,
    #[serde(default = "default_auto_commit_enabled")]
    pub auto_commit_enabled: bool,
    #[serde(default)]
    pub showcases: ShowcaseState,
    #[serde(default = "default_pr_auto_description_enabled")]
    pub pr_auto_description_enabled: bool,
    #[serde(default)]
    pub pr_auto_description_prompt: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct RemoteNotificationsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub targets: Vec<RemoteNotifierTarget>,
    #[serde(default = "default_remote_notification_timeout_ms")]
    pub default_timeout_ms: u64,
}

impl Default for RemoteNotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            targets: Vec::new(),
            default_timeout_ms: default_remote_notification_timeout_ms(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct RemoteNotifierTarget {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub label: Option<String>,
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub projects: RemoteNotifierProjectFilter,
    #[serde(default)]
    pub title_regex: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default = "default_true")]
    pub sound_enabled: bool,
    #[serde(default)]
    pub desktop_enabled: bool,
}

impl Default for RemoteNotifierTarget {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            label: None,
            url: String::new(),
            token: None,
            projects: RemoteNotifierProjectFilter::All,
            title_regex: None,
            timeout_ms: None,
            sound_enabled: true,
            desktop_enabled: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RemoteNotifierProjectFilter {
    #[default]
    All,
    ProjectIds(Vec<String>),
}

impl Config {
    fn from_v8_config(old_config: v8::Config) -> Self {
        Self {
            config_version: "v9".to_string(),
            theme: old_config.theme,
            executor_profile: old_config.executor_profile,
            disclaimer_acknowledged: old_config.disclaimer_acknowledged,
            onboarding_acknowledged: old_config.onboarding_acknowledged,
            notifications: old_config.notifications,
            remote_notifications: RemoteNotificationsConfig::default(),
            editor: old_config.editor,
            github: old_config.github,
            analytics_enabled: old_config.analytics_enabled,
            last_app_version: old_config.last_app_version,
            show_release_notes: old_config.show_release_notes,
            language: old_config.language,
            git_branch_prefix: old_config.git_branch_prefix,
            default_use_original_repos: old_config.default_use_original_repos,
            auto_commit_enabled: old_config.auto_commit_enabled,
            showcases: old_config.showcases,
            pr_auto_description_enabled: old_config.pr_auto_description_enabled,
            pr_auto_description_prompt: old_config.pr_auto_description_prompt,
        }
    }

    pub fn from_previous_version(raw_config: &str) -> Result<Self, Error> {
        let old_config = v8::Config::from(raw_config.to_string());
        Ok(Self::from_v8_config(old_config))
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let remote = &self.remote_notifications;

        if remote.default_timeout_ms == 0 || remote.default_timeout_ms > 60_000 {
            return Err(ConfigError::ValidationError(
                "Remote notification default timeout must be between 1 and 60000 ms".to_string(),
            ));
        }

        let mut seen_ids = std::collections::HashSet::new();
        for target in &remote.targets {
            let target_id = target.id.trim();
            if target_id.is_empty() {
                return Err(ConfigError::ValidationError(
                    "Remote notifier target id must not be empty".to_string(),
                ));
            }

            if !seen_ids.insert(target_id.to_string()) {
                return Err(ConfigError::ValidationError(format!(
                    "Duplicate remote notifier target id: {target_id}"
                )));
            }

            let url = target.url.trim();
            if url.is_empty() {
                return Err(ConfigError::ValidationError(format!(
                    "Remote notifier target '{target_id}' url must not be empty"
                )));
            }

            let parsed = Url::parse(url).map_err(|e| {
                ConfigError::ValidationError(format!(
                    "Remote notifier target '{target_id}' has invalid url: {e}"
                ))
            })?;

            match parsed.scheme() {
                "http" | "https" => {}
                other => {
                    return Err(ConfigError::ValidationError(format!(
                        "Remote notifier target '{target_id}' has unsupported url scheme: {other}"
                    )));
                }
            }

            if let Some(timeout_ms) = target.timeout_ms
                && (timeout_ms == 0 || timeout_ms > 60_000)
            {
                return Err(ConfigError::ValidationError(format!(
                    "Remote notifier target '{target_id}' timeout must be between 1 and 60000 ms"
                )));
            }

            if let Some(regex) = target.title_regex.as_deref()
                && !regex.trim().is_empty()
            {
                Regex::new(regex).map_err(|e| {
                    ConfigError::ValidationError(format!(
                        "Remote notifier target '{target_id}' has invalid title_regex: {e}"
                    ))
                })?;
            }

            if let RemoteNotifierProjectFilter::ProjectIds(project_ids) = &target.projects {
                let mut seen_projects = std::collections::HashSet::new();
                for project_id in project_ids {
                    let project_id = project_id.trim();
                    if project_id.is_empty() {
                        return Err(ConfigError::ValidationError(format!(
                            "Remote notifier target '{target_id}' contains an empty project id"
                        )));
                    }

                    if !seen_projects.insert(project_id.to_string()) {
                        return Err(ConfigError::ValidationError(format!(
                            "Remote notifier target '{target_id}' contains duplicate project id '{project_id}'"
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

impl From<String> for Config {
    fn from(raw_config: String) -> Self {
        if let Ok(config) = serde_json::from_str::<Config>(&raw_config)
            && config.config_version == "v9"
            && config.validate().is_ok()
        {
            return config;
        }

        match Self::from_previous_version(&raw_config) {
            Ok(config) => {
                if let Err(e) = config.validate() {
                    tracing::warn!("Config validation failed after migration: {e}");
                    Self::default()
                } else {
                    tracing::info!("Config upgraded to v9");
                    config
                }
            }
            Err(e) => {
                tracing::warn!("Config migration failed: {}, using default", e);
                Self::default()
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: "v9".to_string(),
            theme: ThemeMode::System,
            executor_profile: ExecutorProfileId::new(BaseCodingAgent::ClaudeCode),
            disclaimer_acknowledged: false,
            onboarding_acknowledged: false,
            notifications: NotificationConfig::default(),
            remote_notifications: RemoteNotificationsConfig::default(),
            editor: EditorConfig::default(),
            github: GitHubConfig::default(),
            analytics_enabled: true,
            last_app_version: None,
            show_release_notes: false,
            language: UiLanguage::default(),
            git_branch_prefix: default_git_branch_prefix(),
            default_use_original_repos: default_use_original_repos(),
            auto_commit_enabled: default_auto_commit_enabled(),
            showcases: ShowcaseState::default(),
            pr_auto_description_enabled: true,
            pr_auto_description_prompt: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Config, RemoteNotificationsConfig, RemoteNotifierProjectFilter, RemoteNotifierTarget,
    };

    fn base_config() -> Config {
        Config {
            remote_notifications: RemoteNotificationsConfig {
                enabled: true,
                targets: vec![],
                default_timeout_ms: 1500,
            },
            ..Config::default()
        }
    }

    #[test]
    fn validates_remote_notifier_config() {
        let mut config = base_config();
        config
            .remote_notifications
            .targets
            .push(RemoteNotifierTarget {
                id: "macbook".to_string(),
                url: "http://127.0.0.1:43110/notify".to_string(),
                token: Some("secret".to_string()),
                projects: RemoteNotifierProjectFilter::ProjectIds(vec![
                    "project-a".to_string(),
                    "project-b".to_string(),
                ]),
                title_regex: Some("^(urgent|prod):".to_string()),
                timeout_ms: Some(1000),
                ..RemoteNotifierTarget::default()
            });

        assert!(config.validate().is_ok());
    }

    #[test]
    fn remote_notifications_default_timeout_is_non_zero() {
        let config = RemoteNotificationsConfig::default();
        assert_eq!(config.default_timeout_ms, 1500);
    }

    #[test]
    fn rejects_duplicate_remote_notifier_ids() {
        let mut config = base_config();
        config.remote_notifications.targets = vec![
            RemoteNotifierTarget {
                id: "same".to_string(),
                url: "http://127.0.0.1:43110/notify".to_string(),
                ..RemoteNotifierTarget::default()
            },
            RemoteNotifierTarget {
                id: "same".to_string(),
                url: "http://127.0.0.1:43111/notify".to_string(),
                ..RemoteNotifierTarget::default()
            },
        ];

        let err = config.validate().unwrap_err();
        assert!(
            err.to_string()
                .contains("Duplicate remote notifier target id")
        );
    }

    #[test]
    fn rejects_invalid_remote_notifier_regex() {
        let mut config = base_config();
        config
            .remote_notifications
            .targets
            .push(RemoteNotifierTarget {
                id: "bad-regex".to_string(),
                url: "http://127.0.0.1:43110/notify".to_string(),
                title_regex: Some("[".to_string()),
                ..RemoteNotifierTarget::default()
            });

        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("invalid title_regex"));
    }
}
