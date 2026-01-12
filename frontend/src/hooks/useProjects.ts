import { useCallback, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useJsonPatchWsStream } from './useJsonPatchWsStream';
import type { Project } from 'shared/types';
import { projectsApi } from '@/lib/api';

type ProjectsState = {
  projects: Record<string, Project>;
};

export interface UseProjectsResult {
  projects: Project[];
  projectsById: Record<string, Project>;
  isLoading: boolean;
  isConnected: boolean;
  error: Error | null;
}

export function useProjects(): UseProjectsResult {
  const endpoint = '/api/projects/stream/ws';

  const initialData = useCallback((): ProjectsState => ({ projects: {} }), []);

  const { data, isConnected, error } = useJsonPatchWsStream<ProjectsState>(
    endpoint,
    true,
    initialData
  );

  const {
    data: fallbackProjects,
    isLoading: isFallbackLoading,
    error: fallbackError,
  } = useQuery({
    queryKey: ['projects', 'list'],
    queryFn: () => projectsApi.getAll(),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  const projectsById = useMemo(() => {
    if (data?.projects && Object.keys(data.projects).length > 0) {
      return data.projects;
    }
    const map: Record<string, Project> = {};
    (fallbackProjects ?? []).forEach((project) => {
      map[project.id] = project;
    });
    return map;
  }, [data?.projects, fallbackProjects]);

  const projects = useMemo(() => {
    return Object.values(projectsById).sort(
      (a, b) =>
        new Date(b.created_at as unknown as string).getTime() -
        new Date(a.created_at as unknown as string).getTime()
    );
  }, [projectsById]);

  const projectsData = Object.keys(projectsById).length ? projects : undefined;
  const errorObj = useMemo(() => {
    if (error && !projectsData) {
      return new Error(error);
    }
    if (fallbackError && !projectsData) {
      return fallbackError as Error;
    }
    return null;
  }, [error, fallbackError, projectsData]);

  return {
    projects: projectsData ?? [],
    projectsById,
    isLoading: !projectsData && !errorObj && isFallbackLoading,
    isConnected,
    error: errorObj,
  };
}
