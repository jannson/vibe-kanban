import type {
  TaskDetailCapabilities,
  TaskDetailStateInput,
} from './state-machine.types';

const CLOSED_STATUSES = new Set(['done', 'cancelled'] as const);

export function deriveTaskDetailCapabilities(
  input: TaskDetailStateInput
): TaskDetailCapabilities {
  const isClosedTask =
    input.taskStatus !== null &&
    CLOSED_STATUSES.has(input.taskStatus as 'done' | 'cancelled');

  if (input.taskStatus === null || !input.hasAttempt) {
    return {
      state: 'no_task',
      isClosedTask,
      canShowFollowUp: false,
      canPollBranchStatus: false,
      canPrepareWorkspace: false,
      canShowPreview: false,
      canShowDiffs: false,
      effectiveView: null,
      shouldShowResumeCard: false,
    };
  }

  if (!isClosedTask) {
    return {
      state: 'open_task',
      isClosedTask: false,
      canShowFollowUp: true,
      canPollBranchStatus: true,
      canPrepareWorkspace: true,
      canShowPreview: true,
      canShowDiffs: true,
      effectiveView: input.requestedView,
      shouldShowResumeCard: false,
    };
  }

  if (input.resumePending) {
    return {
      state: 'closed_resuming',
      isClosedTask: true,
      canShowFollowUp: false,
      canPollBranchStatus: false,
      canPrepareWorkspace: false,
      canShowPreview: false,
      canShowDiffs: false,
      effectiveView: null,
      shouldShowResumeCard: true,
    };
  }

  if (input.resumeSucceeded) {
    return {
      state: 'closed_resumed',
      isClosedTask: true,
      canShowFollowUp: true,
      canPollBranchStatus: true,
      canPrepareWorkspace: true,
      canShowPreview: true,
      canShowDiffs: true,
      effectiveView: input.requestedView,
      shouldShowResumeCard: false,
    };
  }

  return {
    state: 'closed_locked',
    isClosedTask: true,
    canShowFollowUp: false,
    canPollBranchStatus: false,
    canPrepareWorkspace: false,
    canShowPreview: false,
    canShowDiffs: false,
    effectiveView: null,
    shouldShowResumeCard: true,
  };
}
