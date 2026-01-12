import { useCallback, useEffect, useMemo, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Alert, AlertDescription } from '@/components/ui/alert';
import {
  AlertCircle,
  ArrowLeft,
  Folder,
  FolderGit,
  Loader2,
  RefreshCcw,
} from 'lucide-react';
import { fileSystemApi, repoApi } from '@/lib/api';
import { DirectoryEntry, Repo } from 'shared/types';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { defineModal } from '@/lib/modals';
import { FolderPickerDialog } from './FolderPickerDialog';
import { useUserSystem } from '@/components/ConfigProvider';

export interface RepoPickerDialogProps {
  value?: string;
  title?: string;
  description?: string;
  workspaceRoot?: string;
}

export interface RepoPickerResult {
  repo: Repo;
  workspaceRoot: string;
}

type Stage = 'root' | 'existing' | 'new';

const RepoPickerDialogImpl = NiceModal.create<RepoPickerDialogProps>(
  ({
    title = 'Select Repository',
    description = 'Choose or create a git repository',
    workspaceRoot: workspaceRootProp,
  }) => {
    const modal = useModal();
    const { config, updateAndSaveConfig } = useUserSystem();
    const [stage, setStage] = useState<Stage>('existing');
    const [error, setError] = useState('');
    const [isWorking, setIsWorking] = useState(false);

    const configRoots = useMemo(
      () => config?.workspace_roots ?? [],
      [config?.workspace_roots]
    );
    const defaultRoot = useMemo(
      () =>
        workspaceRootProp ||
        config?.default_workspace_root ||
        '',
      [config?.default_workspace_root, workspaceRootProp]
    );
    const [workspaceRoots, setWorkspaceRoots] = useState<string[]>(configRoots);
    const [workspaceRoot, setWorkspaceRoot] = useState(defaultRoot);

    // Stage: existing
    const [allRepos, setAllRepos] = useState<DirectoryEntry[]>([]);
    const [reposLoading, setReposLoading] = useState(false);
    const [showMoreRepos, setShowMoreRepos] = useState(false);
    const [hasAutoFetchedRepos, setHasAutoFetchedRepos] = useState(false);
    const [repoFilter, setRepoFilter] = useState('');

    // Stage: new
    const [repoName, setRepoName] = useState('');
    const [parentPath, setParentPath] = useState('');

    useEffect(() => {
      if (modal.visible) {
        setStage(defaultRoot ? 'existing' : 'root');
        setError('');
        setAllRepos([]);
        setShowMoreRepos(false);
        setRepoFilter('');
        setRepoName('');
        setParentPath(defaultRoot);
        setHasAutoFetchedRepos(false);
        setWorkspaceRoots(configRoots);
        setWorkspaceRoot(defaultRoot);
      }
    }, [modal.visible, configRoots, defaultRoot]);

    useEffect(() => {
      setParentPath(workspaceRoot);
    }, [workspaceRoot]);

    const loadRecentRepos = useCallback(async () => {
      if (!workspaceRoot) {
        setError('Workspace root is required');
        return;
      }
      setReposLoading(true);
      setError('');
      try {
        const repos = await fileSystemApi.listGitRepos(workspaceRoot, 1);
        setAllRepos(repos);
      } catch (err) {
        setError('Failed to load repositories');
        console.error('Failed to load repos:', err);
      } finally {
        setReposLoading(false);
      }
    }, [workspaceRoot]);

    useEffect(() => {
      if (stage !== 'existing' || hasAutoFetchedRepos || reposLoading) {
        return;
      }
      if (!workspaceRoot) {
        return;
      }
      setHasAutoFetchedRepos(true);
      loadRecentRepos();
    }, [stage, hasAutoFetchedRepos, reposLoading, loadRecentRepos, workspaceRoot]);

    const registerAndReturn = async (path: string) => {
      if (!workspaceRoot) {
        setError('Workspace root is required');
        return;
      }
      setIsWorking(true);
      setError('');
      try {
        const repo = await repoApi.register({ path });
        modal.resolve({ repo, workspaceRoot });
        modal.hide();
      } catch (err) {
        setError(
          err instanceof Error ? err.message : 'Failed to register repository'
        );
      } finally {
        setIsWorking(false);
      }
    };

    const handleSelectRepo = (repo: DirectoryEntry) => {
      registerAndReturn(repo.path);
    };

    const handleCreateRepo = async () => {
      if (!repoName.trim()) {
        setError('Repository name is required');
        return;
      }
      if (!workspaceRoot) {
        setError('Workspace root is required');
        return;
      }

      setIsWorking(true);
      setError('');
      try {
        const repo = await repoApi.init({
          parent_path: workspaceRoot,
          folder_name: repoName.trim(),
        });
        modal.resolve({ repo, workspaceRoot });
        modal.hide();
      } catch (err) {
        setError(
          err instanceof Error ? err.message : 'Failed to create repository'
        );
      } finally {
        setIsWorking(false);
      }
    };

    const handleCancel = () => {
      modal.resolve(null);
      modal.hide();
    };

    const handleOpenChange = (open: boolean) => {
      if (!open && !isWorking) {
        handleCancel();
      }
    };

    const goBack = () => {
      setStage('existing');
      setError('');
    };

    const handleSelectWorkspaceRoot = async () => {
      const selectedPath = await FolderPickerDialog.show({
        title: 'Select Workspace Root',
        description: 'Choose a root folder that contains your repositories',
        value: workspaceRoot || undefined,
      });
      if (!selectedPath) return;

      const nextRoots = Array.from(
        new Set([...(workspaceRoots || []), selectedPath])
      );
      const nextDefault = selectedPath;
      const saved = await updateAndSaveConfig({
        workspace_roots: nextRoots,
        default_workspace_root: nextDefault,
      });
      if (!saved) {
        setError('Failed to save workspace root');
        return;
      }
      setWorkspaceRoots(nextRoots);
      setWorkspaceRoot(selectedPath);
      setStage('existing');
      setAllRepos([]);
      setHasAutoFetchedRepos(false);
      setShowMoreRepos(false);
      setRepoFilter('');
    };

    const filteredRepos = useMemo(() => {
      const query = repoFilter.trim().toLowerCase();
      if (!query) return allRepos;
      return allRepos.filter((repo) => {
        const name = repo.name.toLowerCase();
        const path = repo.path.toLowerCase();
        return name.includes(query) || path.includes(query);
      });
    }, [allRepos, repoFilter]);

    return (
      <div className="fixed inset-0 z-[10000] pointer-events-none [&>*]:pointer-events-auto">
        <Dialog open={modal.visible} onOpenChange={handleOpenChange}>
          <DialogContent className="max-w-[500px] w-full">
            <DialogHeader>
              <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
            </DialogHeader>

            <div className="space-y-4">
              {stage === 'root' && (
                <div className="space-y-3">
                  <p className="text-sm text-muted-foreground">
                    A workspace root is required. Select a folder that contains
                    your repositories.
                  </p>
                  <Button
                    type="button"
                    variant="secondary"
                    onClick={handleSelectWorkspaceRoot}
                    disabled={isWorking}
                    className="w-full"
                  >
                    Select Workspace Root
                  </Button>
                </div>
              )}

              {/* Stage: Existing */}
              {stage === 'existing' && (
                <>
                  <div className="space-y-2">
                    <Label>Workspace Root</Label>
                    <div className="flex items-center gap-2">
                      <Input
                        value={workspaceRoot || ''}
                        readOnly
                        className="flex-1"
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        onClick={handleSelectWorkspaceRoot}
                        disabled={isWorking}
                      >
                        <Folder className="h-4 w-4" />
                      </Button>
                    </div>
                  </div>

                  <div className="space-y-2">
                    <Label htmlFor="repo-filter">Filter Repositories</Label>
                    <div className="flex items-center gap-2">
                      <Input
                        id="repo-filter"
                        value={repoFilter}
                        onChange={(event) => setRepoFilter(event.target.value)}
                        placeholder="Search by name or path"
                        disabled={isWorking || reposLoading}
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        onClick={loadRecentRepos}
                        disabled={isWorking || reposLoading || !workspaceRoot}
                        aria-label="Refresh repositories"
                      >
                        <RefreshCcw className="h-4 w-4" />
                      </Button>
                    </div>
                  </div>

                  {reposLoading && (
                    <div className="p-4 border rounded-lg bg-card">
                      <div className="flex items-center gap-3">
                        <div className="animate-spin h-5 w-5 border-2 border-muted-foreground border-t-transparent rounded-full" />
                        <div className="text-sm text-muted-foreground">
                          Loading repositories...
                        </div>
                      </div>
                    </div>
                  )}

                  {!reposLoading && filteredRepos.length > 0 && (
                    <div className="space-y-2">
                      {filteredRepos
                        .slice(
                          0,
                          showMoreRepos ? filteredRepos.length : 3
                        )
                        .map((repo) => (
                          <div
                            key={repo.path}
                            className="p-4 border cursor-pointer hover:shadow-md transition-shadow rounded-lg bg-card"
                            onClick={() => !isWorking && handleSelectRepo(repo)}
                          >
                            <div className="flex items-start gap-3">
                              <FolderGit className="h-5 w-5 mt-0.5 flex-shrink-0 text-muted-foreground" />
                              <div className="min-w-0 flex-1">
                                <div className="font-medium text-foreground">
                                  {repo.name}
                                </div>
                                <div className="text-xs text-muted-foreground truncate mt-1">
                                  {repo.path}
                                </div>
                              </div>
                            </div>
                          </div>
                        ))}

                      {!showMoreRepos && filteredRepos.length > 3 && (
                        <button
                          className="text-sm text-muted-foreground hover:text-foreground transition-colors text-left"
                          onClick={() => setShowMoreRepos(true)}
                        >
                          Show {filteredRepos.length - 3} more repositories
                        </button>
                      )}
                      {showMoreRepos && filteredRepos.length > 3 && (
                        <button
                          className="text-sm text-muted-foreground hover:text-foreground transition-colors text-left"
                          onClick={() => setShowMoreRepos(false)}
                        >
                          Show less
                        </button>
                      )}
                    </div>
                  )}

                  {!reposLoading && filteredRepos.length === 0 && !error && (
                    <div className="p-4 border rounded-lg bg-card space-y-3">
                      <div className="text-sm text-muted-foreground">
                        {repoFilter
                          ? 'No repositories match this filter.'
                          : workspaceRoot
                            ? 'No git repositories found under this workspace root.'
                            : 'Select a workspace root to continue.'}
                      </div>
                      <Button
                        type="button"
                        variant="secondary"
                        onClick={loadRecentRepos}
                        disabled={isWorking || !workspaceRoot}
                      >
                        Reload
                      </Button>
                    </div>
                  )}

                  <div className="pt-2">
                    <Button
                      type="button"
                      variant="secondary"
                      onClick={() => setStage('new')}
                      disabled={isWorking || !workspaceRoot}
                      className="w-full"
                    >
                      Create New Repository
                    </Button>
                  </div>
                </>
              )}

              {/* Stage: New */}
              {stage === 'new' && (
                <>
                  <button
                    className="text-sm text-muted-foreground hover:text-foreground flex items-center gap-1"
                    onClick={goBack}
                    disabled={isWorking}
                  >
                    <ArrowLeft className="h-3 w-3" />
                    Back to repositories
                  </button>

                  <div className="space-y-4">
                    <div className="space-y-2">
                      <Label htmlFor="repo-name">
                        Repository Name <span className="text-red-500">*</span>
                      </Label>
                      <Input
                        id="repo-name"
                        type="text"
                        value={repoName}
                        onChange={(e) => setRepoName(e.target.value)}
                        placeholder="my-project"
                        disabled={isWorking}
                      />
                      <p className="text-xs text-muted-foreground">
                        This will be the folder name for your new repository
                      </p>
                    </div>

                    <div className="space-y-2">
                      <Label htmlFor="parent-path">Parent Directory</Label>
                    <div className="flex space-x-2">
                      <Input
                        id="parent-path"
                        type="text"
                        value={parentPath}
                        placeholder="Workspace Root"
                        className="flex-1"
                        disabled={true}
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        disabled={true}
                      >
                        <Folder className="h-4 w-4" />
                      </Button>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      New repositories are created directly under the workspace
                      root
                    </p>
                  </div>

                    <Button
                      onClick={handleCreateRepo}
                      disabled={isWorking || !repoName.trim()}
                      className="w-full"
                    >
                      {isWorking ? (
                        <>
                          <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                          Creating...
                        </>
                      ) : (
                        'Create Repository'
                      )}
                    </Button>
                  </div>
                </>
              )}

              {error && (
                <Alert variant="destructive">
                  <AlertCircle className="h-4 w-4" />
                  <AlertDescription>{error}</AlertDescription>
                </Alert>
              )}

              {isWorking && stage === 'existing' && (
                <div className="flex items-center justify-center gap-2 text-sm text-muted-foreground">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Registering repository...
                </div>
              )}
            </div>
          </DialogContent>
        </Dialog>
      </div>
    );
  }
);

export const RepoPickerDialog = defineModal<
  RepoPickerDialogProps,
  RepoPickerResult | null
>(RepoPickerDialogImpl);
