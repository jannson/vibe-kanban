// useConversationHistory.ts
import {
  CommandExitStatus,
  ExecutionProcess,
  ExecutionProcessStatus,
  ExecutorAction,
  NormalizedEntry,
  PatchType,
  ToolStatus,
  Workspace,
} from 'shared/types';
import { useExecutionProcessesContext } from '@/contexts/ExecutionProcessesContext';
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { streamJsonPatchEntries } from '@/utils/streamJsonPatchEntries';

export type PatchTypeWithKey = PatchType & {
  patchKey: string;
  executionProcessId: string;
};

export type AddEntryType = 'initial' | 'running' | 'historic';

export type OnEntriesUpdated = (
  newEntries: PatchTypeWithKey[],
  addType: AddEntryType,
  loading: boolean
) => void;

type ExecutionProcessStaticInfo = {
  id: string;
  created_at: string;
  updated_at: string;
  executor_action: ExecutorAction;
};

type ExecutionProcessState = {
  executionProcess: ExecutionProcessStaticInfo;
  entries: PatchTypeWithKey[];
};

type ExecutionProcessStateStore = Record<string, ExecutionProcessState>;

interface UseConversationHistoryParams {
  attempt: Workspace;
  onEntriesUpdated: OnEntriesUpdated;
}

interface UseConversationHistoryResult {
  loadOlderEntries: () => Promise<boolean>;
  hasMoreHistoric: boolean;
  isLoadingHistoric: boolean;
}

const MIN_INITIAL_ENTRIES = 10;
const REMAINING_BATCH_SIZE = 50;
const INITIAL_HISTORIC_MAX_ENTRIES = 200;
const INITIAL_HISTORIC_MAX_CODING_AGENT_PROCESSES = 1;
const INITIAL_HISTORIC_MAX_PATCH_EVENTS = 1500;
const INITIAL_HISTORIC_MAX_PROCESS_LOAD_MS = 5000;

type LoadHistoricEntriesResult = {
  entries: PatchType[];
  truncated: boolean;
};

type HistoricLoadLimits = {
  maxEntries?: number;
  maxPatchEvents?: number;
  maxDurationMs?: number;
};

const makeLoadingPatch = (executionProcessId: string): PatchTypeWithKey => ({
  type: 'NORMALIZED_ENTRY',
  content: {
    entry_type: {
      type: 'loading',
    },
    content: '',
    timestamp: null,
  },
  patchKey: `${executionProcessId}:loading`,
  executionProcessId,
});

const nextActionPatch: (
  failed: boolean,
  execution_processes: number,
  needs_setup: boolean,
  setup_help_text?: string
) => PatchTypeWithKey = (
  failed,
  execution_processes,
  needs_setup,
  setup_help_text
) => ({
  type: 'NORMALIZED_ENTRY',
  content: {
    entry_type: {
      type: 'next_action',
      failed: failed,
      execution_processes: execution_processes,
      needs_setup: needs_setup,
      setup_help_text: setup_help_text ?? null,
    },
    content: '',
    timestamp: null,
  },
  patchKey: 'next_action',
  executionProcessId: '',
});

