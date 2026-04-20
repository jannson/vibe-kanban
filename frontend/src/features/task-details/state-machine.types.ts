import type { LayoutMode } from '@/components/layout/TasksLayout';
import type { TaskStatus } from 'shared/types';

export type TaskDetailStateId =
  | 'no_task'
  | 'open_task'
  | 'closed_locked'
  | 'closed_resuming'
  | 'closed_resumed';

export interface TaskDetailStateInput {
  taskStatus: TaskStatus | null;
  hasAttempt: boolean;
  requestedView: LayoutMode;
  resumePending: boolean;
  resumeSucceeded: boolean;
}

export interface TaskDetailCapabilities {
  state: TaskDetailStateId;
  isClosedTask: boolean;
  canShowFollowUp: boolean;
  canPollBranchStatus: boolean;
  canPrepareWorkspace: boolean;
  canShowPreview: boolean;
  canShowDiffs: boolean;
  effectiveView: LayoutMode;
  shouldShowResumeCard: boolean;
}
