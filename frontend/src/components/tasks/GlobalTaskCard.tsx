import { useCallback } from 'react';
import { Loader2, XCircle } from 'lucide-react';
import { KanbanCard } from '@/components/ui/shadcn-io/kanban';
import { Badge } from '@/components/ui/badge';
import { TaskCardHeader } from '@/components/tasks/TaskCardHeader';
import { useNavigateWithSearch } from '@/hooks';
import { paths } from '@/lib/paths';
import type { TaskStatus, TaskWithAttemptStatus } from 'shared/types';

interface GlobalTaskCardProps {
  task: TaskWithAttemptStatus;
  index: number;
  status: TaskStatus;
  projectName?: string;
  dragDisabled?: boolean;
}

export function GlobalTaskCard({
  task,
  index,
  status,
  projectName,
  dragDisabled = false,
}: GlobalTaskCardProps) {
  const navigate = useNavigateWithSearch();
  const handleClick = useCallback(() => {
    navigate(paths.task(task.project_id, task.id));
  }, [navigate, task.id, task.project_id]);

  return (
    <KanbanCard
      id={task.id}
      name={task.title}
      index={index}
      parent={status}
      onClick={handleClick}
      dragDisabled={dragDisabled}
    >
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between gap-2">
          <Badge variant="secondary" className="max-w-[70%] truncate">
            {projectName ?? 'Unknown Project'}
          </Badge>
          <div className="flex items-center gap-1 shrink-0">
            {task.has_in_progress_attempt && (
              <Loader2 className="h-4 w-4 animate-spin text-blue-500" />
            )}
            {task.last_attempt_failed && (
              <XCircle className="h-4 w-4 text-destructive" />
            )}
          </div>
        </div>
        <TaskCardHeader title={task.title} />
        {task.description && (
          <p className="text-sm text-secondary-foreground break-words">
            {task.description.length > 130
              ? `${task.description.substring(0, 130)}...`
              : task.description}
          </p>
        )}
      </div>
    </KanbanCard>
  );
}
