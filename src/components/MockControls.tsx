// Type-only imports: a value import here would pull mock code into release builds.
import type { MockFault } from "../transport/mock/faults";
import type { MockTransport } from "../transport/mock/MockTransport";

/** Dev-only controls: replay scripted scenarios and inject M2 faults. */
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
      <button
        type="button"
        title="Log a refused tool call, as when the model reaches for a tool that does not exist"
        onClick={() => transport.simulateRefusedCall()}
      >
        activity: refused call
      </button>
      <button
        type="button"
        title="Interrupt the running reply as if the wake word was heard"
        onClick={() => transport.interrupt("wake_word")}
      >
        interrupt: wake word
      </button>
      <button
        type="button"
        title="Pretend the OS keychain is unavailable (D-09 warning path); reopen Settings to see it"
        onClick={() => transport.simulateSessionOnlyKey()}
      >
        key: session-only
      </button>
      <label>
        Next message fails (once) with{" "}
        <select
          defaultValue=""
          onChange={(e) =>
            transport.failNextTurn((e.target.value || null) as MockFault | null)
          }
        >
          <option value="">nothing</option>
          {transport.faults.map((f) => (
            <option key={f} value={f}>
              {f}
            </option>
          ))}
        </select>
      </label>
    </section>
  );
}
