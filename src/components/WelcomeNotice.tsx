import { useEffect, useState, type KeyboardEvent } from "react";
import { describeError } from "../state/errors";
import type { Transport } from "../transport";

const SEEN_KEY = "jarvis.welcomeSeen";

function seen(): boolean {
  try {
    return localStorage.getItem(SEEN_KEY) === "1";
  } catch {
    return false; // unreadable storage counts as "not seen yet"
  }
}

function markSeen(): void {
  try {
    localStorage.setItem(SEEN_KEY, "1");
  } catch {
    // Storage unavailable: the notice may show again next time; nothing breaks.
  }
}

/**
 * The one decision on first launch (issue #45): keep the approval cards for
 * routine file actions, or switch them off (`autoAllowMedium`). Both buttons
 * have the same weight and neither has focus; Esc or Close means "Keep
 * asking". The same switch stays in Settings → Permissions.
 */
export function WelcomeNotice({ transport }: { transport: Transport }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (seen()) return;
    let live = true;
    Promise.resolve()
      .then(() => transport.getSettings())
      .then((s) => {
        if (!live) return;
        // Someone who already switched it on has made this decision.
        if (s.autoAllowMedium) markSeen();
        else setOpen(true);
      })
      // Without settings there is nothing safe to offer; ask again next launch.
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [transport]);

  if (!open) return null;

  const keepAsking = () => {
    markSeen();
    setOpen(false);
  };
  const stopAsking = async () => {
    setBusy(true);
    setError(null);
    try {
      await transport.updateSettings({ autoAllowMedium: true });
      markSeen();
      setOpen(false);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape" && !e.repeat) {
      // Esc here closes the notice; it must not also cancel a reply.
      e.stopPropagation();
      keepAsking();
    }
  };

  return (
    <section
      className="welcome"
      role="region"
      aria-labelledby="welcome-title"
      onKeyDown={onKeyDown}
      data-testid="welcome"
    >
      <header className="welcome-header">
        <h2 id="welcome-title">One decision before we begin</h2>
        <button
          type="button"
          className="close"
          onClick={keepAsking}
          aria-label="Close (keeps asking)"
        >
          Close <kbd>Esc</kbd>
        </button>
      </header>
      <p>
        By default I ask before I read or move your files. If you would rather I
        just got on with it, I can stop asking for routine actions: reading,
        moving and renaming single files inside the folders you allowed. Reading
        a file sends its contents to the AI provider, and you will no longer see
        a card for it.
      </p>
      <p>
        I will still ask after I have read anything from outside, and always for
        bigger actions such as moving a whole folder. Allowed folders and apps
        still apply, and everything I do is listed in Activity.
      </p>
      <p className="hint">
        You can change this any time in Settings → Permissions.
      </p>
      <div className="welcome-actions">
        <button type="button" disabled={busy} onClick={keepAsking}>
          Keep asking
        </button>
        <button type="button" disabled={busy} onClick={() => void stopAsking()}>
          Stop asking for routine actions
        </button>
      </div>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
