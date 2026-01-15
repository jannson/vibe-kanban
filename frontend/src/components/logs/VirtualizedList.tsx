import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import DisplayConversationEntry from '../NormalizedConversation/DisplayConversationEntry';
import { useEntries } from '@/contexts/EntriesContext';
import {
  AddEntryType,
  PatchTypeWithKey,
  useConversationHistory,
} from '@/hooks/useConversationHistory';
import { Loader2 } from 'lucide-react';
import { TaskWithAttemptStatus } from 'shared/types';
import type { WorkspaceWithSession } from '@/types/attempt';
import { ApprovalFormProvider } from '@/contexts/ApprovalFormContext';
import { Button } from '@/components/ui/button';
import { useExecutionProcessesContext } from '@/contexts/ExecutionProcessesContext';

interface VirtualizedListProps {
  attempt: WorkspaceWithSession;
  task?: TaskWithAttemptStatus;
}

interface MessageGroup {
  id: string;
  entries: PatchTypeWithKey[];
}

const DEFAULT_OPEN_COUNT = 2;
const BOTTOM_SCROLL_THRESHOLD = 48;
const SYSTEM_GROUP_ID = '__system__';

const renderItemContent = (
  data: PatchTypeWithKey,
  attempt: WorkspaceWithSession,
  task?: TaskWithAttemptStatus
) => {
  if (data.type === 'STDOUT') {
    return <pre className="whitespace-pre-wrap">{data.content}</pre>;
  }
  if (data.type === 'STDERR') {
    return <pre className="whitespace-pre-wrap text-destructive">{data.content}</pre>;
  }
  if (data.type === 'NORMALIZED_ENTRY') {
    return (
      <DisplayConversationEntry
        expansionKey={data.patchKey}
        entry={data.content}
        executionProcessId={data.executionProcessId}
        taskAttempt={attempt}
        task={task}
      />
    );
  }

  return null;
};

const buildGroups = (entries: PatchTypeWithKey[]): MessageGroup[] => {
  const order: string[] = [];
  const groups = new Map<string, PatchTypeWithKey[]>();

  for (const entry of entries) {
    const id = entry.executionProcessId || SYSTEM_GROUP_ID;
    if (!groups.has(id)) {
      groups.set(id, []);
      order.push(id);
    }
    groups.get(id)!.push(entry);
  }

  return order.map((id) => ({ id, entries: groups.get(id)! }));
};

const formatProcessLabel = (
  t: (key: string, options?: Record<string, unknown>) => string,
  processType: string | null,
  scriptContext: string | null,
  index: number,
  isSystem: boolean
) => {
  if (isSystem) return t('conversation.systemLog');
  if (processType === 'ScriptRequest') {
    if (scriptContext === 'SetupScript') return t('conversation.setupScript');
    if (scriptContext === 'CleanupScript') return t('conversation.cleanupScript');
    if (scriptContext === 'ToolInstallScript')
      return t('conversation.toolInstallScript');
  }
  if (
    processType === 'CodingAgentInitialRequest' ||
    processType === 'CodingAgentFollowUpRequest'
  ) {
    return t('conversation.agentRun', { index });
  }
  return t('conversation.logSection', { index });
};

const getEntryPreview = (entry: PatchTypeWithKey): string => {
  if (entry.type === 'STDOUT' || entry.type === 'STDERR') {
    return entry.content.trim();
  }
  if (entry.type === 'NORMALIZED_ENTRY') {
    return entry.content.content?.trim?.() ?? '';
  }
  return '';
};

const buildGroupPreview = (entries: PatchTypeWithKey[]): string => {
  for (const entry of entries) {
    const text = getEntryPreview(entry);
    if (text) return text.split(/\r?\n/)[0].trim();
  }
  return '';
};

