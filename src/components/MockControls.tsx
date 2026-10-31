import type { MockTransport } from "../transport/mock/MockTransport";

/** Dev-only buttons to replay the scripted scenarios from spec §13 (M1). */
export function MockControls({ transport }: { transport: MockTransport }) {
  return (
    <section aria-label="Mock scenarios">
      {transport.scripts.map((s) => (
        <button
          key={s.name}
          type="button"
          title={s.description}
          onClick={() => transport.runScript(s.name)}
        >
          {s.name}
        </button>
      ))}
    </section>
  );
}
