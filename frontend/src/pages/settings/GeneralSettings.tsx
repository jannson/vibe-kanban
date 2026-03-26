import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { cloneDeep, merge, isEqual } from 'lodash';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Label } from '@/components/ui/label';
import { Input } from '@/components/ui/input';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Checkbox } from '@/components/ui/checkbox';
import { Loader2, Plus, Trash2, Volume2 } from 'lucide-react';
import {
  DEFAULT_PR_DESCRIPTION_PROMPT,
  type RemoteNotifierProjectFilter,
  type RemoteNotifierTarget,
  EditorType,
  type ReviewReadyNotificationStrategy,
  SoundFile,
  ThemeMode,
  UiLanguage,
} from 'shared/types';
import { getLanguageOptions } from '@/i18n/languages';

import { toPrettyCase } from '@/utils/string';
import { useEditorAvailability } from '@/hooks/useEditorAvailability';
import { EditorAvailabilityIndicator } from '@/components/EditorAvailabilityIndicator';
import { useTheme } from '@/components/ThemeProvider';
import { useUserSystem } from '@/components/ConfigProvider';
import { TagManager } from '@/components/TagManager';
import { configApi } from '@/lib/api';

export function GeneralSettings() {
  const { t } = useTranslation(['settings', 'common']);

  // Get language options with proper display names
  const languageOptions = getLanguageOptions(
    t('language.browserDefault', {
      ns: 'common',
      defaultValue: 'Browser Default',
    })
  );
  const {
    config,
    loading,
    updateAndSaveConfig, // Use this on Save
  } = useUserSystem();

  // Draft state management
  const [draft, setDraft] = useState(() => (config ? cloneDeep(config) : null));
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [testingRemoteTargetIndex, setTestingRemoteTargetIndex] = useState<
    number | null
  >(null);
  const [remoteTargetTestResults, setRemoteTargetTestResults] = useState<
    Record<number, { type: 'success' | 'error'; message: string }>
  >({});
  const [branchPrefixError, setBranchPrefixError] = useState<string | null>(
    null
  );
  const { setTheme } = useTheme();

  // Check editor availability when draft editor changes
  const editorAvailability = useEditorAvailability(draft?.editor.editor_type);

  const validateBranchPrefix = useCallback(
    (prefix: string): string | null => {
      if (!prefix) return null; // empty allowed
      if (prefix.includes('/'))
        return t('settings.general.git.branchPrefix.errors.slash');
      if (prefix.startsWith('.'))
        return t('settings.general.git.branchPrefix.errors.startsWithDot');
      if (prefix.endsWith('.') || prefix.endsWith('.lock'))
        return t('settings.general.git.branchPrefix.errors.endsWithDot');
      if (prefix.includes('..') || prefix.includes('@{'))
        return t('settings.general.git.branchPrefix.errors.invalidSequence');
      if (/[ \t~^:?*[\\]/.test(prefix))
        return t('settings.general.git.branchPrefix.errors.invalidChars');
      // Control chars check
      for (let i = 0; i < prefix.length; i++) {
        const code = prefix.charCodeAt(i);
        if (code < 0x20 || code === 0x7f)
          return t('settings.general.git.branchPrefix.errors.controlChars');
      }
      return null;
    },
    [t]
  );

  // When config loads or changes externally, update draft only if not dirty
  useEffect(() => {
    if (!config) return;
    if (!dirty) {
      setDraft(cloneDeep(config));
    }
  }, [config, dirty]);

  // Check for unsaved changes
  const hasUnsavedChanges = useMemo(() => {
    if (!draft || !config) return false;
    return !isEqual(draft, config);
  }, [draft, config]);

  // Generic draft update helper
  const updateDraft = useCallback(
    (patch: Partial<typeof config>) => {
      setDraft((prev: typeof config) => {
        if (!prev) return prev;
        const next = merge({}, prev, patch);
        // Mark dirty if changed
        if (!isEqual(next, config)) {
          setDirty(true);
        }
        return next;
      });
    },
    [config]
  );

  // Optional: warn on tab close/navigation with unsaved changes
  useEffect(() => {
    const handler = (e: BeforeUnloadEvent) => {
      if (hasUnsavedChanges) {
        e.preventDefault();
        e.returnValue = '';
      }
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [hasUnsavedChanges]);

  const playSound = async (soundFile: SoundFile) => {
    const audio = new Audio(`/api/sounds/${soundFile}`);
    try {
      await audio.play();
    } catch (err) {
      console.error('Failed to play sound:', err);
    }
  };

  const handleSave = async () => {
    if (!draft) return;

    setSaving(true);
    setError(null);
    setSuccess(false);

    try {
      await updateAndSaveConfig(draft); // Atomically apply + persist
      setTheme(draft.theme);
      setDirty(false);
      setSuccess(true);
      setTimeout(() => setSuccess(false), 3000);
    } catch (err) {
      setError(t('settings.general.save.error'));
      console.error('Error saving config:', err);
    } finally {
      setSaving(false);
    }
  };

  const handleDiscard = () => {
    if (!config) return;
    setDraft(cloneDeep(config));
    setDirty(false);
  };

  const resetDisclaimer = async () => {
    if (!config) return;
    updateAndSaveConfig({ disclaimer_acknowledged: false });
  };

  const resetOnboarding = async () => {
    if (!config) return;
    updateAndSaveConfig({ onboarding_acknowledged: false });
  };

  const parseTimeoutValue = (value: string): bigint | null => {
    const trimmed = value.trim();
    if (!trimmed) return null;
    const parsed = Number.parseInt(trimmed, 10);
    if (Number.isNaN(parsed)) return null;
    return parsed as unknown as bigint;
  };

  const formatTimeoutValue = (value: bigint | null | undefined): string => {
    if (value == null) return '';
    return String(value);
  };

  const updateRemoteNotifications = useCallback(
    (patch: Partial<NonNullable<typeof config>['remote_notifications']>) => {
      if (!draft) return;
      updateDraft({
        remote_notifications: {
          ...draft.remote_notifications,
          ...patch,
        },
      });
    },
    [draft, updateDraft]
  );

  const updateRemoteTarget = useCallback(
    (index: number, patch: Partial<RemoteNotifierTarget>) => {
      if (!draft) return;
      const nextTargets = draft.remote_notifications.targets.map((target, i) =>
        i === index ? { ...target, ...patch } : target
      );
      updateRemoteNotifications({ targets: nextTargets });
    },
    [draft, updateRemoteNotifications]
  );

  const createRemoteTargetId = (): string => {
    if (
      typeof globalThis.crypto !== 'undefined' &&
      typeof globalThis.crypto.randomUUID === 'function'
    ) {
      return globalThis.crypto.randomUUID();
    }

    return `remote-target-${Date.now()}-${Math.random()
      .toString(36)
      .slice(2, 10)}`;
  };

  const addRemoteTarget = useCallback(() => {
    if (!draft) return;
    const newTarget: RemoteNotifierTarget = {
      id: createRemoteTargetId(),
      enabled: true,
      label: null,
      url: '',
      token: null,
      projects: { type: 'all' },
      title_regex: null,
      timeout_ms: null,
      sound_enabled: true,
      desktop_enabled: false,
    };

    updateRemoteNotifications({
      targets: [...draft.remote_notifications.targets, newTarget],
    });
  }, [draft, updateRemoteNotifications]);

  const removeRemoteTarget = useCallback(
    (index: number) => {
      if (!draft) return;
      updateRemoteNotifications({
        targets: draft.remote_notifications.targets.filter((_, i) => i !== index),
      });
    },
    [draft, updateRemoteNotifications]
  );

  const formatProjectsValue = (projects: RemoteNotifierProjectFilter): string => {
    if (projects.type === 'all') return '';
    return projects.value.join(', ');
  };

  const parseProjectsValue = (value: string): RemoteNotifierProjectFilter => {
    const projectIds = value
      .split(',')
      .map((item) => item.trim())
      .filter(Boolean);
    if (projectIds.length === 0) {
      return { type: 'all' };
    }
    return { type: 'project_ids', value: projectIds };
  };

  const testRemoteTarget = useCallback(
    async (index: number) => {
      if (!draft) return;

      const target = draft.remote_notifications.targets[index];
      if (!target) return;

      setTestingRemoteTargetIndex(index);
      setRemoteTargetTestResults((prev) => {
        const next = { ...prev };
        delete next[index];
        return next;
      });

      try {
        const response = await configApi.testRemoteNotifierTarget({
          target,
          default_timeout_ms:
            draft.remote_notifications.default_timeout_ms ?? null,
        });

        setRemoteTargetTestResults((prev) => ({
          ...prev,
          [index]: {
            type: 'success',
            message: response.message,
          },
        }));
      } catch (err) {
        const message =
          err instanceof Error
            ? err.message
            : t('settings.general.remoteNotifications.targets.test.error', {
                defaultValue: 'Failed to test remote notifier target.',
              });

        setRemoteTargetTestResults((prev) => ({
          ...prev,
          [index]: {
            type: 'error',
            message,
          },
        }));
      } finally {
        setTestingRemoteTargetIndex(null);
      }
    },
    [draft, t]
  );

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8">
        <Loader2 className="h-8 w-8 animate-spin" />
        <span className="ml-2">{t('settings.general.loading')}</span>
      </div>
    );
  }

  if (!config) {
    return (
      <div className="py-8">
        <Alert variant="destructive">
          <AlertDescription>{t('settings.general.loadError')}</AlertDescription>
        </Alert>
      </div>
    );
  }

  const remoteNotifications = draft?.remote_notifications ?? config.remote_notifications;

  return (
    <div className="space-y-6">
      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {success && (
        <Alert variant="success">
          <AlertDescription className="font-medium">
            {t('settings.general.save.success')}
          </AlertDescription>
        </Alert>
      )}

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.general.appearance.title')}</CardTitle>
          <CardDescription>
            {t('settings.general.appearance.description')}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="theme">
              {t('settings.general.appearance.theme.label')}
            </Label>
            <Select
              value={draft?.theme}
              onValueChange={(value: ThemeMode) =>
                updateDraft({ theme: value })
              }
            >
              <SelectTrigger id="theme">
                <SelectValue
                  placeholder={t(
                    'settings.general.appearance.theme.placeholder'
                  )}
                />
              </SelectTrigger>
              <SelectContent>
                {Object.values(ThemeMode).map((theme) => (
                  <SelectItem key={theme} value={theme}>
                    {toPrettyCase(theme)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-sm text-muted-foreground">
              {t('settings.general.appearance.theme.helper')}
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="language">
              {t('settings.general.appearance.language.label')}
            </Label>
            <Select
              value={draft?.language}
              onValueChange={(value: UiLanguage) =>
                updateDraft({ language: value })
              }
            >
              <SelectTrigger id="language">
                <SelectValue
                  placeholder={t(
                    'settings.general.appearance.language.placeholder'
                  )}
                />
              </SelectTrigger>
              <SelectContent>
                {languageOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-sm text-muted-foreground">
              {t('settings.general.appearance.language.helper')}
            </p>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.general.editor.title')}</CardTitle>
          <CardDescription>
            {t('settings.general.editor.description')}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="editor-type">
              {t('settings.general.editor.type.label')}
            </Label>
            <Select
              value={draft?.editor.editor_type}
              onValueChange={(value: EditorType) =>
                updateDraft({
                  editor: { ...draft!.editor, editor_type: value },
                })
              }
            >
              <SelectTrigger id="editor-type">
                <SelectValue
                  placeholder={t('settings.general.editor.type.placeholder')}
                />
              </SelectTrigger>
              <SelectContent>
                {Object.values(EditorType).map((editor) => (
                  <SelectItem key={editor} value={editor}>
                    {toPrettyCase(editor)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {/* Editor availability status indicator */}
            {draft?.editor.editor_type !== EditorType.CUSTOM && (
              <EditorAvailabilityIndicator availability={editorAvailability} />
            )}

            <p className="text-sm text-muted-foreground">
              {t('settings.general.editor.type.helper')}
            </p>
          </div>

          {draft?.editor.editor_type === EditorType.CUSTOM && (
            <div className="space-y-2">
              <Label htmlFor="custom-command">
                {t('settings.general.editor.customCommand.label')}
              </Label>
              <Input
                id="custom-command"
                placeholder={t(
                  'settings.general.editor.customCommand.placeholder'
                )}
                value={draft?.editor.custom_command || ''}
                onChange={(e) =>
                  updateDraft({
                    editor: {
                      ...draft!.editor,
                      custom_command: e.target.value || null,
                    },
                  })
                }
              />
              <p className="text-sm text-muted-foreground">
                {t('settings.general.editor.customCommand.helper')}
              </p>
            </div>
          )}

          {(draft?.editor.editor_type === EditorType.VS_CODE ||
            draft?.editor.editor_type === EditorType.CURSOR ||
            draft?.editor.editor_type === EditorType.WINDSURF) && (
            <>
              <div className="space-y-2">
                <Label htmlFor="remote-ssh-host">
                  {t('settings.general.editor.remoteSsh.host.label')}
                </Label>
                <Input
                  id="remote-ssh-host"
                  placeholder={t(
                    'settings.general.editor.remoteSsh.host.placeholder'
                  )}
                  value={draft?.editor.remote_ssh_host || ''}
                  onChange={(e) =>
                    updateDraft({
                      editor: {
                        ...draft!.editor,
                        remote_ssh_host: e.target.value || null,
                      },
                    })
                  }
                />
                <p className="text-sm text-muted-foreground">
                  {t('settings.general.editor.remoteSsh.host.helper')}
                </p>
              </div>

              {draft?.editor.remote_ssh_host && (
                <div className="space-y-2">
                  <Label htmlFor="remote-ssh-user">
                    {t('settings.general.editor.remoteSsh.user.label')}
                  </Label>
                  <Input
                    id="remote-ssh-user"
                    placeholder={t(
                      'settings.general.editor.remoteSsh.user.placeholder'
                    )}
                    value={draft?.editor.remote_ssh_user || ''}
                    onChange={(e) =>
                      updateDraft({
                        editor: {
                          ...draft!.editor,
                          remote_ssh_user: e.target.value || null,
                        },
                      })
                    }
                  />
                  <p className="text-sm text-muted-foreground">
                    {t('settings.general.editor.remoteSsh.user.helper')}
                  </p>
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.general.git.title')}</CardTitle>
          <CardDescription>
            {t('settings.general.git.description')}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="git-branch-prefix">
              {t('settings.general.git.branchPrefix.label')}
            </Label>
            <Input
              id="git-branch-prefix"
              type="text"
              placeholder={t('settings.general.git.branchPrefix.placeholder')}
              value={draft?.git_branch_prefix ?? ''}
              onChange={(e) => {
                const value = e.target.value.trim();
                updateDraft({ git_branch_prefix: value });
                setBranchPrefixError(validateBranchPrefix(value));
              }}
              aria-invalid={!!branchPrefixError}
              className={branchPrefixError ? 'border-destructive' : undefined}
            />
            {branchPrefixError && (
              <p className="text-sm text-destructive">{branchPrefixError}</p>
            )}
            <p className="text-sm text-muted-foreground">
              {t('settings.general.git.branchPrefix.helper')}{' '}
              {draft?.git_branch_prefix ? (
                <>
                  {t('settings.general.git.branchPrefix.preview')}{' '}
                  <code className="text-xs bg-muted px-1 py-0.5 rounded">
                    {t('settings.general.git.branchPrefix.previewWithPrefix', {
                      prefix: draft.git_branch_prefix,
                    })}
                  </code>
                </>
              ) : (
                <>
                  {t('settings.general.git.branchPrefix.preview')}{' '}
                  <code className="text-xs bg-muted px-1 py-0.5 rounded">
                    {t('settings.general.git.branchPrefix.previewNoPrefix')}
                  </code>
                </>
              )}
            </p>
          </div>
          <div className="flex items-center space-x-2">
            <Checkbox
              id="default-use-original-repos"
              checked={draft?.default_use_original_repos ?? false}
              onCheckedChange={(checked: boolean) =>
                updateDraft({ default_use_original_repos: checked })
              }
            />
            <div className="space-y-0.5">
              <Label
                htmlFor="default-use-original-repos"
                className="cursor-pointer"
              >
                {t('settings.general.git.defaultOriginalRepos.label')}
              </Label>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.git.defaultOriginalRepos.helper')}
              </p>
            </div>
          </div>
          <div className="flex items-center space-x-2">
            <Checkbox
              id="auto-commit-enabled"
              checked={draft?.auto_commit_enabled ?? false}
              onCheckedChange={(checked: boolean) =>
                updateDraft({ auto_commit_enabled: checked })
              }
            />
            <div className="space-y-0.5">
              <Label htmlFor="auto-commit-enabled" className="cursor-pointer">
                {t('settings.general.git.autoCommit.label')}
              </Label>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.git.autoCommit.helper')}
              </p>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.general.pullRequests.title')}</CardTitle>
          <CardDescription>
            {t('settings.general.pullRequests.description')}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center space-x-2">
            <Checkbox
              id="pr-auto-description"
              checked={draft?.pr_auto_description_enabled ?? false}
              onCheckedChange={(checked: boolean) =>
                updateDraft({ pr_auto_description_enabled: checked })
              }
            />
            <div className="space-y-0.5">
              <Label htmlFor="pr-auto-description" className="cursor-pointer">
                {t('settings.general.pullRequests.autoDescription.label')}
              </Label>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.pullRequests.autoDescription.helper')}
              </p>
            </div>
          </div>
          <div className="flex items-center space-x-2">
            <Checkbox
              id="use-custom-prompt"
              checked={draft?.pr_auto_description_prompt != null}
              onCheckedChange={(checked: boolean) => {
                if (checked) {
                  updateDraft({
                    pr_auto_description_prompt: DEFAULT_PR_DESCRIPTION_PROMPT,
                  });
                } else {
                  updateDraft({ pr_auto_description_prompt: null });
                }
              }}
            />
            <Label htmlFor="use-custom-prompt" className="cursor-pointer">
              {t('settings.general.pullRequests.customPrompt.useCustom')}
            </Label>
          </div>
          <div className="space-y-2">
            <textarea
              id="pr-custom-prompt"
              className={`flex min-h-[100px] w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ${
                draft?.pr_auto_description_prompt == null
                  ? 'opacity-50 cursor-not-allowed'
                  : ''
              }`}
              value={
                draft?.pr_auto_description_prompt ??
                DEFAULT_PR_DESCRIPTION_PROMPT
              }
              disabled={draft?.pr_auto_description_prompt == null}
              onChange={(e) =>
                updateDraft({
                  pr_auto_description_prompt: e.target.value,
                })
              }
            />
            <p className="text-sm text-muted-foreground">
              {t('settings.general.pullRequests.customPrompt.helper')}
            </p>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.general.notifications.title')}</CardTitle>
          <CardDescription>
            {t('settings.general.notifications.description')}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center space-x-2">
            <Checkbox
              id="sound-enabled"
              checked={draft?.notifications.sound_enabled}
              onCheckedChange={(checked: boolean) =>
                updateDraft({
                  notifications: {
                    ...draft!.notifications,
                    sound_enabled: checked,
                  },
                })
              }
            />
            <div className="space-y-0.5">
              <Label htmlFor="sound-enabled" className="cursor-pointer">
                {t('settings.general.notifications.sound.label')}
              </Label>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.notifications.sound.helper')}
              </p>
            </div>
          </div>
          {draft?.notifications.sound_enabled && (
            <div className="ml-6 space-y-2">
              <Label htmlFor="sound-file">
                {t('settings.general.notifications.sound.fileLabel')}
              </Label>
              <div className="flex gap-2">
                <Select
                  value={draft.notifications.sound_file}
                  onValueChange={(value: SoundFile) =>
                    updateDraft({
                      notifications: {
                        ...draft.notifications,
                        sound_file: value,
                      },
                    })
                  }
                >
                  <SelectTrigger id="sound-file" className="flex-1">
                    <SelectValue
                      placeholder={t(
                        'settings.general.notifications.sound.filePlaceholder'
                      )}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {Object.values(SoundFile).map((soundFile) => (
                      <SelectItem key={soundFile} value={soundFile}>
                        {toPrettyCase(soundFile)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => playSound(draft.notifications.sound_file)}
                  className="px-3"
                >
                  <Volume2 className="h-4 w-4" />
                </Button>
              </div>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.notifications.sound.fileHelper')}
              </p>
            </div>
          )}
          <div className="flex items-center space-x-2">
            <Checkbox
              id="push-notifications"
              checked={draft?.notifications.push_enabled}
              onCheckedChange={(checked: boolean) =>
                updateDraft({
                  notifications: {
                    ...draft!.notifications,
                    push_enabled: checked,
                  },
                })
              }
            />
            <div className="space-y-0.5">
              <Label htmlFor="push-notifications" className="cursor-pointer">
                {t('settings.general.notifications.push.label')}
              </Label>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.notifications.push.helper')}
              </p>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>
            {t('settings.general.remoteNotifications.title', {
              defaultValue: 'Remote Notifiers',
            })}
          </CardTitle>
          <CardDescription>
            {t('settings.general.remoteNotifications.description', {
              defaultValue:
                'Route task completion notifications to local notifier endpoints such as SSH reverse-tunneled computers.',
            })}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center space-x-2">
            <Checkbox
              id="remote-notifications-enabled"
              checked={draft?.remote_notifications.enabled ?? false}
              onCheckedChange={(checked: boolean) =>
                updateRemoteNotifications({ enabled: checked })
              }
            />
            <div className="space-y-0.5">
              <Label
                htmlFor="remote-notifications-enabled"
                className="cursor-pointer"
              >
                {t('settings.general.remoteNotifications.enabled.label', {
                  defaultValue: 'Enable Remote Notifiers',
                })}
              </Label>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.remoteNotifications.enabled.helper', {
                  defaultValue:
                    'Send review-ready events to configured local notifier endpoints.',
                })}
              </p>
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="remote-default-timeout">
              {t('settings.general.remoteNotifications.defaultTimeout.label', {
                defaultValue: 'Default Timeout (ms)',
              })}
            </Label>
            <Input
              id="remote-default-timeout"
              type="number"
              min="1"
              max="60000"
              value={formatTimeoutValue(
                draft?.remote_notifications.default_timeout_ms
              )}
              onChange={(e) =>
                updateRemoteNotifications({
                  default_timeout_ms:
                    (parseTimeoutValue(e.target.value) ??
                      (1500 as unknown as bigint)) as unknown as bigint,
                })
              }
            />
            <p className="text-sm text-muted-foreground">
              {t('settings.general.remoteNotifications.defaultTimeout.helper', {
                defaultValue:
                  'Fallback request timeout used when a target-specific timeout is not set.',
              })}
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="review-ready-notification-strategy">
              {t('settings.general.remoteNotifications.strategy.label', {
                defaultValue: 'Review-Ready Notification Strategy',
              })}
            </Label>
            <Select
              value={
                draft?.review_ready_notification_strategy ?? 'LOCAL_ONLY'
              }
              onValueChange={(value: ReviewReadyNotificationStrategy) =>
                updateDraft({
                  review_ready_notification_strategy: value,
                })
              }
            >
              <SelectTrigger id="review-ready-notification-strategy">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="LOCAL_ONLY">
                  {t('settings.general.remoteNotifications.strategy.localOnly', {
                    defaultValue: 'Local Only',
                  })}
                </SelectItem>
                <SelectItem value="REMOTE_ONLY">
                  {t('settings.general.remoteNotifications.strategy.remoteOnly', {
                    defaultValue: 'Remote Only',
                  })}
                </SelectItem>
                <SelectItem value="BOTH">
                  {t('settings.general.remoteNotifications.strategy.both', {
                    defaultValue: 'Both',
                  })}
                </SelectItem>
              </SelectContent>
            </Select>
            <p className="text-sm text-muted-foreground">
              {t('settings.general.remoteNotifications.strategy.helper', {
                defaultValue:
                  'Choose whether review-ready events notify on the server host, the local notifier, or both.',
              })}
            </p>
          </div>

          <div className="flex items-center justify-between">
            <div>
              <p className="font-medium">
                {t('settings.general.remoteNotifications.targets.title', {
                  defaultValue: 'Targets',
                })}
              </p>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.remoteNotifications.targets.helper', {
                  defaultValue:
                    'Each target can filter by project IDs and task title regex.',
                })}
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={addRemoteTarget}>
              <Plus className="mr-2 h-4 w-4" />
              {t('settings.general.remoteNotifications.targets.add', {
                defaultValue: 'Add Target',
              })}
            </Button>
          </div>

          <div className="space-y-4">
            {remoteNotifications.targets.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                {t('settings.general.remoteNotifications.targets.empty', {
                  defaultValue: 'No remote notifier targets configured.',
                })}
              </p>
            ) : (
              remoteNotifications.targets.map((target, index) => (
                <div
                  key={target.id || index}
                  className="rounded-lg border p-4 space-y-4"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="flex items-center space-x-2">
                      <Checkbox
                        id={`remote-target-enabled-${index}`}
                        checked={target.enabled}
                        onCheckedChange={(checked: boolean) =>
                          updateRemoteTarget(index, { enabled: checked })
                        }
                      />
                      <div className="space-y-0.5">
                        <Label
                          htmlFor={`remote-target-enabled-${index}`}
                          className="cursor-pointer"
                        >
                          {target.label ||
                            target.id ||
                            t(
                              'settings.general.remoteNotifications.targets.unnamed',
                              {
                                defaultValue: 'Unnamed target',
                              }
                            )}
                        </Label>
                        <p className="text-xs text-muted-foreground">
                          {t('settings.general.remoteNotifications.targets.id', {
                            defaultValue: 'ID',
                          })}
                          : {target.id}
                        </p>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => testRemoteTarget(index)}
                        disabled={testingRemoteTargetIndex === index}
                      >
                        {testingRemoteTargetIndex === index && (
                          <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                        )}
                        {t('settings.general.remoteNotifications.targets.test.label', {
                          defaultValue: 'Test',
                        })}
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => removeRemoteTarget(index)}
                        aria-label={t(
                          'settings.general.remoteNotifications.targets.remove',
                          {
                            defaultValue: 'Remove target',
                          }
                        )}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  </div>

                  {remoteTargetTestResults[index] && (
                    <Alert
                      variant={
                        remoteTargetTestResults[index].type === 'success'
                          ? 'success'
                          : 'destructive'
                      }
                    >
                      <AlertDescription>
                        {remoteTargetTestResults[index].message}
                      </AlertDescription>
                    </Alert>
                  )}

                  <div className="grid gap-4 md:grid-cols-2">
                    <div className="space-y-2">
                      <Label>{t('settings.general.remoteNotifications.fields.id', {
                        defaultValue: 'Target ID',
                      })}</Label>
                      <Input
                        value={target.id}
                        onChange={(e) =>
                          updateRemoteTarget(index, { id: e.target.value })
                        }
                      />
                    </div>

                    <div className="space-y-2">
                      <Label>{t('settings.general.remoteNotifications.fields.label', {
                        defaultValue: 'Label',
                      })}</Label>
                      <Input
                        value={target.label ?? ''}
                        onChange={(e) =>
                          updateRemoteTarget(index, {
                            label: e.target.value || null,
                          })
                        }
                        placeholder={t(
                          'settings.general.remoteNotifications.fields.labelPlaceholder',
                          {
                            defaultValue: 'e.g. Janson MacBook',
                          }
                        )}
                      />
                    </div>

                    <div className="space-y-2 md:col-span-2">
                      <Label>{t('settings.general.remoteNotifications.fields.url', {
                        defaultValue: 'URL',
                      })}</Label>
                      <Input
                        value={target.url}
                        onChange={(e) =>
                          updateRemoteTarget(index, { url: e.target.value })
                        }
                        placeholder="http://127.0.0.1:43110/notify"
                      />
                    </div>

                    <div className="space-y-2">
                      <Label>{t('settings.general.remoteNotifications.fields.token', {
                        defaultValue: 'Token',
                      })}</Label>
                      <Input
                        value={target.token ?? ''}
                        onChange={(e) =>
                          updateRemoteTarget(index, {
                            token: e.target.value || null,
                          })
                        }
                        placeholder={t(
                          'settings.general.remoteNotifications.fields.tokenPlaceholder',
                          {
                            defaultValue: 'Bearer token for notifier auth',
                          }
                        )}
                      />
                    </div>

                    <div className="space-y-2">
                      <Label>
                        {t('settings.general.remoteNotifications.fields.timeout', {
                          defaultValue: 'Timeout (ms)',
                        })}
                      </Label>
                      <Input
                        type="number"
                        min="1"
                        max="60000"
                        value={formatTimeoutValue(target.timeout_ms)}
                        onChange={(e) =>
                          updateRemoteTarget(index, {
                            timeout_ms: parseTimeoutValue(
                              e.target.value
                            ) as unknown as bigint | null,
                          })
                        }
                        placeholder={formatTimeoutValue(
                          remoteNotifications.default_timeout_ms
                        )}
                      />
                    </div>

                    <div className="space-y-2 md:col-span-2">
                      <Label>
                        {t(
                          'settings.general.remoteNotifications.fields.projects',
                          {
                            defaultValue: 'Project IDs',
                          }
                        )}
                      </Label>
                      <Input
                        value={formatProjectsValue(target.projects)}
                        onChange={(e) =>
                          updateRemoteTarget(index, {
                            projects: parseProjectsValue(e.target.value),
                          })
                        }
                        placeholder={t(
                          'settings.general.remoteNotifications.fields.projectsPlaceholder',
                          {
                            defaultValue:
                              'Leave empty for ALL, or enter comma-separated project IDs',
                          }
                        )}
                      />
                    </div>

                    <div className="space-y-2 md:col-span-2">
                      <Label>
                        {t(
                          'settings.general.remoteNotifications.fields.titleRegex',
                          {
                            defaultValue: 'Task Title Regex',
                          }
                        )}
                      </Label>
                      <Input
                        value={target.title_regex ?? ''}
                        onChange={(e) =>
                          updateRemoteTarget(index, {
                            title_regex: e.target.value || null,
                          })
                        }
                        placeholder={t(
                          'settings.general.remoteNotifications.fields.titleRegexPlaceholder',
                          {
                            defaultValue:
                              'Optional regex, e.g. ^(urgent|prod):',
                          }
                        )}
                      />
                    </div>
                  </div>

                  <div className="flex flex-wrap gap-4">
                    <div className="flex items-center space-x-2">
                      <Checkbox
                        id={`remote-target-sound-${index}`}
                        checked={target.sound_enabled}
                        onCheckedChange={(checked: boolean) =>
                          updateRemoteTarget(index, { sound_enabled: checked })
                        }
                      />
                      <Label
                        htmlFor={`remote-target-sound-${index}`}
                        className="cursor-pointer"
                      >
                        {t('settings.general.remoteNotifications.fields.sound', {
                          defaultValue: 'Sound',
                        })}
                      </Label>
                    </div>

                    <div className="flex items-center space-x-2">
                      <Checkbox
                        id={`remote-target-desktop-${index}`}
                        checked={target.desktop_enabled}
                        onCheckedChange={(checked: boolean) =>
                          updateRemoteTarget(index, {
                            desktop_enabled: checked,
                          })
                        }
                      />
                      <Label
                        htmlFor={`remote-target-desktop-${index}`}
                        className="cursor-pointer"
                      >
                        {t(
                          'settings.general.remoteNotifications.fields.desktop',
                          {
                            defaultValue: 'Desktop Notification',
                          }
                        )}
                      </Label>
                    </div>
                  </div>

                  <p className="text-xs text-muted-foreground">
                    {t(
                      'settings.general.remoteNotifications.fields.desktopHelper',
                      {
                        defaultValue:
                          'Requires the local vk-notifier process to be started with --desktop-enabled.',
                      }
                    )}
                  </p>
                </div>
              ))
            )}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.general.privacy.title')}</CardTitle>
          <CardDescription>
            {t('settings.general.privacy.description')}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center space-x-2">
            <Checkbox
              id="analytics-enabled"
              checked={draft?.analytics_enabled ?? false}
              onCheckedChange={(checked: boolean) =>
                updateDraft({ analytics_enabled: checked })
              }
            />
            <div className="space-y-0.5">
              <Label htmlFor="analytics-enabled" className="cursor-pointer">
                {t('settings.general.privacy.telemetry.label')}
              </Label>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.privacy.telemetry.helper')}
              </p>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.general.taskTemplates.title')}</CardTitle>
          <CardDescription>
            {t('settings.general.taskTemplates.description')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <TagManager />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.general.safety.title')}</CardTitle>
          <CardDescription>
            {t('settings.general.safety.description')}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <p className="font-medium">
                {t('settings.general.safety.disclaimer.title')}
              </p>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.safety.disclaimer.description')}
              </p>
            </div>
            <Button variant="outline" onClick={resetDisclaimer}>
              {t('settings.general.safety.disclaimer.button')}
            </Button>
          </div>
          <div className="flex items-center justify-between">
            <div>
              <p className="font-medium">
                {t('settings.general.safety.onboarding.title')}
              </p>
              <p className="text-sm text-muted-foreground">
                {t('settings.general.safety.onboarding.description')}
              </p>
            </div>
            <Button variant="outline" onClick={resetOnboarding}>
              {t('settings.general.safety.onboarding.button')}
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* Sticky Save Button */}
      <div className="sticky bottom-0 z-10 bg-background/80 backdrop-blur-sm border-t py-4">
        <div className="flex items-center justify-between">
          {hasUnsavedChanges ? (
            <span className="text-sm text-muted-foreground">
              {t('settings.general.save.unsavedChanges')}
            </span>
          ) : (
            <span />
          )}
          <div className="flex gap-2">
            <Button
              variant="outline"
              onClick={handleDiscard}
              disabled={!hasUnsavedChanges || saving}
            >
              {t('settings.general.save.discard')}
            </Button>
            <Button
              onClick={handleSave}
              disabled={!hasUnsavedChanges || saving || !!branchPrefixError}
            >
              {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {t('settings.general.save.button')}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