const VirtualizedList = ({ attempt, task }: VirtualizedListProps) => {
  const { t } = useTranslation('common');
  const [channelData, setChannelData] = useState<PatchTypeWithKey[] | null>(
    null
  );
  const [loading, setLoading] = useState(true);
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({});
  const { setEntries, reset } = useEntries();
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const scrollToBottomRef = useRef(false);
  const isPinnedToBottomRef = useRef(true);
  const pendingScrollRestoreRef = useRef<{
    scrollHeight: number;
    scrollTop: number;
  } | null>(null);

  const { executionProcessesByIdVisible } = useExecutionProcessesContext();

  useEffect(() => {
    setLoading(true);
    setChannelData(null);
    setOpenGroups({});
    reset();
  }, [attempt.id, reset]);

  const onEntriesUpdated = (
    newEntries: PatchTypeWithKey[],
    _addType: AddEntryType,
    newLoading: boolean
  ) => {
    setChannelData(newEntries);
    setEntries(newEntries);

    if (isPinnedToBottomRef.current) {
      scrollToBottomRef.current = true;
    }

    if (loading) {
      setLoading(newLoading);
    }
  };

  const { loadOlderEntries, hasMoreHistoric, isLoadingHistoric } =
    useConversationHistory({
      attempt,
      onEntriesUpdated,
    });

  const groups = useMemo(
    () => buildGroups(channelData ?? []),
    [channelData]
  );

  useEffect(() => {
    if (groups.length === 0) return;
    setOpenGroups((prev) => {
      const next = { ...prev };
      let changed = false;
      groups.forEach((group, index) => {
        if (next[group.id] === undefined) {
          const defaultOpen =
            index >= groups.length - DEFAULT_OPEN_COUNT ||
            executionProcessesByIdVisible[group.id]?.status === 'running';
          next[group.id] = defaultOpen;
          changed = true;
        }
      });
      return changed ? next : prev;
    });
  }, [groups, executionProcessesByIdVisible]);

  useLayoutEffect(() => {
    const container = scrollContainerRef.current;
    if (!container) return;

    if (pendingScrollRestoreRef.current) {
      const { scrollHeight, scrollTop } = pendingScrollRestoreRef.current;
      const delta = container.scrollHeight - scrollHeight;
      container.scrollTop = scrollTop + delta;
      pendingScrollRestoreRef.current = null;
      return;
    }

    if (scrollToBottomRef.current) {
      container.scrollTop = container.scrollHeight;
      scrollToBottomRef.current = false;
    }
  }, [groups.length]);

  const handleLoadEarlier = async () => {
    const container = scrollContainerRef.current;
    if (container) {
      pendingScrollRestoreRef.current = {
        scrollHeight: container.scrollHeight,
        scrollTop: container.scrollTop,
      };
    }
    await loadOlderEntries();
  };

  return (
    <ApprovalFormProvider>
      <div className="flex-1 min-h-0 flex flex-col">
        {hasMoreHistoric && (
          <div className="shrink-0 flex items-center justify-center py-2 border-b border-dashed">
            <Button
              variant="ghost"
              size="sm"
              disabled={isLoadingHistoric}
              onClick={handleLoadEarlier}
            >
              {isLoadingHistoric
                ? t('states.loading')
                : t('conversation.loadEarlierRuns')}
            </Button>
          </div>
        )}
        <div
          ref={scrollContainerRef}
          className="flex-1 min-h-0 overflow-y-auto"
          onScroll={(event) => {
            const target = event.currentTarget;
            const distanceToBottom =
              target.scrollHeight - (target.scrollTop + target.clientHeight);
            isPinnedToBottomRef.current =
              distanceToBottom <= BOTTOM_SCROLL_THRESHOLD;
          }}
        >
          <div className="py-3 space-y-3">
            {groups.map((group, index) => {
              const process = executionProcessesByIdVisible[group.id];
              const isSystem = group.id === SYSTEM_GROUP_ID;
              const processType = process?.executor_action.typ.type ?? null;
              const scriptContext =
                process?.executor_action.typ.type === 'ScriptRequest'
                  ? process.executor_action.typ.context
                  : null;
              const label = formatProcessLabel(
                t,
                processType,
                scriptContext,
                index + 1,
                isSystem
              );
              const preview = buildGroupPreview(group.entries);
              const status = process?.status ?? '';
              const createdAt = process?.created_at
                ? new Date(process.created_at).toLocaleString()
                : '';
              const open = openGroups[group.id] ?? false;

              return (
                <details
                  key={group.id}
                  className="mx-auto w-full max-w-[50rem] border border-dashed rounded-sm bg-background"
                  open={open}
                  onToggle={(event) => {
                    const target = event.currentTarget as HTMLDetailsElement;
                    setOpenGroups((prev) => ({
                      ...prev,
                      [group.id]: target.open,
                    }));
                  }}
                >
                  <summary className="cursor-pointer select-none px-3 py-2 text-sm flex items-center justify-between gap-2">
                    <span className="min-w-0 flex-1 flex flex-col">
                      <span className="font-medium truncate">{label}</span>
                      {preview && (
                        <span className="text-xs text-muted-foreground truncate">
                          {preview}
                        </span>
                      )}
                    </span>
                    <span className="text-xs text-muted-foreground whitespace-nowrap">
                      {status}
                      {createdAt ? ` · ${createdAt}` : ''}
                    </span>
                  </summary>
                  <div className="border-t border-dashed">
                    {group.entries.map((entry) => (
                      <div key={entry.patchKey}>
                        {renderItemContent(entry, attempt, task)}
                      </div>
                    ))}
                  </div>
                </details>
              );
            })}
          </div>
        </div>
      </div>
      {loading && (
        <div className="float-left top-0 left-0 w-full h-full bg-primary flex flex-col gap-2 justify-center items-center">
          <Loader2 className="h-8 w-8 animate-spin" />
          <p>Loading History</p>
        </div>
      )}
    </ApprovalFormProvider>
  );
};

export default VirtualizedList;
