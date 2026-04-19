import { useEffect } from 'react';
import { useLexicalComposerContext } from '@lexical/react/LexicalComposerContext';
import {
  KEY_MODIFIER_COMMAND,
  KEY_ENTER_COMMAND,
  COMMAND_PRIORITY_NORMAL,
  COMMAND_PRIORITY_HIGH,
} from 'lexical';

type Props = {
  onCmdEnter?: () => void;
  onShiftCmdEnter?: () => void;
  onEnter?: () => void;
};

export function KeyboardCommandsPlugin({
  onCmdEnter,
  onShiftCmdEnter,
  onEnter,
}: Props) {
  const [editor] = useLexicalComposerContext();

  useEffect(() => {
    if (!onCmdEnter && !onShiftCmdEnter && !onEnter) return;

    // Handle the modifier command to trigger the callbacks
    const unregisterModifier = editor.registerCommand(
      KEY_MODIFIER_COMMAND,
      (event: KeyboardEvent) => {
        if (event.isComposing) {
          return false;
        }

        if (!(event.metaKey || event.ctrlKey) || event.key !== 'Enter') {
          return false;
        }

        event.preventDefault();
        event.stopPropagation();

        if (event.shiftKey && onShiftCmdEnter) {
          onShiftCmdEnter();
          return true;
        }

        if (!event.shiftKey && onCmdEnter) {
          onCmdEnter();
          return true;
        }

        return false;
      },
      COMMAND_PRIORITY_NORMAL
    );

    // Block KEY_ENTER_COMMAND when CMD/Ctrl is pressed to prevent
    // RichTextPlugin from inserting a new line
    const unregisterEnter = editor.registerCommand(
      KEY_ENTER_COMMAND,
      (event: KeyboardEvent | null) => {
        if (!event) return false;
        if (event.isComposing) return false;

        if (!event.metaKey && !event.ctrlKey && !event.shiftKey && onEnter) {
          event.preventDefault();
          event.stopPropagation();
          onEnter();
          return true;
        }

        if (event.metaKey || event.ctrlKey) {
          return true; // Mark as handled, preventing line break insertion
        }
        return false;
      },
      COMMAND_PRIORITY_HIGH
    );

    return () => {
      unregisterModifier();
      unregisterEnter();
    };
  }, [editor, onCmdEnter, onShiftCmdEnter, onEnter]);

  return null;
}
