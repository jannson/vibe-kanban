import { useMutation } from '@tanstack/react-query';
import { attemptsApi } from '@/lib/api';

export function useResumeTaskAttempt() {
  return useMutation({
    mutationFn: (attemptId: string) => attemptsApi.resumeTaskAttempt(attemptId),
  });
}
