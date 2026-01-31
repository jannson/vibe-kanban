import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
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
  Folder,
  Loader2,
} from 'lucide-react';
import { repoApi } from '@/lib/api';
import { Repo } from 'shared/types';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { defineModal } from '@/lib/modals';
import { FolderPickerDialog } from './FolderPickerDialog';

export interface RepoPickerDialogProps {
  value?: string;
  title?: string;
  description?: string;
}

export interface RepoPickerResult {
  repo: Repo;
}

const RepoPickerDialogImpl = NiceModal.create<RepoPickerDialogProps>(
  ({
    title = 'Select Repository',
    description = 'Choose or create a git repository',
  }) => {
    const modal = useModal();
    const [error, setError] = useState('');
    const [isWorking, setIsWorking] = useState(false);

    useEffect(() => {
      if (modal.visible) {
        setError('');
      }
    }, [modal.visible]);

    const registerAndReturn = async (path: string) => {
      setIsWorking(true);
      setError('');
      try {
        const repo = await repoApi.register({ path });
        modal.resolve({ repo });
        modal.hide();
      } catch (err) {
        setError(
          err instanceof Error ? err.message : 'Failed to register repository'
        );
      } finally {
        setIsWorking(false);
      }
    };

    const handleSelectRepoFolder = async () => {
      const selectedPath = await FolderPickerDialog.show({
        title: 'Select Git Repository',
        description: 'Choose a git repository folder',
      });
      if (!selectedPath) return;
      await registerAndReturn(selectedPath);
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

    return (
      <div className="fixed inset-0 z-[10000] pointer-events-none [&>*]:pointer-events-auto">
        <Dialog open={modal.visible} onOpenChange={handleOpenChange}>
          <DialogContent className="max-w-[500px] w-full">
            <DialogHeader>
              <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
            </DialogHeader>

            <div className="space-y-4">
              <div className="space-y-2">
                <Label>Repository</Label>
                <div className="space-y-2">
                  <Button
                    type="button"
                    variant="secondary"
                    onClick={handleSelectRepoFolder}
                    disabled={isWorking}
                    className="w-full"
                  >
                    <Folder className="mr-2 h-4 w-4" />
                    Select Repository Folder
                  </Button>
                  <p className="text-xs text-muted-foreground">
                    Only folders that contain a .git entry are accepted.
                  </p>
                </div>
              </div>

              {error && (
                <Alert variant="destructive">
                  <AlertCircle className="h-4 w-4" />
                  <AlertDescription>{error}</AlertDescription>
                </Alert>
              )}

              {isWorking && (
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
