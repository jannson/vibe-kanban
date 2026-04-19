import {
  Loader2,
  Send,
  StopCircle,
  AlertCircle,
  Clock,
  X,
  Paperclip,
  Sparkles,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription } from '@/components/ui/alert';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
//
import { useEffect, useMemo, useRef, useState, useCallback } from 'react';
import { ScratchType, type TaskWithAttemptStatus } from 'shared/types';
import { useBranchStatus } from '@/hooks';
import { useAttemptExecution } from '@/hooks/useAttemptExecution';
import { useUserSystem } from '@/components/ConfigProvider';
import { cn } from '@/lib/utils';
//
import { useReview } from '@/contexts/ReviewProvider';
import { useClickedElements } from '@/contexts/ClickedElementsProvider';
import { useEntries } from '@/contexts/EntriesContext';
import { useProject } from '@/contexts/ProjectContext';
//
import { VariantSelector } from '@/components/tasks/VariantSelector';
import { useAttemptBranch } from '@/hooks/useAttemptBranch';
import { FollowUpConflictSection } from '@/components/tasks/follow-up/FollowUpConflictSection';
import { ClickedElementsBanner } from '@/components/tasks/ClickedElementsBanner';
import WYSIWYGEditor from '@/components/ui/wysiwyg';
import { useRetryUi } from '@/contexts/RetryUiContext';
import { useFollowUpSend } from '@/hooks/useFollowUpSend';
import { useVariant } from '@/hooks/useVariant';
import type {
  DraftFollowUpData,
  ExecutorAction,
  ExecutorProfileId,
  QuickReplyRule,
} from 'shared/types';
import { buildResolveConflictsInstructions } from '@/lib/conflicts';
import { useTranslation } from 'react-i18next';
import { useScratch } from '@/hooks/useScratch';
import { useDebouncedCallback } from '@/hooks/useDebouncedCallback';
import { useQueueStatus } from '@/hooks/useQueueStatus';
import { imagesApi } from '@/lib/api';
import type { Session } from 'shared/types';

const LAST_QUICK_REPLY_STORAGE_KEY = 'task-follow-up-last-quick-reply';

function extractLastSentence(text: string): string {
  const compact = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .pop();

  if (!compact) return '';

  const segments = compact
    .split(/(?<=[。！？!?])/)
    .map((segment) => segment.trim())
    .filter(Boolean);

  return segments[segments.length - 1] ?? compact;
}

function normalizeQuickReplyText(text: string): string {
  return text.trim().toLowerCase();
}

function getQuickReplyRulePriority(
  phrase: string,
  query: string,
  rules: QuickReplyRule[]
): number {
  const normalizedPhrase = normalizeQuickReplyText(phrase);
  const normalizedQuery = normalizeQuickReplyText(query);

  if (!normalizedPhrase || !normalizedQuery) return 0;

  let priority = 0;

  if (normalizedQuery.includes(normalizedPhrase)) {
    priority += 500;
  }

  rules.forEach((rule, index) => {
    const pattern = rule.pattern.trim();

    if (!pattern) {
      return;
    }

    let regex: RegExp;
    try {
      regex = new RegExp(pattern);
    } catch {
      return;
    }

    if (!regex.test(query)) {
      return;
    }

    const matchesRule = rule.phrases.some((token: string) => {
      const normalizedToken = normalizeQuickReplyText(token);
      return (
        normalizedPhrase === normalizedToken ||
        normalizedPhrase.includes(normalizedToken) ||
        normalizedToken.includes(normalizedPhrase)
      );
    });

    if (matchesRule) {
      priority += 1_000 - index * 10;
    }
  });

  return priority;
}

function appendQuickPhrase(base: string, phrase: string): string {
  const trimmed = base.trimEnd();
  if (!trimmed) {
    return phrase;
  }

  if (trimmed.endsWith('\n')) {
    return `${trimmed}${phrase}`;
  }

  return `${trimmed}\n${phrase}`;
}

interface TaskFollowUpSectionProps {
  task: TaskWithAttemptStatus;
  session?: Session;
  gitEnabled?: boolean;
}

