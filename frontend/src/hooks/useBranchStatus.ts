import { useQuery } from '@tanstack/react-query';
import { attemptsApi } from '@/lib/api';

export function useBranchStatus(
  attemptId?: string,
  options?: { enabled?: boolean; resumeKey?: string | null }
) {
  return useQuery({
    queryKey: ['branchStatus', attemptId, options?.resumeKey ?? null],
    queryFn: () =>
      attemptsApi.getBranchStatus(attemptId!, {
        resumeKey: options?.resumeKey ?? undefined,
      }),
    enabled: !!attemptId && (options?.enabled ?? true),
    refetchInterval: 5000,
  });
}
