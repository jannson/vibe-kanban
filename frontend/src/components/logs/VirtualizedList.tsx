import AutoSizer from 'react-virtualized-auto-sizer';
import type { HTMLAttributes } from 'react';
import { forwardRef, useEffect, useMemo, useRef, useState } from 'react';
import {
  VariableSizeList,
  ListChildComponentProps,
} from 'react-window';

import DisplayConversationEntry from '../NormalizedConversation/DisplayConversationEntry';
import { useEntries } from '@/contexts/EntriesContext';
import {
  AddEntryType,
  PatchTypeWithKey,
  useConversationHistory,
} from '@/hooks/useConversationHistory';
import { Loader2 } from 'lucide-react';
import { TaskWithAttemptStatus } from 'shared/types';
import type { WorkspaceWithSession } from '@/types/attempt';
import { ApprovalFormProvider } from '@/contexts/ApprovalFormContext';

interface VirtualizedListProps {
  attempt: WorkspaceWithSession;
  task?: TaskWithAttemptStatus;
}

interface MessageListContext {
  attempt: WorkspaceWithSession;
  task?: TaskWithAttemptStatus;
}

const ESTIMATED_ROW_HEIGHT = 72;

const renderItemContent = (
  data: PatchTypeWithKey,
  context: MessageListContext
) => {
  const attempt = context.attempt;
  const task = context.task;

  if (data.type === 'STDOUT') {
    return <p>{data.content}</p>;
  }
  if (data.type === 'STDERR') {
    return <p>{data.content}</p>;
  }
  if (data.type === 'NORMALIZED_ENTRY') {
    return (
      <DisplayConversationEntry
        expansionKey={data.patchKey}
        entry={data.content}
        executionProcessId={data.executionProcessId}
        taskAttempt={attempt}
        task={task}
      />
    );
  }

  return null;
};

const InnerElement = forwardRef<HTMLDivElement, HTMLAttributes<HTMLDivElement>>(
  ({ style, ...rest }, ref) => (
    <div
      ref={ref}
      style={{ ...style, paddingTop: 8, paddingBottom: 8 }}
      {...rest}
    />
  )
);
InnerElement.displayName = 'VirtualizedListInner';

interface RowData {
  items: PatchTypeWithKey[];
  context: MessageListContext;
  setSize: (index: number, size: number) => void;
}

const Row = ({ index, style, data }: ListChildComponentProps<RowData>) => {
  const item = data.items[index];
  const rowRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!rowRef.current) return;
    const node = rowRef.current;

    const updateSize = () => {
      const next = node.getBoundingClientRect().height;
      data.setSize(index, next);
    };

    updateSize();

    const observer = new ResizeObserver(updateSize);
    observer.observe(node);
    return () => observer.disconnect();
  }, [data, index, item]);

  return (
    <div style={{ ...style, width: '100%' }}>
      <div ref={rowRef} className="px-4">
        {renderItemContent(item, data.context)}
      </div>
    </div>
  );
};

const VirtualizedList = ({ attempt, task }: VirtualizedListProps) => {
  const [channelData, setChannelData] = useState<PatchTypeWithKey[] | null>(
    null
  );
  const [loading, setLoading] = useState(true);
  const { setEntries, reset } = useEntries();
  const listRef = useRef<VariableSizeList>(null);
  const sizeMapRef = useRef<Record<number, number>>({});
  const scrollToBottomRef = useRef(false);

  useEffect(() => {
    setLoading(true);
    setChannelData(null);
    reset();
  }, [attempt.id, reset]);

  const onEntriesUpdated = (
    newEntries: PatchTypeWithKey[],
    _addType: AddEntryType,
    newLoading: boolean
  ) => {
    scrollToBottomRef.current = true;

    setChannelData(newEntries);
    setEntries(newEntries);

    if (loading) {
      setLoading(newLoading);
    }
  };

  useConversationHistory({ attempt, onEntriesUpdated });

  const messageListContext = useMemo(
    () => ({ attempt, task }),
    [attempt, task]
  );
  const items = channelData ?? [];

  useEffect(() => {
    sizeMapRef.current = {};
    listRef.current?.resetAfterIndex(0, true);
  }, [attempt.id]);

  useEffect(() => {
    if (!items.length) return;
    if (scrollToBottomRef.current) {
      listRef.current?.scrollToItem(items.length - 1, 'end');
      scrollToBottomRef.current = false;
    }
  }, [items.length]);

  const setSize = (index: number, size: number) => {
    const current = sizeMapRef.current[index];
    if (current === size) return;
    sizeMapRef.current[index] = size;
    listRef.current?.resetAfterIndex(index);
  };

  const getItemSize = (index: number) =>
    sizeMapRef.current[index] ?? ESTIMATED_ROW_HEIGHT;

  return (
    <ApprovalFormProvider>
      <div className="flex-1">
        <AutoSizer>
          {({ height, width }) => (
            <VariableSizeList
              ref={listRef}
              height={height}
              width={width}
              itemCount={items.length}
              itemSize={getItemSize}
              estimatedItemSize={ESTIMATED_ROW_HEIGHT}
              itemData={{
                items,
                context: messageListContext,
                setSize,
              }}
              innerElementType={InnerElement}
            >
              {Row}
            </VariableSizeList>
          )}
        </AutoSizer>
      </div>
      {loading && (
        <div className="float-left top-0 left-0 w-full h-full bg-primary flex flex-col gap-2 justify-center items-center">
          <Loader2 className="h-8 w-8 animate-spin" />
          <p>Loading History</p>
        </div>
      )}
    </ApprovalFormProvider>
  );
};

export default VirtualizedList;
