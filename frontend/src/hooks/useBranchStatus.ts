import { useQuery } from '@tanstack/react-query';
import { attemptsApi } from '@/lib/api';

export function useBranchStatus(
  attemptId?: string,
  options?: { enabled?: boolean; resume?: boolean }
) {
  return useQuery({
    queryKey: ['branchStatus', attemptId, options?.resume ?? false],
    queryFn: () =>
      attemptsApi.getBranchStatus(attemptId!, {
        resume: options?.resume,
      }),
    enabled: !!attemptId && (options?.enabled ?? true),
    refetchInterval: 5000,
  });
}
