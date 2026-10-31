/** Persistent marker so mock behaviour is never mistaken for the real backend. */
export function MockBadge() {
  return (
    <div className="mock-badge" role="note" aria-label="Mock backend in use">
      MOCK
    </div>
  );
}
