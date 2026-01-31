import { useMutation, useQueryClient } from '@tanstack/react-query';
import { attemptsApi } from '@/lib/api';
import { repoBranchKeys } from './useRepoBranches';

type CommitParams = {
  repoId: string;
  message?: string | null;
};

export function useCommit(
  attemptId?: string,
  onSuccess?: () => void,
  onError?: (err: unknown) => void
) {
  const queryClient = useQueryClient();

  return useMutation<void, unknown, CommitParams>({
    mutationFn: (params: CommitParams) => {
      if (!attemptId) return Promise.resolve();
      return attemptsApi.commit(attemptId, {
        repo_id: params.repoId,
        message: params.message ?? null,
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['branchStatus', attemptId] });
      queryClient.invalidateQueries({ queryKey: repoBranchKeys.all });
      onSuccess?.();
    },
    onError: (err) => {
      console.error('Failed to commit:', err);
      onError?.(err);
    },
  });
}
