import { useEffect } from "react";

export type ShortcutAction = "start" | "stop" | "restart" | "logs";

interface UseKeyboardShortcutsOptions {
  onStart?: () => void;
  onStop?: () => void;
  onRestart?: () => void;
  onLogs?: () => void;
}

export function useKeyboardShortcuts({
  onStart,
  onStop,
  onRestart,
  onLogs,
}: UseKeyboardShortcutsOptions) {
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      // Ignore when typing in an input/textarea
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

      switch (e.key.toLowerCase()) {
        case "s":
          onStart?.();
          break;
        case "x":
          onStop?.();
          break;
        case "r":
          onRestart?.();
          break;
        case "l":
          onLogs?.();
          break;
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onStart, onStop, onRestart, onLogs]);
}
