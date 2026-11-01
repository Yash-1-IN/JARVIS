import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";

interface Props {
  id: string;
  title: string;
  onClose: () => void;
  children: ReactNode;
}

/**
 * One panel beside the conversation (issue #37). Focus moves to its heading
 * when it opens; Esc inside it, or Close, returns to the chat. The opener puts
 * focus back on its own button.
 */
export function SidePanel({ id, title, onClose, children }: Props) {
  const heading = useRef<HTMLHeadingElement>(null);
  const headingId = `${id}-title`;

  useEffect(() => {
    heading.current?.focus();
  }, [id]);

  const onKeyDown = (e: KeyboardEvent) => {
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
      className="side-panel"
      role="region"
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
