import { isSubtaskOriginalRepoNoGitMode } from './gitMode';

function assert(condition: unknown, message: string): void {
  if (!condition) throw new Error(message);
}

export function runGitModeUnitTests(): void {
  assert(
    isSubtaskOriginalRepoNoGitMode(
      { parent_workspace_id: 'parent-id' },
      { use_original_repos: true }
    ),
    'expected subtask+original-repo to enable no-git mode'
  );

  assert(
    !isSubtaskOriginalRepoNoGitMode(
      { parent_workspace_id: null },
      { use_original_repos: true }
    ),
    'expected non-subtask to keep git mode enabled'
  );

  assert(
    !isSubtaskOriginalRepoNoGitMode(
      { parent_workspace_id: 'parent-id' },
      { use_original_repos: false }
    ),
    'expected non-original-repo to keep git mode enabled'
  );

  assert(
    !isSubtaskOriginalRepoNoGitMode(undefined, undefined),
    'expected undefined inputs to keep git mode enabled'
  );
}
