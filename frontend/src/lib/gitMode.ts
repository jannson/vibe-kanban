type TaskLike = {
  parent_workspace_id?: string | null;
};

type AttemptLike = {
  use_original_repos?: boolean | null;
};

export function isSubtaskOriginalRepoNoGitMode(
  task?: TaskLike | null,
  attempt?: AttemptLike | null
): boolean {
  return Boolean(task?.parent_workspace_id && attempt?.use_original_repos);
}
