import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";

interface Props {
  id: string;
  title: string;
  onClose: () => void;
  /** A pop-up window: a dialog that keeps Tab inside it. Otherwise a region in the page. */
  modal?: boolean;
  children: ReactNode;
}

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * One panel beside the conversation (issue #37). Focus moves to its heading
 * when it opens; Esc inside it, or Close, returns to the chat. The opener puts
 * focus back on its own button.
 */
export function SidePanel({ id, title, onClose, modal, children }: Props) {
  const heading = useRef<HTMLHeadingElement>(null);
  const root = useRef<HTMLElement>(null);
  const headingId = `${id}-title`;

  useEffect(() => {
    heading.current?.focus({ preventScroll: true });
  }, [id]);

  const onKeyDown = (e: KeyboardEvent) => {
    // In a pop-up, Tab wraps inside it: the page behind is not reachable.
    if (modal && e.key === "Tab" && root.current) {
      const items = [
        ...root.current.querySelectorAll<HTMLElement>(FOCUSABLE),
      ].filter((el) => !el.closest("[hidden]"));
      if (items.length > 0) {
        const first = items[0];
        const last = items[items.length - 1];
        const at = document.activeElement;
        if (
          e.shiftKey &&
          (at === first || at === root.current || at === heading.current)
        ) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && at === last) {
          e.preventDefault();
          first.focus();
        }
      }
    }
    // Esc closes the panel instead of cancelling a request; the global
    // shortcut listener on window never sees this key press.
    if (e.key === "Escape" && !e.repeat) {
      e.stopPropagation();
      onClose();
    }
  };

  return (
    <section
      id={id}
      ref={root}
      className="side-panel"
      role={modal ? "dialog" : "region"}
      aria-modal={modal ? true : undefined}
      aria-labelledby={headingId}
      onKeyDown={onKeyDown}
    >
      <header className="side-panel-header">
        <h2 id={headingId} ref={heading} tabIndex={-1}>
          {title}
        </h2>
        <button type="button" className="close" onClick={onClose}>
          Close <kbd>Esc</kbd>
        </button>
      </header>
      <div className="side-panel-body">{children}</div>
    </section>
  );
}
