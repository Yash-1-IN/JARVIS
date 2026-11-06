import { useEffect, useState } from "react";
import { describeError } from "../state/errors";
import type { Transport } from "../transport";
import type { ModelProgress } from "../types/generated/ModelProgress";
import type { VoiceStatus } from "../types/generated/VoiceStatus";

/**
 * The notice the person must see before the wake word is turned on (`docs/contracts.md`, "Wake word").
 * It may be added to, not shortened.
 */
export const WAKE_NOTICE =
  'The wake word uses openWakeWord\'s "hey jarvis" model, licensed CC BY-NC-SA 4.0 for personal, non-commercial use only. While it is on, JARVIS keeps the microphone open and listens for the phrase; sound is kept in memory only and is never saved. It is not included in the installer: it is downloaded to this computer when you turn it on.';

const MB = 1_000_000;

/**
 * Settings → Models → Wake word (D-29): a switch for "hey jarvis". Off by default. Turning it on shows
 * the licence notice first; accepting sends the notice and the switch together, and the files are
 * downloaded if they are not there yet. The backend enforces the gate as well, so this is never the
 * only check. Where the wake word is not offered (macOS for now) it says so instead of showing a switch.
 */
export function WakeWordSettings({ transport }: { transport: Transport }) {
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [notice, setNotice] = useState(false);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<ModelProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    transport
      .voiceStatus()
      .then((s) => live && setStatus(s))
      .catch((e: unknown) => live && setError(describeError(e)));
    const off = transport.onModelsProgress((p) => live && setProgress(p));
    return () => {
      live = false;
      off();
    };
  }, [transport]);

  /** Saves the choice, then fetches the wake files if they are not there yet. */
  const turnOn = async (acceptNotice: boolean) => {
    setError(null);
    setBusy(true);
    try {
      let s = await transport.voiceSettingsSet(
        acceptNotice
          ? { wakeNoticeAccepted: true, wakeEnabled: true }
          : { wakeEnabled: true },
      );
      setStatus(s);
      setNotice(false);
      if (!s.wakeReady) s = await transport.voiceModelsInstall();
      setStatus(s);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const turnOff = async () => {
    setError(null);
    setBusy(true);
    try {
      setStatus(await transport.voiceSettingsSet({ wakeEnabled: false }));
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };

  const percent =
    progress && progress.totalBytes > 0
      ? Math.min(
          100,
          Math.floor((progress.doneBytes / progress.totalBytes) * 100),
        )
      : 0;

  return (
    <div className="wake-settings">
      <h3>Wake word</h3>
      {status === null ? (
        <p>{error ? "" : "Loading…"}</p>
      ) : !status.wakeSupported ? (
        <p data-testid="wake-unsupported">
          The wake word is not available on this computer yet. Use the talk
          button to speak.
        </p>
      ) : (
        <>
          <p data-testid="wake-status">
            {!status.wakeEnabled
              ? "Off. JARVIS only listens when you press the talk button."
              : status.wakeReady
                ? "On. JARVIS is listening for “hey jarvis”; the microphone shows Waiting for wake word."
                : "On, but its files are not downloaded yet."}
          </p>
          {!status.wakeEnabled && !notice && (
            <p>
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  status.wakeNoticeAccepted
                    ? void turnOn(false)
                    : setNotice(true)
                }
              >
                Turn on the wake word
              </button>
            </p>
          )}
          {status.wakeEnabled && (
            <p>
              <button
                type="button"
                disabled={busy}
                onClick={() => void turnOff()}
              >
                Turn off the wake word
              </button>{" "}
              {!status.wakeReady && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void turnOn(false)}
                >
                  Download its files
                </button>
              )}
            </p>
          )}
          {notice && (
            <div
              role="group"
              aria-labelledby="wake-notice-title"
              className="wake-notice warning"
            >
              <p id="wake-notice-title">
                <strong>Before you turn it on</strong>
              </p>
              <p data-testid="wake-notice">{WAKE_NOTICE}</p>
              <p className="hint">
                A voice from a TV or speakers can also say the phrase and start
                a request. Medium and high-risk actions still always ask first.
              </p>
              <p>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void turnOn(true)}
                >
                  Accept and turn on
                </button>{" "}
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => setNotice(false)}
                >
                  Cancel
                </button>
              </p>
            </div>
          )}
          {busy && progress && (
            <p>
              <progress
                aria-label="Speech and wake word files download"
                max={100}
                value={percent}
              />{" "}
              <span role="status" data-testid="wake-progress">
                {Math.round(progress.doneBytes / MB)} of{" "}
                {Math.round(progress.totalBytes / MB)} MB ({percent}%)
              </span>
            </p>
          )}
        </>
      )}
      {error && (
        <p role="alert" className="warning" data-testid="wake-error">
          {error}
        </p>
      )}
      <p className="hint">
        Off by default. While it is on, the microphone stays open and sound is
        never saved or sent. It is for personal use only (D-29).
      </p>
    </div>
  );
}
