import { useCallback, useEffect, useState } from "react";
import { describeError } from "../state/errors";
import type { Transport } from "../transport";
import type { AudioDevices } from "../types/generated/AudioDevices";

/** Names that usually mean a wireless headset, whose microphone is thin and hurts recognition. */
const HEADSET_NAME =
  /bluetooth|headset|headphone|airpods|buds|earbud|hands-?free/i;

/**
 * Settings → Models → Microphone: which microphone JARVIS listens to, and which one it is really
 * using. Wireless headsets often make the computer switch to a thin "headset mode" microphone that
 * speech recognition handles badly (S1 report), so the choice is the person's, made on purpose.
 */
export function MicrophoneSettings({ transport }: { transport: Transport }) {
  const [devices, setDevices] = useState<AudioDevices | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchAll = useCallback(
    () => Promise.all([transport.voiceDevices(), transport.voiceStatus()]),
    [transport],
  );

  useEffect(() => {
    let live = true;
    fetchAll()
      .then(([d, s]) => {
        if (!live) return;
        setDevices(d);
        setChosen(s.inputDevice);
      })
      .catch((e: unknown) => live && setError(describeError(e)));
    return () => {
      live = false;
    };
  }, [fetchAll]);

  /** The Refresh button: look again (a headset may have been connected or removed). */
  const refresh = async () => {
    setError(null);
    try {
      const [d, s] = await fetchAll();
      setDevices(d);
      setChosen(s.inputDevice);
    } catch (e) {
      setError(describeError(e));
    }
  };

  const choose = async (value: string) => {
    setError(null);
    setBusy(true);
    try {
      const s = await transport.voiceSettingsSet({
        inputDevice: value === "" ? null : value,
      });
      setChosen(s.inputDevice);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };

  // A saved name that is no longer plugged in falls back to the default, as the backend does.
  const saved = chosen !== null && devices?.inputs.includes(chosen);
  const inUse = saved ? chosen : (devices?.defaultInput ?? null);
  const looksWireless = inUse !== null && HEADSET_NAME.test(inUse);

  return (
    <div className="microphone-settings">
      <h3>Microphone</h3>
      {devices === null ? (
        <p>{error ? "" : "Loading…"}</p>
      ) : devices.inputs.length === 0 ? (
        <p data-testid="mic-none">No microphone was found.</p>
      ) : (
        <>
          <p>
            <label htmlFor="mic-select">Microphone to listen to</label>{" "}
            <select
              id="mic-select"
              value={saved ? (chosen ?? "") : ""}
              disabled={busy}
              onChange={(e) => void choose(e.target.value)}
            >
              <option value="">
                System default
                {devices.defaultInput ? ` (${devices.defaultInput})` : ""}
              </option>
              {devices.inputs.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>{" "}
            <button
              type="button"
              disabled={busy}
              onClick={() => void refresh()}
            >
              Refresh list
            </button>
          </p>
          <p data-testid="mic-in-use">
            In use: {inUse ?? "unknown"}
            {chosen !== null && !saved && " (the saved one is not connected)"}
          </p>
          {looksWireless && (
            <p role="note" className="warning" data-testid="mic-wireless">
              This looks like a wireless headset. Its microphone is often thin
              and makes JARVIS mishear you. If that happens, pick the
              computer&apos;s own microphone above.
            </p>
          )}
        </>
      )}
      {error && (
        <p role="alert" className="warning" data-testid="mic-error">
          {error}
        </p>
      )}
      <p className="hint">
        Changing it applies to the next time you talk. Nothing you say is saved
        or sent when you pick a microphone.
      </p>
    </div>
  );
}
