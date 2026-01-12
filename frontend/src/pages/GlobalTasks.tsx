import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  KanbanBoard,
  KanbanCards,
  KanbanHeader,
  KanbanProvider,
  type DragEndEvent,
} from '@/components/ui/shadcn-io/kanban';
import { AlertTriangle } from 'lucide-react';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Loader } from '@/components/ui/loader';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { statusBoardColors, statusLabels } from '@/utils/statusLabels';
import type { TaskWithAttemptStatus } from 'shared/types';
import { useProjects } from '@/hooks/useProjects';
import { useAllTasksStream } from '@/hooks/useAllTasksStream';
import { GlobalTaskCard } from '@/components/tasks/GlobalTaskCard';

const GLOBAL_STATUSES = [
  'todo',
  'inprogress',
  'inreview',
  'done',
  'cancelled',
] as const;
type GlobalStatus = (typeof GLOBAL_STATUSES)[number];

const ARCHIVED_LIMIT = 20;

const headerGradient = (status: GlobalStatus) =>
  `linear-gradient(hsl(var(${statusBoardColors[status]}) / 0.03), hsl(var(${statusBoardColors[status]}) / 0.03))`;

export function GlobalTasks() {
  const { t } = useTranslation(['tasks', 'common']);
  const {
    projects,
    projectsById,
    isLoading: isProjectsLoading,
    error: projectsError,
  } = useProjects();
  const [selectedProjectId, setSelectedProjectId] = useState<string>('all');
  const {
    tasks,
    isLoading: isTasksLoading,
    error: tasksError,
  } = useAllTasksStream();

  const filteredTasks = useMemo(() => {
    const scoped =
      selectedProjectId === 'all'
        ? tasks
        : tasks.filter((task) => task.project_id === selectedProjectId);
    return scoped.filter((task) =>
      GLOBAL_STATUSES.includes(task.status as GlobalStatus)
    );
  }, [selectedProjectId, tasks]);

  const columns = useMemo(() => {
    const map = GLOBAL_STATUSES.reduce(
      (acc, status) => {
        acc[status] = [];
        return acc;
      },
      {} as Record<GlobalStatus, TaskWithAttemptStatus[]>
    );

    filteredTasks.forEach((task) => {
      const status = task.status as GlobalStatus;
      if (!map[status]) return;
      map[status].push(task);
    });

    GLOBAL_STATUSES.forEach((status) => {
      map[status].sort(
        (a, b) =>
          new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
      );
      if (status === 'done' || status === 'cancelled') {
        map[status] = map[status].slice(0, ARCHIVED_LIMIT);
      }
    });

    return map;
  }, [filteredTasks]);

  const handleDragEnd = useCallback((_event: DragEndEvent) => {}, []);

  const error =
    projectsError ??
    (tasksError ? new Error(tasksError) : null);
  const isLoading = isProjectsLoading || isTasksLoading;
  const hasTasks = filteredTasks.length > 0;

  if (error) {
    return (
      <div className="p-4">
        <Alert>
          <AlertTitle className="flex items-center gap-2">
            <AlertTriangle size="16" />
            {t('common:states.error')}
          </AlertTitle>
          <AlertDescription>
            {error.message || 'Failed to load tasks'}
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  if (isLoading && !hasTasks) {
    return <Loader message={t('loading')} size={32} className="py-8" />;
  }

  return (
    <div className="min-h-full h-full flex flex-col">
      <div className="border-b px-4 py-3 flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-medium">All Tasks</div>
          <div className="text-xs text-muted-foreground">
            To Do, In Progress, In Review, Done, Cancelled
          </div>
        </div>
        <Select value={selectedProjectId} onValueChange={setSelectedProjectId}>
          <SelectTrigger className="w-56">
            <SelectValue placeholder="All Projects" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All Projects</SelectItem>
            {projects.length > 0 ? (
              projects.map((project) => (
                <SelectItem key={project.id} value={project.id}>
                  {project.name}
                </SelectItem>
              ))
            ) : (
              <SelectItem value="no-projects" disabled>
                No projects
              </SelectItem>
            )}
          </SelectContent>
        </Select>
      </div>

      {!hasTasks ? (
        <div className="flex-1 min-h-0 flex items-center justify-center text-sm text-muted-foreground">
          No tasks to show.
        </div>
      ) : (
        <div className="w-full h-full overflow-x-auto overflow-y-auto overscroll-x-contain p-4">
          <KanbanProvider onDragEnd={handleDragEnd}>
            {GLOBAL_STATUSES.map((status) => (
              <KanbanBoard key={status} id={status}>
                <KanbanHeader>
                  <div
                    className="sticky top-0 z-20 flex items-center gap-2 p-3 border-b border-dashed bg-background"
                    style={{
                      backgroundImage: headerGradient(status),
                    }}
                  >
                    <span className="flex-1 flex items-center gap-2">
                      <div
                        className="h-2 w-2 rounded-full"
                        style={{
                          backgroundColor: `hsl(var(${statusBoardColors[status]}))`,
                        }}
                      />
                      <p className="m-0 text-sm">{statusLabels[status]}</p>
                    </span>
                    <span className="text-xs text-muted-foreground">
                      {columns[status].length}
                    </span>
                  </div>
                </KanbanHeader>
                <KanbanCards>
                  {columns[status].map((task, index) => (
                    <GlobalTaskCard
                      key={task.id}
                      task={task}
                      index={index}
                      status={status}
                      projectName={projectsById[task.project_id]?.name}
                      dragDisabled={true}
                    />
                  ))}
                </KanbanCards>
              </KanbanBoard>
            ))}
          </KanbanProvider>
        </div>
      )}
    </div>
  );
}
