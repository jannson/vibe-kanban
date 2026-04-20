import { useQuery } from '@tanstack/react-query';
import { attemptsApi } from '@/lib/api';

export function useBranchStatus(
  attemptId?: string,
  options?: { enabled?: boolean }
) {
  return useQuery({
    queryKey: ['branchStatus', attemptId],
    queryFn: () => attemptsApi.getBranchStatus(attemptId!),
    enabled: !!attemptId && (options?.enabled ?? true),
    refetchInterval: 5000,
  });
}