export const useConversationHistory = ({
  attempt,
  onEntriesUpdated,
}: UseConversationHistoryParams): UseConversationHistoryResult => {
  const { executionProcessesVisible: executionProcessesRaw } =
    useExecutionProcessesContext();
  const executionProcesses = useRef<ExecutionProcess[]>(executionProcessesRaw);
  const displayedExecutionProcesses = useRef<ExecutionProcessStateStore>({});
  const loadedInitialEntries = useRef(false);
  const streamingProcessIdsRef = useRef<Set<string>>(new Set());
  const truncatedHistoricProcessIdsRef = useRef<Set<string>>(new Set());
  const onEntriesUpdatedRef = useRef<OnEntriesUpdated | null>(null);
  const [hasMoreHistoric, setHasMoreHistoric] = useState(false);
  const [isLoadingHistoric, setIsLoadingHistoric] = useState(false);

  const updateHasMoreHistoric = useCallback(() => {
    const remaining = executionProcesses.current.some(
      (process) =>
        process.status !== ExecutionProcessStatus.running &&
        !displayedExecutionProcesses.current[process.id]
    );
    const hasTruncatedHistoric = truncatedHistoricProcessIdsRef.current.size > 0;
    const next = remaining || hasTruncatedHistoric;
    setHasMoreHistoric((prev) => (prev === next ? prev : next));
  }, []);

  const mergeIntoDisplayed = useCallback(
    (mutator: (state: ExecutionProcessStateStore) => void) => {
      const state = displayedExecutionProcesses.current;
      mutator(state);
      updateHasMoreHistoric();
    },
    [updateHasMoreHistoric]
  );
  useEffect(() => {
    onEntriesUpdatedRef.current = onEntriesUpdated;
  }, [onEntriesUpdated]);

  // Keep executionProcesses up to date
  useEffect(() => {
    executionProcesses.current = executionProcessesRaw.filter(
      (ep) =>
        ep.run_reason === 'setupscript' ||
        ep.run_reason === 'cleanupscript' ||
        ep.run_reason === 'codingagent'
    );
  }, [executionProcessesRaw]);

  useEffect(() => {
    updateHasMoreHistoric();
  }, [executionProcessesRaw, updateHasMoreHistoric]);

  const loadEntriesForHistoricExecutionProcess = useCallback(
    (executionProcess: ExecutionProcess, limits?: HistoricLoadLimits) => {
    let url = '';
    if (executionProcess.executor_action.typ.type === 'ScriptRequest') {
      url = `/api/execution-processes/${executionProcess.id}/raw-logs/ws`;
    } else {
      url = `/api/execution-processes/${executionProcess.id}/normalized-logs/ws`;
    }

      return new Promise<LoadHistoricEntriesResult>((resolve) => {
      let settled = false;
      let truncated = false;
      let patchEvents = 0;
      let controller: {
        close: () => void;
        getEntries: () => PatchType[];
      } | null = null;
      let timeoutId: number | null = null;

      const settle = (entries: PatchType[]) => {
        if (settled) return;
        settled = true;
        if (timeoutId !== null) {
          window.clearTimeout(timeoutId);
        }
        resolve({ entries, truncated });
      };

      if (limits?.maxDurationMs !== undefined && limits.maxDurationMs > 0) {
        timeoutId = window.setTimeout(() => {
          truncated = true;
          if (controller) {
            const snapshot = controller.getEntries();
            controller.close();
            settle(snapshot);
          } else {
            settle([]);
          }
        }, limits.maxDurationMs);
      }

      controller = streamJsonPatchEntries<PatchType>(url, {
        onEntries: (entries) => {
          patchEvents += 1;
          if (
            limits?.maxPatchEvents !== undefined &&
            limits.maxPatchEvents > 0 &&
            patchEvents >= limits.maxPatchEvents
          ) {
            truncated = true;
            controller?.close();
            settle(entries);
            return;
          }

          if (
            limits?.maxEntries !== undefined &&
            limits.maxEntries > 0 &&
            entries.length >= limits.maxEntries
          ) {
            truncated = true;
            controller?.close();
            settle(entries.slice(0, limits.maxEntries));
          }
        },
        onFinished: (allEntries) => {
          controller?.close();
          settle(allEntries);
        },
        onError: (err) => {
          console.warn!(
            `Error loading entries for historic execution process ${executionProcess.id}`,
            err
          );
          controller?.close();
          settle([]);
        },
      });
      });
    },
    []
  );

  const getLiveExecutionProcess = (
    executionProcessId: string
  ): ExecutionProcess | undefined => {
    return executionProcesses?.current.find(
      (executionProcess) => executionProcess.id === executionProcessId
    );
  };

  const patchWithKey = (
    patch: PatchType,
    executionProcessId: string,
    index: number | 'user'
  ) => {
    return {
      ...patch,
      patchKey: `${executionProcessId}:${index}`,
      executionProcessId,
    };
  };

  const flattenEntries = (
    executionProcessState: ExecutionProcessStateStore
  ): PatchTypeWithKey[] => {
    return Object.values(executionProcessState)
      .filter(
        (p) =>
          p.executionProcess.executor_action.typ.type ===
            'CodingAgentFollowUpRequest' ||
          p.executionProcess.executor_action.typ.type ===
            'CodingAgentInitialRequest'
      )
      .sort(
        (a, b) =>
          new Date(
            a.executionProcess.created_at as unknown as string
          ).getTime() -
          new Date(b.executionProcess.created_at as unknown as string).getTime()
      )
      .flatMap((p) => p.entries);
  };

  const getActiveAgentProcesses = (): ExecutionProcess[] => {
    return (
      executionProcesses?.current.filter(
        (p) =>
          p.status === ExecutionProcessStatus.running &&
          p.run_reason !== 'devserver'
      ) ?? []
    );
  };

  const flattenEntriesForEmit = useCallback(
    (executionProcessState: ExecutionProcessStateStore): PatchTypeWithKey[] => {
      // Flags to control Next Action bar emit
      let hasPendingApproval = false;
      let hasRunningProcess = false;
      let lastProcessFailedOrKilled = false;
      let needsSetup = false;
      let setupHelpText: string | undefined;

      // Create user messages + tool calls for setup/cleanup scripts
      const allEntries = Object.values(executionProcessState)
        .sort(
          (a, b) =>
            new Date(
              a.executionProcess.created_at as unknown as string
            ).getTime() -
            new Date(
              b.executionProcess.created_at as unknown as string
            ).getTime()
        )
        .flatMap((p, index) => {
          const entries: PatchTypeWithKey[] = [];
          if (
            p.executionProcess.executor_action.typ.type ===
              'CodingAgentInitialRequest' ||
            p.executionProcess.executor_action.typ.type ===
              'CodingAgentFollowUpRequest'
          ) {
            // New user message
            const userNormalizedEntry: NormalizedEntry = {
              entry_type: {
                type: 'user_message',
              },
              content: p.executionProcess.executor_action.typ.prompt,
              timestamp: null,
            };
            const userPatch: PatchType = {
              type: 'NORMALIZED_ENTRY',
              content: userNormalizedEntry,
            };
            const userPatchTypeWithKey = patchWithKey(
              userPatch,
              p.executionProcess.id,
              'user'
            );
            entries.push(userPatchTypeWithKey);

            // Remove all coding agent added user messages, replace with our custom one
            const entriesExcludingUser = p.entries.filter(
              (e) =>
                e.type !== 'NORMALIZED_ENTRY' ||
                e.content.entry_type.type !== 'user_message'
            );

            const hasPendingApprovalEntry = entriesExcludingUser.some(
              (entry) => {
                if (entry.type !== 'NORMALIZED_ENTRY') return false;
                const entryType = entry.content.entry_type;
                return (
                  entryType.type === 'tool_use' &&
                  entryType.status.status === 'pending_approval'
                );
              }
            );

            if (hasPendingApprovalEntry) {
              hasPendingApproval = true;
            }

            entries.push(...entriesExcludingUser);

            const liveProcessStatus = getLiveExecutionProcess(
              p.executionProcess.id
            )?.status;
            const isProcessRunning =
              liveProcessStatus === ExecutionProcessStatus.running;
            const processFailedOrKilled =
              liveProcessStatus === ExecutionProcessStatus.failed ||
              liveProcessStatus === ExecutionProcessStatus.killed;

            if (isProcessRunning) {
              hasRunningProcess = true;
            }

            if (
              processFailedOrKilled &&
              index === Object.keys(executionProcessState).length - 1
            ) {
              lastProcessFailedOrKilled = true;

              // Check if this failed process has a SetupRequired entry
              const hasSetupRequired = entriesExcludingUser.some((entry) => {
                if (entry.type !== 'NORMALIZED_ENTRY') return false;
                if (
                  entry.content.entry_type.type === 'error_message' &&
                  entry.content.entry_type.error_type.type === 'setup_required'
                ) {
                  setupHelpText = entry.content.content;
                  return true;
                }
                return false;
              });

              if (hasSetupRequired) {
                needsSetup = true;
              }
            }

            if (isProcessRunning && !hasPendingApprovalEntry) {
              entries.push(makeLoadingPatch(p.executionProcess.id));
            }
          } else if (
            p.executionProcess.executor_action.typ.type === 'ScriptRequest'
          ) {
            // Add setup and cleanup script as a tool call
            let toolName = '';
            switch (p.executionProcess.executor_action.typ.context) {
              case 'SetupScript':
                toolName = 'Setup Script';
                break;
              case 'CleanupScript':
                toolName = 'Cleanup Script';
                break;
              case 'ToolInstallScript':
                toolName = 'Tool Install Script';
                break;
              default:
                return [];
            }

            const executionProcess = getLiveExecutionProcess(
              p.executionProcess.id
            );

            if (executionProcess?.status === ExecutionProcessStatus.running) {
              hasRunningProcess = true;
            }

            if (
              (executionProcess?.status === ExecutionProcessStatus.failed ||
                executionProcess?.status === ExecutionProcessStatus.killed) &&
              index === Object.keys(executionProcessState).length - 1
            ) {
              lastProcessFailedOrKilled = true;
            }

            const exitCode = Number(executionProcess?.exit_code) || 0;
            const exit_status: CommandExitStatus | null =
              executionProcess?.status === 'running'
                ? null
                : {
                    type: 'exit_code',
                    code: exitCode,
                  };

            const toolStatus: ToolStatus =
              executionProcess?.status === ExecutionProcessStatus.running
                ? { status: 'created' }
                : exitCode === 0
                  ? { status: 'success' }
                  : { status: 'failed' };

            const output = p.entries.map((line) => line.content).join('\n');

            const toolNormalizedEntry: NormalizedEntry = {
              entry_type: {
                type: 'tool_use',
                tool_name: toolName,
                action_type: {
                  action: 'command_run',
                  command: p.executionProcess.executor_action.typ.script,
                  result: {
                    output,
                    exit_status,
                  },
                },
                status: toolStatus,
              },
              content: toolName,
              timestamp: null,
            };
            const toolPatch: PatchType = {
              type: 'NORMALIZED_ENTRY',
              content: toolNormalizedEntry,
            };
            const toolPatchWithKey: PatchTypeWithKey = patchWithKey(
              toolPatch,
              p.executionProcess.id,
              0
            );

            entries.push(toolPatchWithKey);
          }

          return entries;
        });

      // Emit the next action bar if no process running
      if (!hasRunningProcess && !hasPendingApproval) {
        allEntries.push(
          nextActionPatch(
            lastProcessFailedOrKilled,
            Object.keys(executionProcessState).length,
            needsSetup,
            setupHelpText
          )
        );
      }

      return allEntries;
    },
    []
  );

  const emitEntries = useCallback(
    (
      executionProcessState: ExecutionProcessStateStore,
      addEntryType: AddEntryType,
      loading: boolean
    ) => {
      const entries = flattenEntriesForEmit(executionProcessState);
      onEntriesUpdatedRef.current?.(entries, addEntryType, loading);
    },
    [flattenEntriesForEmit]
  );

  // This emits its own events as they are streamed
  const loadRunningAndEmit = useCallback(
    (executionProcess: ExecutionProcess): Promise<void> => {
      return new Promise((resolve, reject) => {
        let url = '';
        if (executionProcess.executor_action.typ.type === 'ScriptRequest') {
          url = `/api/execution-processes/${executionProcess.id}/raw-logs/ws`;
        } else {
          url = `/api/execution-processes/${executionProcess.id}/normalized-logs/ws`;
        }
        const controller = streamJsonPatchEntries<PatchType>(url, {
          onEntries(entries) {
            const patchesWithKey = entries.map((entry, index) =>
              patchWithKey(entry, executionProcess.id, index)
            );
            mergeIntoDisplayed((state) => {
              state[executionProcess.id] = {
                executionProcess,
                entries: patchesWithKey,
              };
            });
            emitEntries(displayedExecutionProcesses.current, 'running', false);
          },
          onFinished: () => {
            emitEntries(displayedExecutionProcesses.current, 'running', false);
            controller.close();
            resolve();
          },
          onError: () => {
            controller.close();
            reject();
          },
        });
      });
    },
    [emitEntries, mergeIntoDisplayed]
  );

  // Sometimes it can take a few seconds for the stream to start, wrap the loadRunningAndEmit method
  const loadRunningAndEmitWithBackoff = useCallback(
    async (executionProcess: ExecutionProcess) => {
      for (let i = 0; i < 20; i++) {
        try {
          await loadRunningAndEmit(executionProcess);
          break;
        } catch (_) {
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
      }
    },
    [loadRunningAndEmit]
  );

  const loadInitialEntries =
    useCallback(async (): Promise<ExecutionProcessStateStore> => {
      const localDisplayedExecutionProcesses: ExecutionProcessStateStore = {};

      if (!executionProcesses?.current) return localDisplayedExecutionProcesses;
      let loadedCodingAgentProcesses = 0;
      let loadedEntries = 0;

      for (const executionProcess of [
        ...executionProcesses.current,
      ].reverse()) {
        if (executionProcess.status === ExecutionProcessStatus.running)
          continue;
        const isCodingAgentProcess =
          executionProcess.executor_action.typ.type ===
            'CodingAgentInitialRequest' ||
          executionProcess.executor_action.typ.type ===
            'CodingAgentFollowUpRequest';
        if (!isCodingAgentProcess) continue;

        const remainingEntryBudget = Math.max(
          1,
          INITIAL_HISTORIC_MAX_ENTRIES - loadedEntries
        );

        const { entries, truncated } = await loadEntriesForHistoricExecutionProcess(
          executionProcess,
          {
            maxEntries: remainingEntryBudget,
            maxPatchEvents: INITIAL_HISTORIC_MAX_PATCH_EVENTS,
            maxDurationMs: INITIAL_HISTORIC_MAX_PROCESS_LOAD_MS,
          }
        );
        const entriesWithKey = entries.map((e, idx) =>
          patchWithKey(e, executionProcess.id, idx)
        );

        localDisplayedExecutionProcesses[executionProcess.id] = {
          executionProcess,
          entries: entriesWithKey,
        };
        loadedCodingAgentProcesses += 1;
        loadedEntries += entriesWithKey.length;

        if (truncated) {
          truncatedHistoricProcessIdsRef.current.add(executionProcess.id);
        } else {
          truncatedHistoricProcessIdsRef.current.delete(executionProcess.id);
        }

        if (
          loadedCodingAgentProcesses >=
            INITIAL_HISTORIC_MAX_CODING_AGENT_PROCESSES ||
          loadedEntries >= INITIAL_HISTORIC_MAX_ENTRIES ||
          flattenEntries(localDisplayedExecutionProcesses).length >
            MIN_INITIAL_ENTRIES
        ) {
          break;
        }
      }

      return localDisplayedExecutionProcesses;
    }, [executionProcesses, loadEntriesForHistoricExecutionProcess]);

  const loadRemainingEntriesInBatches = useCallback(
    async (batchSize: number): Promise<boolean> => {
      if (!executionProcesses?.current) return false;

      let anyUpdated = false;
      for (const executionProcess of [
        ...executionProcesses.current,
      ].reverse()) {
        const current = displayedExecutionProcesses.current;
        const shouldReloadTruncated =
          truncatedHistoricProcessIdsRef.current.has(executionProcess.id);
        if (
          (current[executionProcess.id] && !shouldReloadTruncated) ||
          executionProcess.status === ExecutionProcessStatus.running
        )
          continue;

        const { entries } =
          await loadEntriesForHistoricExecutionProcess(executionProcess);
        const entriesWithKey = entries.map((e, idx) =>
          patchWithKey(e, executionProcess.id, idx)
        );

        mergeIntoDisplayed((state) => {
          state[executionProcess.id] = {
            executionProcess,
            entries: entriesWithKey,
          };
        });
        truncatedHistoricProcessIdsRef.current.delete(executionProcess.id);

        if (
          flattenEntries(displayedExecutionProcesses.current).length > batchSize
        ) {
          anyUpdated = true;
          break;
        }
        anyUpdated = true;
      }
      return anyUpdated;
    },
    [executionProcesses, loadEntriesForHistoricExecutionProcess, mergeIntoDisplayed]
  );

  const backfillLatestTruncatedProcess = useCallback(async (): Promise<boolean> => {
    const latestTruncated = [...executionProcesses.current]
      .reverse()
      .find((process) => {
        const isCodingAgentProcess =
          process.executor_action.typ.type ===
            'CodingAgentInitialRequest' ||
          process.executor_action.typ.type ===
            'CodingAgentFollowUpRequest';
        return (
          isCodingAgentProcess &&
          process.status !== ExecutionProcessStatus.running &&
          truncatedHistoricProcessIdsRef.current.has(process.id)
        );
      });

    if (!latestTruncated) return false;

    const { entries } =
      await loadEntriesForHistoricExecutionProcess(latestTruncated);
    const entriesWithKey = entries.map((e, idx) =>
      patchWithKey(e, latestTruncated.id, idx)
    );

    mergeIntoDisplayed((state) => {
      state[latestTruncated.id] = {
        executionProcess: latestTruncated,
        entries: entriesWithKey,
      };
    });
    truncatedHistoricProcessIdsRef.current.delete(latestTruncated.id);
    return true;
  }, [executionProcesses, loadEntriesForHistoricExecutionProcess, mergeIntoDisplayed]);

  const ensureProcessVisible = useCallback(
    (p: ExecutionProcess) => {
      mergeIntoDisplayed((state) => {
        if (!state[p.id]) {
          state[p.id] = {
            executionProcess: {
              id: p.id,
              created_at: p.created_at,
              updated_at: p.updated_at,
              executor_action: p.executor_action,
            },
            entries: [],
          };
        }
      });
    },
    [mergeIntoDisplayed]
  );

  const idListKey = useMemo(
    () => executionProcessesRaw?.map((p) => p.id).join(','),
    [executionProcessesRaw]
  );

  const idStatusKey = useMemo(
    () => executionProcessesRaw?.map((p) => `${p.id}:${p.status}`).join(','),
    [executionProcessesRaw]
  );

  // Initial load when attempt changes
  useEffect(() => {
    let cancelled = false;
    (async () => {
      // Waiting for execution processes to load
      if (
        executionProcesses?.current.length === 0 ||
        loadedInitialEntries.current
      )
        return;

      // Initial entries
      const allInitialEntries = await loadInitialEntries();
      if (cancelled) return;
      mergeIntoDisplayed((state) => {
        Object.assign(state, allInitialEntries);
      });
      emitEntries(displayedExecutionProcesses.current, 'initial', false);
      loadedInitialEntries.current = true;

      updateHasMoreHistoric();

      if (truncatedHistoricProcessIdsRef.current.size > 0) {
        void backfillLatestTruncatedProcess().then((updated) => {
          if (!updated || cancelled) return;
          emitEntries(displayedExecutionProcesses.current, 'historic', false);
          updateHasMoreHistoric();
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    attempt.id,
    idListKey,
    loadInitialEntries,
    loadRemainingEntriesInBatches,
    backfillLatestTruncatedProcess,
    emitEntries,
    mergeIntoDisplayed,
    updateHasMoreHistoric,
  ]); // include idListKey so new processes trigger reload

  useEffect(() => {
    const activeProcesses = getActiveAgentProcesses();
    if (activeProcesses.length === 0) return;

    for (const activeProcess of activeProcesses) {
      if (!displayedExecutionProcesses.current[activeProcess.id]) {
        const runningOrInitial =
          Object.keys(displayedExecutionProcesses.current).length > 1
            ? 'running'
            : 'initial';
        ensureProcessVisible(activeProcess);
        emitEntries(
          displayedExecutionProcesses.current,
          runningOrInitial,
          false
        );
      }

      if (
        activeProcess.status === ExecutionProcessStatus.running &&
        !streamingProcessIdsRef.current.has(activeProcess.id)
      ) {
        streamingProcessIdsRef.current.add(activeProcess.id);
        loadRunningAndEmitWithBackoff(activeProcess).finally(() => {
          streamingProcessIdsRef.current.delete(activeProcess.id);
        });
      }
    }
  }, [
    attempt.id,
    idStatusKey,
    emitEntries,
    ensureProcessVisible,
    loadRunningAndEmitWithBackoff,
  ]);

  // If an execution process is removed, remove it from the state
  useEffect(() => {
    if (!executionProcessesRaw) return;

    const removedProcessIds = Object.keys(
      displayedExecutionProcesses.current
    ).filter((id) => !executionProcessesRaw.some((p) => p.id === id));

    if (removedProcessIds.length > 0) {
      mergeIntoDisplayed((state) => {
        removedProcessIds.forEach((id) => {
          delete state[id];
        });
      });
      updateHasMoreHistoric();
    }
  }, [
    attempt.id,
    idListKey,
    executionProcessesRaw,
    mergeIntoDisplayed,
    updateHasMoreHistoric,
  ]);

  // Reset state when attempt changes
  useEffect(() => {
    displayedExecutionProcesses.current = {};
    loadedInitialEntries.current = false;
    streamingProcessIdsRef.current.clear();
    truncatedHistoricProcessIdsRef.current.clear();
    emitEntries(displayedExecutionProcesses.current, 'initial', true);
    setHasMoreHistoric(false);
    setIsLoadingHistoric(false);
  }, [attempt.id, emitEntries]);

  const loadOlderEntries = useCallback(async () => {
    if (isLoadingHistoric) return false;
    setIsLoadingHistoric(true);
    const updated = await loadRemainingEntriesInBatches(REMAINING_BATCH_SIZE);
    emitEntries(displayedExecutionProcesses.current, 'historic', false);
    updateHasMoreHistoric();
    setIsLoadingHistoric(false);
    return updated;
  }, [
    emitEntries,
    isLoadingHistoric,
    loadRemainingEntriesInBatches,
    updateHasMoreHistoric,
  ]);

  return { loadOlderEntries, hasMoreHistoric, isLoadingHistoric };
};
