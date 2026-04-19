use anyhow::Error;
use executors::{executors::BaseCodingAgent, profile::ExecutorProfileId};
use regex::Regex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::Url;
pub use v9::{
    EditorConfig, EditorType, GitHubConfig, NotificationConfig, RemoteNotificationsConfig,
    RemoteNotifierProjectFilter, RemoteNotifierTarget, ShowcaseState, SoundFile, ThemeMode,
    UiLanguage,
};

use crate::services::config::{ConfigError, versions::v9};

fn default_git_branch_prefix() -> String {
    "kb".to_string()
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

fn default_review_ready_notification_strategy() -> ReviewReadyNotificationStrategy {
    ReviewReadyNotificationStrategy::LocalOnly
}

fn default_quick_reply_enabled() -> bool {
    true
}

fn default_execution_log_retention_days() -> u32 {
    30
}

fn default_cleanup_dropped_execution_logs() -> bool {
    true
}

fn default_execution_log_max_mb() -> u32 {
    20
}

fn default_execution_log_cleanup_on_startup() -> bool {
    false
}

fn default_cleanup_closed_task_intermediate_logs() -> bool {
    true
}

fn default_keep_latest_execution_logs_per_session() -> u32 {
    1
}

fn default_quick_reply_phrases() -> Vec<String> {
    vec![
        "愿意".to_string(),
        "要".to_string(),
        "下一步".to_string(),
        "请继续".to_string(),
        "请直接修改".to_string(),
        "请重新显示".to_string(),
        "继续".to_string(),
        "可以".to_string(),
    ]
}

fn default_quick_reply_rules() -> Vec<QuickReplyRule> {
    vec![
        QuickReplyRule {
            pattern: "愿意.*下一步|下一步.*愿意".to_string(),
            phrases: vec!["愿意".to_string(), "下一步".to_string()],
        },
        QuickReplyRule {
            pattern: "要.*下一步|下一步.*要".to_string(),
            phrases: vec!["要".to_string(), "下一步".to_string()],
        },
        QuickReplyRule {
            pattern: "愿意.*继续|继续.*愿意".to_string(),
            phrases: vec!["愿意".to_string(), "继续".to_string(), "请继续".to_string()],
        },
        QuickReplyRule {
            pattern: "需要.*下一步|下一步.*需要".to_string(),
            phrases: vec!["下一步".to_string()],
        },
        QuickReplyRule {
            pattern: "愿意".to_string(),
            phrases: vec!["愿意".to_string()],
        },
        QuickReplyRule {
            pattern: "要".to_string(),
            phrases: vec!["要".to_string()],
        },
        QuickReplyRule {
            pattern: "下一步".to_string(),
            phrases: vec!["下一步".to_string()],
        },
        QuickReplyRule {
            pattern: "继续".to_string(),
            phrases: vec!["继续".to_string(), "请继续".to_string()],
        },
        QuickReplyRule {
            pattern: "重新显示".to_string(),
            phrases: vec!["重新显示".to_string(), "请重新显示".to_string()],
        },
        QuickReplyRule {
            pattern: "直接修改|直接改|修改".to_string(),
            phrases: vec!["直接修改".to_string(), "请直接修改".to_string()],
        },
        QuickReplyRule {
            pattern: "可以".to_string(),
            phrases: vec!["可以".to_string()],
        },
    ]
}

fn normalize_git_branch_prefix(prefix: String) -> String {
    if prefix.trim() == "vk" {
        "kb".to_string()
    } else {
        prefix
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct QuickReplyRule {
    pub pattern: String,
    #[serde(default)]
    pub phrases: Vec<String>,
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
    #[serde(default = "default_review_ready_notification_strategy")]
    pub review_ready_notification_strategy: ReviewReadyNotificationStrategy,
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
    #[serde(default = "default_quick_reply_enabled")]
    pub quick_reply_enabled: bool,
    #[serde(default = "default_quick_reply_phrases")]
    pub quick_reply_phrases: Vec<String>,
    #[serde(default = "default_quick_reply_rules")]
    pub quick_reply_rules: Vec<QuickReplyRule>,
    #[serde(default = "default_execution_log_retention_days")]
    pub execution_log_retention_days: u32,
    #[serde(default = "default_cleanup_dropped_execution_logs")]
    pub cleanup_dropped_execution_logs: bool,
    #[serde(default = "default_execution_log_max_mb")]
    pub execution_log_max_mb: u32,
    #[serde(default = "default_execution_log_cleanup_on_startup")]
    pub execution_log_cleanup_on_startup: bool,
    #[serde(default = "default_cleanup_closed_task_intermediate_logs")]
    pub cleanup_closed_task_intermediate_logs: bool,
    #[serde(default = "default_keep_latest_execution_logs_per_session")]
    pub keep_latest_execution_logs_per_session: u32,
    #[serde(default)]
    pub showcases: ShowcaseState,
    #[serde(default = "default_pr_auto_description_enabled")]
    pub pr_auto_description_enabled: bool,
    #[serde(default)]
    pub pr_auto_description_prompt: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewReadyNotificationStrategy {
    #[default]
    LocalOnly,
    RemoteOnly,
    Both,
}

impl Config {
    fn from_v9_config(old_config: v9::Config) -> Self {
        Self {
            config_version: "v10".to_string(),
            theme: old_config.theme,
            executor_profile: old_config.executor_profile,
            disclaimer_acknowledged: old_config.disclaimer_acknowledged,
            onboarding_acknowledged: old_config.onboarding_acknowledged,
            notifications: old_config.notifications,
            remote_notifications: old_config.remote_notifications,
            review_ready_notification_strategy: ReviewReadyNotificationStrategy::RemoteOnly,
            editor: old_config.editor,
            github: old_config.github,
            analytics_enabled: old_config.analytics_enabled,
            last_app_version: old_config.last_app_version,
            show_release_notes: old_config.show_release_notes,
            language: old_config.language,
            git_branch_prefix: normalize_git_branch_prefix(old_config.git_branch_prefix),
            default_use_original_repos: old_config.default_use_original_repos,
            auto_commit_enabled: old_config.auto_commit_enabled,
            quick_reply_enabled: default_quick_reply_enabled(),
            quick_reply_phrases: default_quick_reply_phrases(),
            quick_reply_rules: default_quick_reply_rules(),
            execution_log_retention_days: default_execution_log_retention_days(),
            cleanup_dropped_execution_logs: default_cleanup_dropped_execution_logs(),
            execution_log_max_mb: default_execution_log_max_mb(),
            execution_log_cleanup_on_startup: default_execution_log_cleanup_on_startup(),
            cleanup_closed_task_intermediate_logs: default_cleanup_closed_task_intermediate_logs(),
            keep_latest_execution_logs_per_session: default_keep_latest_execution_logs_per_session(
            ),
            showcases: old_config.showcases,
            pr_auto_description_enabled: old_config.pr_auto_description_enabled,
            pr_auto_description_prompt: old_config.pr_auto_description_prompt,
        }
    }

    fn normalize_legacy_values(mut self) -> Self {
        self.git_branch_prefix = normalize_git_branch_prefix(self.git_branch_prefix);
        self.quick_reply_phrases = self
            .quick_reply_phrases
            .into_iter()
            .map(|phrase| phrase.trim().to_string())
            .filter(|phrase| !phrase.is_empty())
            .fold(Vec::new(), |mut acc, phrase| {
                if !acc.contains(&phrase) {
                    acc.push(phrase);
                }
                acc
            });

        if self.quick_reply_phrases.is_empty() {
            self.quick_reply_phrases = default_quick_reply_phrases();
        }

        self.quick_reply_rules = self
            .quick_reply_rules
            .into_iter()
            .filter_map(|rule| {
                let pattern = rule.pattern.trim().to_string();
                let phrases = rule
                    .phrases
                    .into_iter()
                    .map(|phrase| phrase.trim().to_string())
                    .filter(|phrase| !phrase.is_empty())
                    .fold(Vec::new(), |mut acc, phrase| {
                        if !acc.contains(&phrase) {
                            acc.push(phrase);
                        }
                        acc
                    });

                if pattern.is_empty() || phrases.is_empty() {
                    None
                } else {
                    Some(QuickReplyRule { pattern, phrases })
                }
            })
            .collect();

        if self.quick_reply_rules.is_empty() {
            self.quick_reply_rules = default_quick_reply_rules();
        }

        self
    }

    pub fn from_previous_version(raw_config: &str) -> Result<Self, Error> {
        let old_config = v9::Config::from(raw_config.to_string());
        Ok(Self::from_v9_config(old_config))
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let remote = &self.remote_notifications;

        if self
            .quick_reply_phrases
            .iter()
            .any(|phrase| phrase.trim().is_empty())
        {
            return Err(ConfigError::ValidationError(
                "Quick reply phrases must not contain empty items".to_string(),
            ));
        }

        if self.execution_log_retention_days > 3650 {
            return Err(ConfigError::ValidationError(
                "Execution log retention must be between 0 and 3650 days".to_string(),
            ));
        }

        if self.execution_log_max_mb > 1024 {
            return Err(ConfigError::ValidationError(
                "Execution log size cap must be between 0 and 1024 MB".to_string(),
            ));
        }

        if self.keep_latest_execution_logs_per_session == 0
            || self.keep_latest_execution_logs_per_session > 20
        {
            return Err(ConfigError::ValidationError(
                "Keep latest execution logs per session must be between 1 and 20".to_string(),
            ));
        }

        for rule in &self.quick_reply_rules {
            if rule.pattern.trim().is_empty() {
                return Err(ConfigError::ValidationError(
                    "Quick reply rules must not contain empty patterns".to_string(),
                ));
            }

            if rule.phrases.iter().any(|phrase| phrase.trim().is_empty()) {
                return Err(ConfigError::ValidationError(
                    "Quick reply rules must not contain empty phrases".to_string(),
                ));
            }

            Regex::new(rule.pattern.trim()).map_err(|e| {
                ConfigError::ValidationError(format!(
                    "Quick reply rule has invalid regex '{}': {e}",
                    rule.pattern.trim()
                ))
            })?;
        }

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
            && config.config_version == "v10"
            && config.clone().normalize_legacy_values().validate().is_ok()
        {
            return config.normalize_legacy_values();
        }

        match Self::from_previous_version(&raw_config) {
            Ok(config) => {
                let config = config.normalize_legacy_values();
                if let Err(e) = config.validate() {
                    tracing::warn!("Config validation failed after migration: {e}");
                    Self::default()
                } else {
                    tracing::info!("Config upgraded to v10");
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
            config_version: "v10".to_string(),
            theme: ThemeMode::System,
            executor_profile: ExecutorProfileId::new(BaseCodingAgent::ClaudeCode),
            disclaimer_acknowledged: false,
            onboarding_acknowledged: false,
            notifications: NotificationConfig::default(),
            remote_notifications: RemoteNotificationsConfig::default(),
            review_ready_notification_strategy: default_review_ready_notification_strategy(),
            editor: EditorConfig::default(),
            github: GitHubConfig::default(),
            analytics_enabled: true,
            last_app_version: None,
            show_release_notes: false,
            language: UiLanguage::default(),
            git_branch_prefix: default_git_branch_prefix(),
            default_use_original_repos: default_use_original_repos(),
            auto_commit_enabled: default_auto_commit_enabled(),
            quick_reply_enabled: default_quick_reply_enabled(),
            quick_reply_phrases: default_quick_reply_phrases(),
            quick_reply_rules: default_quick_reply_rules(),
            execution_log_retention_days: default_execution_log_retention_days(),
            cleanup_dropped_execution_logs: default_cleanup_dropped_execution_logs(),
            execution_log_max_mb: default_execution_log_max_mb(),
            execution_log_cleanup_on_startup: default_execution_log_cleanup_on_startup(),
            cleanup_closed_task_intermediate_logs: default_cleanup_closed_task_intermediate_logs(),
            keep_latest_execution_logs_per_session: default_keep_latest_execution_logs_per_session(
            ),
            showcases: ShowcaseState::default(),
            pr_auto_description_enabled: true,
            pr_auto_description_prompt: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Config, QuickReplyRule, RemoteNotificationsConfig, RemoteNotifierProjectFilter,
        RemoteNotifierTarget, ReviewReadyNotificationStrategy,
    };
    use crate::services::config::versions::v9;

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
    fn default_review_ready_strategy_is_local_only() {
        assert_eq!(
            Config::default().review_ready_notification_strategy,
            ReviewReadyNotificationStrategy::LocalOnly
        );
    }

    #[test]
    fn default_quick_reply_config_is_populated() {
        let config = Config::default();

        assert!(config.quick_reply_enabled);
        assert!(!config.quick_reply_phrases.is_empty());
        assert!(config.quick_reply_phrases.contains(&"下一步".to_string()));
        assert!(!config.quick_reply_rules.is_empty());
        assert_eq!(config.execution_log_retention_days, 30);
        assert!(config.cleanup_dropped_execution_logs);
        assert_eq!(config.execution_log_max_mb, 20);
        assert!(!config.execution_log_cleanup_on_startup);
        assert!(config.cleanup_closed_task_intermediate_logs);
        assert_eq!(config.keep_latest_execution_logs_per_session, 1);
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
                id: "macbook".to_string(),
                url: "http://127.0.0.1:43110/notify".to_string(),
                title_regex: Some("(".to_string()),
                ..RemoteNotifierTarget::default()
            });

        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("invalid title_regex"));
    }

    #[test]
    fn migrates_legacy_vk_prefix_in_v10_config() {
        let mut config = Config::default();
        config.git_branch_prefix = "vk".to_string();

        let raw_config = serde_json::to_string(&config).unwrap();
        let migrated = Config::from(raw_config);

        assert_eq!(migrated.git_branch_prefix, "kb");
    }

    #[test]
    fn migrates_legacy_vk_prefix_from_v9_config() {
        let mut config = v9::Config::default();
        config.git_branch_prefix = "vk".to_string();

        let raw_config = serde_json::to_string(&config).unwrap();
        let migrated = Config::from(raw_config);

        assert_eq!(migrated.git_branch_prefix, "kb");
    }

    #[test]
    fn normalizes_quick_reply_phrases() {
        let mut config = Config::default();
        config.quick_reply_phrases = vec![
            " 愿意 ".to_string(),
            "".to_string(),
            "愿意".to_string(),
            "下一步".to_string(),
        ];

        let raw_config = serde_json::to_string(&config).unwrap();

        let migrated = Config::from(raw_config);

        assert_eq!(
            migrated.quick_reply_phrases,
            vec!["愿意".to_string(), "下一步".to_string()]
        );
    }

    #[test]
    fn normalizes_quick_reply_rules() {
        let mut config = Config::default();
        config.quick_reply_rules = vec![
            QuickReplyRule {
                pattern: " 愿意 ".to_string(),
                phrases: vec![" 愿意 ".to_string(), "".to_string(), "愿意".to_string()],
            },
            QuickReplyRule {
                pattern: "".to_string(),
                phrases: vec!["下一步".to_string()],
            },
        ];

        let raw_config = serde_json::to_string(&config).unwrap();
        let migrated = Config::from(raw_config);

        assert_eq!(migrated.quick_reply_rules.len(), 1);
        assert_eq!(migrated.quick_reply_rules[0].pattern, "愿意");
        assert_eq!(
            migrated.quick_reply_rules[0].phrases,
            vec!["愿意".to_string()]
        );
    }

    #[test]
    fn rejects_invalid_quick_reply_rule_regex() {
        let mut config = Config::default();
        config.quick_reply_rules = vec![QuickReplyRule {
            pattern: "(".to_string(),
            phrases: vec!["愿意".to_string()],
        }];

        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("invalid regex"));
    }

    #[test]
    fn rejects_invalid_execution_log_retention_days() {
        let mut config = Config::default();
        config.execution_log_retention_days = 3651;

        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("Execution log retention"));
    }

    #[test]
    fn rejects_invalid_execution_log_max_mb() {
        let mut config = Config::default();
        config.execution_log_max_mb = 1025;

        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("Execution log size cap"));
    }

    #[test]
    fn rejects_invalid_keep_latest_execution_logs_per_session() {
        let mut config = Config::default();
        config.keep_latest_execution_logs_per_session = 0;

        let err = config.validate().unwrap_err();
        assert!(
            err.to_string()
                .contains("Keep latest execution logs per session")
        );
    }
}
