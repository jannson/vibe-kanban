import React, { createContext, useCallback, useContext, useRef } from 'react';

type CollapseHandler = () => void;

type LogsCollapseContextType = {
  collapseAllRuns: () => void;
  setCollapseHandler: (handler: CollapseHandler | null) => void;
};

const LogsCollapseContext = createContext<LogsCollapseContextType | null>(null);

export const LogsCollapseProvider: React.FC<{
  children: React.ReactNode;
}> = ({ children }) => {
  const handlerRef = useRef<CollapseHandler | null>(null);

  const setCollapseHandler = useCallback((handler: CollapseHandler | null) => {
    handlerRef.current = handler;
  }, []);

  const collapseAllRuns = useCallback(() => {
    handlerRef.current?.();
  }, []);

  return (
    <LogsCollapseContext.Provider
      value={{ collapseAllRuns, setCollapseHandler }}
    >
      {children}
    </LogsCollapseContext.Provider>
  );
};

export const useLogsCollapse = () => {
  return useContext(LogsCollapseContext);
};