export function TaskFollowUpSection({
  task,
  session,
  gitEnabled = true,
}: TaskFollowUpSectionProps) {
  const { t } = useTranslation('tasks');
  const { projectId } = useProject();

  // Derive IDs from session
  const workspaceId = session?.workspace_id;
  const sessionId = session?.id;

  const { isAttemptRunning, stopExecution, isStopping, processes } =
    useAttemptExecution(workspaceId, task.id);

  const { data: branchStatus, refetch: refetchBranchStatus } =
    useBranchStatus(workspaceId, { enabled: gitEnabled });

  const repoWithConflicts = useMemo(
    () =>
      branchStatus?.find(
        (r) => r.is_rebase_in_progress || (r.conflicted_files?.length ?? 0) > 0
      ),
    [branchStatus]
  );
  const { branch: attemptBranch, refetch: refetchAttemptBranch } =
    useAttemptBranch(workspaceId);
  const { profiles, config } = useUserSystem();
  const { comments, generateReviewMarkdown, clearComments } = useReview();
  const {
    generateMarkdown: generateClickedMarkdown,
    clearElements: clearClickedElements,
  } = useClickedElements();

  const reviewMarkdown = useMemo(
    () => generateReviewMarkdown(),
    [generateReviewMarkdown]
  );

  const clickedMarkdown = useMemo(
    () => generateClickedMarkdown(),
    [generateClickedMarkdown]
  );

  // Non-editable conflict resolution instructions (derived, like review comments)
  const conflictResolutionInstructions = useMemo(() => {
    if (!repoWithConflicts?.conflicted_files?.length) return null;
    return buildResolveConflictsInstructions(
      attemptBranch,
      repoWithConflicts.target_branch_name,
      repoWithConflicts.conflicted_files,
      repoWithConflicts.conflict_op ?? null,
      repoWithConflicts.repo_name
    );
  }, [attemptBranch, repoWithConflicts]);

  // Editor state (persisted via scratch)
  const {
    scratch,
    updateScratch,
    isLoading: isScratchLoading,
  } = useScratch(ScratchType.DRAFT_FOLLOW_UP, sessionId ?? '');

  // Derive the message and variant from scratch
  const scratchData: DraftFollowUpData | undefined =
    scratch?.payload?.type === 'DRAFT_FOLLOW_UP'
      ? scratch.payload.data
      : undefined;

  // Track whether the follow-up textarea is focused
  const [isTextareaFocused, setIsTextareaFocused] = useState(false);

  // Local message state for immediate UI feedback (before debounced save)
  const [localMessage, setLocalMessage] = useState('');

  // Variant selection - derive default from latest process
  const latestProfileId = useMemo<ExecutorProfileId | null>(() => {
    if (!processes?.length) return null;

    const extractProfile = (
      action: ExecutorAction | null
    ): ExecutorProfileId | null => {
      let curr: ExecutorAction | null = action;
      while (curr) {
        const typ = curr.typ;
        switch (typ.type) {
          case 'CodingAgentInitialRequest':
          case 'CodingAgentFollowUpRequest':
            return typ.executor_profile_id;
          case 'ScriptRequest':
            curr = curr.next_action;
            continue;
        }
      }
      return null;
    };
    return (
      processes
        .slice()
        .reverse()
        .map((p) => extractProfile(p.executor_action ?? null))
        .find((pid) => pid !== null) ?? null
    );
  }, [processes]);

  const processVariant = latestProfileId?.variant ?? null;

  const currentProfile = useMemo(() => {
    if (!latestProfileId) return null;
    return profiles?.[latestProfileId.executor] ?? null;
  }, [latestProfileId, profiles]);

  // Variant selection with priority: user selection > scratch > process
  const { selectedVariant, setSelectedVariant: setVariantFromHook } =
    useVariant({
      processVariant,
      scratchVariant: scratchData?.variant,
    });

  // Ref to track current variant for use in message save callback
  const variantRef = useRef<string | null>(selectedVariant);
  useEffect(() => {
    variantRef.current = selectedVariant;
  }, [selectedVariant]);

  // Refs to stabilize callbacks - avoid re-creating callbacks when these values change
  const scratchRef = useRef(scratch);
  useEffect(() => {
    scratchRef.current = scratch;
  }, [scratch]);

  // Save scratch helper (used for both message and variant changes)
  // Uses scratchRef to avoid callback invalidation when scratch updates
  const saveToScratch = useCallback(
    async (message: string, variant: string | null) => {
      if (!workspaceId) return;
      // Don't create empty scratch entries - only save if there's actual content,
      // a variant is selected, or scratch already exists (to allow clearing a draft)
      if (!message.trim() && !variant && !scratchRef.current) return;
      try {
        await updateScratch({
          payload: {
            type: 'DRAFT_FOLLOW_UP',
            data: { message, variant },
          },
        });
      } catch (e) {
        console.error('Failed to save follow-up draft', e);
      }
    },
    [workspaceId, updateScratch]
  );

  // Wrapper to update variant and save to scratch immediately
  const setSelectedVariant = useCallback(
    (variant: string | null) => {
      setVariantFromHook(variant);
      // Save immediately when user changes variant
      saveToScratch(localMessage, variant);
    },
    [setVariantFromHook, saveToScratch, localMessage]
  );

  // Debounced save for message changes (uses current variant from ref)
  const { debounced: setFollowUpMessage, cancel: cancelDebouncedSave } =
    useDebouncedCallback(
      useCallback(
        (value: string) => saveToScratch(value, variantRef.current),
        [saveToScratch]
      ),
      500
    );

  // Sync local message from scratch when it loads (but not while user is typing)
  useEffect(() => {
    if (isScratchLoading) return;
    if (isTextareaFocused) return; // Don't overwrite while user is typing
    setLocalMessage(scratchData?.message ?? '');
  }, [isScratchLoading, scratchData?.message, isTextareaFocused]);

  // During retry, follow-up box is greyed/disabled (not hidden)
  // Use RetryUi context so optimistic retry immediately disables this box
  const { activeRetryProcessId } = useRetryUi();
  const isRetryActive = !!activeRetryProcessId;

  // Queue status for queuing follow-up messages while agent is running
  const {
    isQueued,
    queuedMessage,
    isLoading: isQueueLoading,
    queueMessage,
    cancelQueue,
    refresh: refreshQueueStatus,
  } = useQueueStatus(sessionId);

  // Track previous process count to detect new processes
  const prevProcessCountRef = useRef(processes.length);

  // Refresh queue status when execution stops OR when a new process starts
  useEffect(() => {
    const prevCount = prevProcessCountRef.current;
    prevProcessCountRef.current = processes.length;

    if (!workspaceId) return;

    // Refresh when execution stops
    if (!isAttemptRunning) {
      refreshQueueStatus();
      return;
    }

    // Refresh when a new process starts (could be queued message consumption or follow-up)
    if (processes.length > prevCount) {
      refreshQueueStatus();
      // Re-sync local message from current scratch state
      // If scratch was deleted, scratchData will be undefined, so localMessage becomes ''
      setLocalMessage(scratchData?.message ?? '');
    }
  }, [
    isAttemptRunning,
    workspaceId,
    processes.length,
    refreshQueueStatus,
    scratchData?.message,
  ]);

  // When queued, display the queued message content so user can edit it
  const displayMessage =
    isQueued && queuedMessage ? queuedMessage.data.message : localMessage;

  // Check if there's a pending approval - users shouldn't be able to type during approvals
  const { entries } = useEntries();
  const hasPendingApproval = useMemo(() => {
    return entries.some((entry) => {
      if (entry.type !== 'NORMALIZED_ENTRY') return false;
      const entryType = entry.content.entry_type;
      return (
        entryType.type === 'tool_use' &&
        entryType.status.status === 'pending_approval'
      );
    });
  }, [entries]);

  const [lastQuickReply, setLastQuickReply] = useState(() => {
    return localStorage.getItem(LAST_QUICK_REPLY_STORAGE_KEY) ?? '';
  });

  const configuredQuickPhrases = useMemo(
    () => config?.quick_reply_phrases ?? [],
    [config?.quick_reply_phrases]
  );
  const configuredQuickReplyRules = useMemo(
    () => config?.quick_reply_rules ?? [],
    [config?.quick_reply_rules]
  );

  const rankedQuickPhrases = useMemo(() => {
    const query = extractLastSentence(displayMessage) || displayMessage.trim();

    return configuredQuickPhrases
      .map((phrase, index) => ({
        phrase,
        index,
        priority:
          getQuickReplyRulePriority(phrase, query, configuredQuickReplyRules) +
          (phrase === lastQuickReply ? 10_000 : 0),
      }))
      .sort((left, right) => {
        if (right.priority !== left.priority) {
          return right.priority - left.priority;
        }
        return left.index - right.index;
      })
      .map((item) => item.phrase);
  }, [
    configuredQuickPhrases,
    configuredQuickReplyRules,
    displayMessage,
    lastQuickReply,
  ]);

  // Send follow-up action
  const { isSendingFollowUp, followUpError, setFollowUpError, onSendFollowUp } =
    useFollowUpSend({
      sessionId,
      message: localMessage,
      conflictMarkdown: conflictResolutionInstructions,
      reviewMarkdown,
      clickedMarkdown,
      selectedVariant,
      clearComments,
      clearClickedElements,
      onAfterSendCleanup: () => {
        cancelDebouncedSave(); // Cancel any pending debounced save to avoid race condition
        setLocalMessage(''); // Clear local state immediately
        // Scratch deletion is handled by the backend when the queued message is consumed
      },
    });

  // Separate logic for when textarea should be disabled vs when send button should be disabled
  const canTypeFollowUp = useMemo(() => {
    if (!workspaceId || processes.length === 0 || isSendingFollowUp) {
      return false;
    }

    if (isRetryActive) return false; // disable typing while retry editor is active
    if (hasPendingApproval) return false; // disable typing during approval
    // Note: isQueued no longer blocks typing - editing auto-cancels the queue
    return true;
  }, [
    workspaceId,
    processes.length,
    isSendingFollowUp,
    isRetryActive,
    hasPendingApproval,
  ]);

  const canSendFollowUp = useMemo(() => {
    if (!canTypeFollowUp) {
      return false;
    }

    // Allow sending if conflict instructions, review comments, clicked elements, or message is present
    return Boolean(
      conflictResolutionInstructions ||
        reviewMarkdown ||
        clickedMarkdown ||
        localMessage.trim()
    );
  }, [
    canTypeFollowUp,
    conflictResolutionInstructions,
    reviewMarkdown,
    clickedMarkdown,
    localMessage,
  ]);
  const isEditable = !isRetryActive && !hasPendingApproval;

  const hasFollowUpContent = useMemo(
    () =>
      Boolean(
        localMessage.trim() ||
          conflictResolutionInstructions ||
          reviewMarkdown ||
          clickedMarkdown
      ),
    [
      localMessage,
      conflictResolutionInstructions,
      reviewMarkdown,
      clickedMarkdown,
    ]
  );

  // Handler to queue the current message for execution after agent finishes
  const handleQueueMessage = useCallback(async () => {
    if (
      !localMessage.trim() &&
      !conflictResolutionInstructions &&
      !reviewMarkdown &&
      !clickedMarkdown
    ) {
      return;
    }

    // Cancel any pending debounced save and save immediately before queueing
    // This prevents the race condition where the debounce fires after queueing
    cancelDebouncedSave();
    await saveToScratch(localMessage, selectedVariant);

    // Combine all the content that would be sent (same as follow-up send)
    const parts = [
      conflictResolutionInstructions,
      clickedMarkdown,
      reviewMarkdown,
      localMessage,
    ].filter(Boolean);
    const combinedMessage = parts.join('\n\n');
    await queueMessage(combinedMessage, selectedVariant);
  }, [
    localMessage,
    conflictResolutionInstructions,
    reviewMarkdown,
    clickedMarkdown,
    selectedVariant,
    queueMessage,
    cancelDebouncedSave,
    saveToScratch,
  ]);

  const handlePrimaryEnterAction = useCallback(() => {
    if (!isEditable) {
      return;
    }

    if (isAttemptRunning) {
      if (!isQueueLoading && hasFollowUpContent) {
        void handleQueueMessage();
      }
      return;
    }

    if (canSendFollowUp && !isSendingFollowUp) {
      void onSendFollowUp();
    }
  }, [
    isEditable,
    isAttemptRunning,
    isQueueLoading,
    hasFollowUpContent,
    handleQueueMessage,
    canSendFollowUp,
    isSendingFollowUp,
    onSendFollowUp,
  ]);

  // Ref to access setFollowUpMessage without adding it as a dependency
  const setFollowUpMessageRef = useRef(setFollowUpMessage);
  useEffect(() => {
    setFollowUpMessageRef.current = setFollowUpMessage;
  }, [setFollowUpMessage]);

  // Ref for followUpError to use in stable onChange handler
  const followUpErrorRef = useRef(followUpError);
  useEffect(() => {
    followUpErrorRef.current = followUpError;
  }, [followUpError]);

  // Refs for queue state to use in stable onChange handler
  const isQueuedRef = useRef(isQueued);
  useEffect(() => {
    isQueuedRef.current = isQueued;
  }, [isQueued]);

  const cancelQueueRef = useRef(cancelQueue);
  useEffect(() => {
    cancelQueueRef.current = cancelQueue;
  }, [cancelQueue]);

  const queuedMessageRef = useRef(queuedMessage);
  useEffect(() => {
    queuedMessageRef.current = queuedMessage;
  }, [queuedMessage]);

  // Handle image paste - upload to container and insert markdown
  const handlePasteFiles = useCallback(
    async (files: File[]) => {
      if (!workspaceId) return;

      for (const file of files) {
        try {
          const response = await imagesApi.uploadForAttempt(workspaceId, file);
          // Append markdown image to current message
          const imageMarkdown = `![${response.original_name}](${response.file_path})`;

          // If queued, cancel queue and use queued message as base (same as editor change behavior)
          if (isQueuedRef.current && queuedMessageRef.current) {
            cancelQueueRef.current();
            const base = queuedMessageRef.current.data.message;
            const newMessage = base
              ? `${base}\n\n${imageMarkdown}`
              : imageMarkdown;
            setLocalMessage(newMessage);
            setFollowUpMessageRef.current(newMessage);
          } else {
            setLocalMessage((prev) => {
              const newMessage = prev
                ? `${prev}\n\n${imageMarkdown}`
                : imageMarkdown;
              setFollowUpMessageRef.current(newMessage); // Debounced save to scratch
              return newMessage;
            });
          }
        } catch (error) {
          console.error('Failed to upload image:', error);
        }
      }
    },
    [workspaceId]
  );

  // Attachment button - file input ref and handlers
  const fileInputRef = useRef<HTMLInputElement>(null);
  const handleAttachClick = useCallback(() => {
    fileInputRef.current?.click();
  }, []);
  const handleFileInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = Array.from(e.target.files || []).filter((f) =>
        f.type.startsWith('image/')
      );
      if (files.length > 0) {
        handlePasteFiles(files);
      }
      // Reset input so same file can be selected again
      e.target.value = '';
    },
    [handlePasteFiles]
  );

  const handleInsertQuickPhrase = useCallback((phrase: string) => {
    localStorage.setItem(LAST_QUICK_REPLY_STORAGE_KEY, phrase);
    setLastQuickReply(phrase);

    if (isQueuedRef.current && queuedMessageRef.current) {
      cancelQueueRef.current();
      const base = queuedMessageRef.current.data.message;
      const newMessage = appendQuickPhrase(base, phrase);
      setLocalMessage(newMessage);
      setFollowUpMessageRef.current(newMessage);
      return;
    }

    setLocalMessage((prev) => {
      const newMessage = appendQuickPhrase(prev, phrase);
      setFollowUpMessageRef.current(newMessage);
      return newMessage;
    });
  }, []);

  // Stable onChange handler for WYSIWYGEditor
  const handleEditorChange = useCallback(
    (value: string) => {
      // Auto-cancel queue when user starts editing
      if (isQueuedRef.current) {
        cancelQueueRef.current();
      }
      setLocalMessage(value); // Immediate update for UI responsiveness
      setFollowUpMessageRef.current(value); // Debounced save to scratch
      if (followUpErrorRef.current) setFollowUpError(null);
    },
    [setFollowUpError]
  );

  // Memoize placeholder to avoid re-renders
  const hasExtraContext = !!(reviewMarkdown || conflictResolutionInstructions);
  const editorPlaceholder = useMemo(
    () =>
      hasExtraContext
        ? '(Optional) Add additional instructions... Type @ to insert tags or search files.'
        : 'Continue working on this task attempt... Type @ to insert tags or search files.',
    [hasExtraContext]
  );

  // When a process completes (e.g., agent resolved conflicts), refresh branch status promptly
  const prevRunningRef = useRef<boolean>(isAttemptRunning);
  useEffect(() => {
    if (prevRunningRef.current && !isAttemptRunning && workspaceId) {
      refetchBranchStatus();
      refetchAttemptBranch();
    }
    prevRunningRef.current = isAttemptRunning;
  }, [
    isAttemptRunning,
    workspaceId,
    refetchBranchStatus,
    refetchAttemptBranch,
  ]);

  if (!workspaceId) return null;

  if (isScratchLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin h-6 w-6" />
      </div>
    );
  }

  return (
    <div
      className={cn(
        'grid h-full min-h-0 grid-rows-[minmax(0,1fr)_auto] overflow-hidden',
        isRetryActive && 'opacity-50'
      )}
    >
      {/* Scrollable content area */}
      <div className="overflow-y-auto min-h-0 p-4">
        <div className="space-y-2">
          {followUpError && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{followUpError}</AlertDescription>
            </Alert>
          )}
          <div className="space-y-2">
            {/* Review comments preview */}
            {reviewMarkdown && (
              <div className="mb-4">
                <div className="text-sm whitespace-pre-wrap break-words rounded-md border bg-muted p-3">
                  {reviewMarkdown}
                </div>
              </div>
            )}

            {/* Conflict notice and actions (optional UI) */}
            {branchStatus && (
              <FollowUpConflictSection
                workspaceId={workspaceId}
                attemptBranch={attemptBranch}
                branchStatus={branchStatus}
                isEditable={isEditable}
                onResolve={onSendFollowUp}
                enableResolve={
                  canSendFollowUp && !isAttemptRunning && isEditable
                }
                enableAbort={canSendFollowUp && !isAttemptRunning}
                conflictResolutionInstructions={conflictResolutionInstructions}
              />
            )}

            {/* Clicked elements notice and actions */}
            <ClickedElementsBanner />

            {/* Queued message indicator */}
            {isQueued && queuedMessage && (
              <div className="flex items-center gap-2 text-sm text-muted-foreground bg-muted p-3 rounded-md border">
                <Clock className="h-4 w-4 flex-shrink-0" />
                <div className="font-medium">
                  {t(
                    'followUp.queuedMessage',
                    'Message queued - will execute when current run finishes'
                  )}
                </div>
              </div>
            )}

            <div
              className="flex flex-col gap-2"
              onFocus={() => setIsTextareaFocused(true)}
              onBlur={(e) => {
                // Only blur if focus is leaving the container entirely
                if (!e.currentTarget.contains(e.relatedTarget)) {
                  setIsTextareaFocused(false);
                }
              }}
            >
              <WYSIWYGEditor
                placeholder={editorPlaceholder}
                value={displayMessage}
                onChange={handleEditorChange}
                disabled={!isEditable}
                onEnter={handlePrimaryEnterAction}
                onPasteFiles={handlePasteFiles}
                projectId={projectId}
                taskAttemptId={workspaceId}
                className="min-h-[40px]"
              />
            </div>
          </div>
        </div>
      </div>

      {/* Always-visible action bar */}
      <div className="p-4">
        <div className="flex flex-row gap-2 items-center">
          <div className="flex-1 flex gap-2">
            <VariantSelector
              currentProfile={currentProfile}
              selectedVariant={selectedVariant}
              onChange={setSelectedVariant}
              disabled={!isEditable}
            />
          </div>

          {/* Hidden file input for attachment - always present */}
          <input
            ref={fileInputRef}
            type="file"
            accept="image/*"
            multiple
            className="hidden"
            onChange={handleFileInputChange}
          />

          {/* Attach button - always visible */}
          <Button
            onClick={handleAttachClick}
            disabled={!isEditable}
            size="sm"
            variant="outline"
            title="Attach image"
            aria-label="Attach image"
          >
            <Paperclip className="h-4 w-4" />
          </Button>

          {config?.quick_reply_enabled && rankedQuickPhrases.length > 0 && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  disabled={!isEditable}
                  size="sm"
                  variant="outline"
                  title={t('followUp.quickPhrases')}
                  aria-label={t('followUp.quickPhrases')}
                >
                  <Sparkles className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuLabel>
                  {t('followUp.quickPhrasesFromConfig')}
                </DropdownMenuLabel>
                {rankedQuickPhrases.map((phrase, index) => (
                  <DropdownMenuItem
                    key={`config-${phrase}`}
                    onClick={() => handleInsertQuickPhrase(phrase)}
                  >
                    {phrase}
                    {index === 0 && phrase === lastQuickReply ? (
                      <span className="ml-auto text-xs text-muted-foreground">
                        {t('followUp.quickPhrasesRecent')}
                      </span>
                    ) : null}
                  </DropdownMenuItem>
                ))}
                <DropdownMenuSeparator />
                <div className="px-2 py-1 text-xs text-muted-foreground">
                  {t('followUp.quickPhrasesMatchHint')}
                </div>
              </DropdownMenuContent>
            </DropdownMenu>
          )}

          {isAttemptRunning ? (
            <div className="flex items-center gap-2">
              {/* Queue/Cancel Queue button when running */}
              {isQueued ? (
                <Button
                  onClick={cancelQueue}
                  disabled={isQueueLoading}
                  size="sm"
                  variant="outline"
                >
                  {isQueueLoading ? (
                    <Loader2 className="animate-spin h-4 w-4 mr-2" />
                  ) : (
                    <>
                      <X className="h-4 w-4 mr-2" />
                      {t('followUp.cancelQueue', 'Cancel Queue')}
                    </>
                  )}
                </Button>
              ) : (
                <Button
                  onClick={handleQueueMessage}
                  disabled={
                    isQueueLoading ||
                    !hasFollowUpContent
                  }
                  size="sm"
                >
                  {isQueueLoading ? (
                    <Loader2 className="animate-spin h-4 w-4 mr-2" />
                  ) : (
                    <>
                      <Clock className="h-4 w-4 mr-2" />
                      {t('followUp.queue', 'Queue')}
                    </>
                  )}
                </Button>
              )}
              <Button
                onClick={stopExecution}
                disabled={isStopping}
                size="sm"
                variant="destructive"
              >
                {isStopping ? (
                  <Loader2 className="animate-spin h-4 w-4 mr-2" />
                ) : (
                  <>
                    <StopCircle className="h-4 w-4 mr-2" />
                    {t('followUp.stop')}
                  </>
                )}
              </Button>
            </div>
          ) : (
            <div className="flex items-center gap-2">
              {comments.length > 0 && (
                <Button
                  onClick={clearComments}
                  size="sm"
                  variant="destructive"
                  disabled={!isEditable}
                >
                  {t('followUp.clearReviewComments')}
                </Button>
              )}
              <Button
                onClick={onSendFollowUp}
                disabled={!canSendFollowUp || !isEditable}
                size="sm"
              >
                {isSendingFollowUp ? (
                  <Loader2 className="animate-spin h-4 w-4 mr-2" />
                ) : (
                  <>
                    <Send className="h-4 w-4 mr-2" />
                    {conflictResolutionInstructions
                      ? t('followUp.resolveConflicts')
                      : t('followUp.send')}
                  </>
                )}
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
