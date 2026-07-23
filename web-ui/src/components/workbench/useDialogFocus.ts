import { useCallback, useEffect, useRef } from 'react';
import type { KeyboardEvent } from 'react';

const focusableSelector = [
  'button:not(:disabled)',
  'input:not(:disabled)',
  'select:not(:disabled)',
  'textarea:not(:disabled)',
  '[href]',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

export function useDialogFocus<T extends HTMLElement>(open: boolean, onClose: () => void) {
  const dialogRef = useRef<T>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    if (!open) return;
    previouslyFocusedRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const timer = window.setTimeout(() => {
      const dialog = dialogRef.current;
      const preferred = dialog?.querySelector<HTMLElement>('[data-dialog-autofocus]');
      const first = dialog?.querySelector<HTMLElement>(focusableSelector);
      (preferred ?? first ?? dialog)?.focus();
    }, 0);
    return () => {
      window.clearTimeout(timer);
      previouslyFocusedRef.current?.focus();
      previouslyFocusedRef.current = null;
    };
  }, [open]);

  const onDialogKeyDown = useCallback((event: KeyboardEvent<T>) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onCloseRef.current();
      return;
    }
    if (event.key !== 'Tab') return;
    const focusable = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>(focusableSelector),
    ).filter((item) => !item.hasAttribute('disabled') && item.getAttribute('aria-hidden') !== 'true');
    if (focusable.length === 0) {
      event.preventDefault();
      event.currentTarget.focus();
      return;
    }
    const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
    const nextIndex = event.shiftKey
      ? (currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1)
      : (currentIndex < 0 || currentIndex === focusable.length - 1 ? 0 : currentIndex + 1);
    event.preventDefault();
    focusable[nextIndex]?.focus();
  }, []);

  return { dialogRef, onDialogKeyDown };
}
