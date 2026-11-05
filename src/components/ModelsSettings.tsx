import { useEffect, useState } from "react";
import { describeError } from "../state/errors";
import type { Transport } from "../transport";
import type { ModelProgress } from "../types/generated/ModelProgress";
import type { VoiceStatus } from "../types/generated/VoiceStatus";

const MB = 1_000_000;
const mb = (bytes: number) => Math.round(bytes / MB);

/**
 * Settings → Models (M3 step 8): download the speech models once, so push to talk can turn
 * speech into text. The backend downloads from the project's official release, checks every
 * file against a checksum pinned in the app, and only then installs; this panel shows the
 * progress and the result. Nothing here carries audio.
 */
export function ModelsSettings({ transport }: { transport: Transport }) {
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [progress, setProgress] = useState<ModelProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    transport
      .voiceStatus()
      .then((s) => live && setStatus(s))
      .catch((e: unknown) => live && setError(describeError(e)));
    // Progress also arrives if a download was started elsewhere (for example from the console).
    const off = transport.onModelsProgress((p) => live && setProgress(p));
    return () => {
      live = false;
      off();
    };
  }, [transport]);

  const download = async () => {
    setError(null);
    setProgress(null);
    setBusy(true);
    try {
      setStatus(await transport.voiceModelsInstall());
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
      setProgress(null);
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
    <div className="models-settings">
      <h3>Speech models</h3>
      <p data-testid="models-status">
        {status === null
          ? "Loading…"
          : status.modelsReady
            ? "The speech models are installed."
            : "The speech models are not installed yet."}
      </p>
      {status && !status.modelsReady && (
        <p>
          <button type="button" disabled={busy} onClick={() => void download()}>
            {busy ? "Downloading…" : "Download speech models"}
          </button>
        </p>
      )}
      {busy && (
        <p>
          <progress
            aria-label="Speech model download"
            max={100}
            value={progress ? percent : undefined}
          />{" "}
          <span role="status" data-testid="models-progress">
            {progress
              ? `${mb(progress.doneBytes)} of ${mb(progress.totalBytes)} MB (${percent}%)`
              : "Starting…"}
          </span>
        </p>
      )}
      {error && (
        <p role="alert" className="warning" data-testid="models-error">
          {error}
        </p>
      )}
      <p className="hint">
        Push to talk needs these to turn what you say into text. It is a
        one-time download of about 250 MB from the official sherpa-onnx releases
        on github.com. JARVIS checks the file against a checksum built into the
        app before it uses it, and throws it away if it does not match. If the
        download is interrupted, press the button again and it continues where
        it stopped.
      </p>
      <p className="hint">
        What is shared: your computer asks github.com for the file, which sees
        your internet address like any download. Nothing you say is sent, and
        audio is never saved.
      </p>
    </div>
  );
}
