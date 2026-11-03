import { useEffect, useState, type FormEvent } from "react";
import { describeError } from "../state/errors";
import type { Transport } from "../transport";
import type { SpotifyStatus } from "../types/generated/SpotifyStatus";

/**
 * Settings → Music (issue #68): connect Spotify so "play <song>" works. The Client ID is
 * from the person's own Spotify developer app and is not a secret; the sign-in itself
 * happens in the system browser, and no token ever reaches this window.
 */
export function MusicSettings({ transport }: { transport: Transport }) {
  const [status, setStatus] = useState<SpotifyStatus | null>(null);
  const [clientId, setClientId] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    transport
      .spotifyStatus()
      .then((s) => {
        if (!live) return;
        setStatus(s);
        setClientId(s.clientId ?? "");
      })
      .catch((e: unknown) => live && setError(describeError(e)));
    return () => {
      live = false;
    };
  }, [transport]);

  const run = async (
    work: () => Promise<SpotifyStatus>,
    done: (s: SpotifyStatus) => string,
  ) => {
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      const s = await work();
      setStatus(s);
      setNotice(done(s));
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };

  const saveId = (e: FormEvent) => {
    e.preventDefault();
    void run(
      () => transport.spotifySetClientId(clientId.trim() || null),
      (s) => (s.clientId ? "Client ID saved." : "Client ID removed."),
    ).then(() => undefined);
  };

  const connect = async () => {
    setError(null);
    setNotice(null);
    setConnecting(true);
    try {
      const s = await transport.spotifyConnect();
      setStatus(s);
      setNotice("Connected to Spotify.");
    } catch (e) {
      setError(describeError(e));
    } finally {
      setConnecting(false);
    }
  };

  // Cancel ends a sign-in that is waiting for the browser; the pending connect rejects.
  const cancel = () => void transport.spotifyDisconnect().catch(() => {});
  const disconnect = () =>
    run(
      () => transport.spotifyDisconnect(),
      () => "Disconnected from Spotify.",
    );

  const saved = status?.clientId ?? null;
  const changed = clientId.trim().toLowerCase() !== (saved ?? "");

  return (
    <div className="music-settings">
      <h3>Spotify</h3>
      <p data-testid="spotify-status">
        {status === null
          ? "Loading…"
          : status.connected
            ? `Connected${status.displayName ? ` as ${status.displayName}` : ""}.`
            : "Not connected."}
      </p>
      <form onSubmit={saveId} autoComplete="off">
        <label htmlFor="spotify-client-id">Spotify Client ID</label>{" "}
        <input
          id="spotify-client-id"
          name="spotifyClientId"
          autoComplete="off"
          spellCheck={false}
          maxLength={64}
          value={clientId}
          onChange={(e) => setClientId(e.target.value)}
        />{" "}
        <button type="submit" disabled={busy || connecting || !changed}>
          Save Client ID
        </button>
      </form>
      <p>
        {connecting ? (
          <>
            <span role="status">Waiting for you to finish in the browser…</span>{" "}
            <button type="button" onClick={cancel}>
              Cancel
            </button>
          </>
        ) : status?.connected ? (
          <button
            type="button"
            disabled={busy}
            onClick={() => void disconnect()}
          >
            Disconnect
          </button>
        ) : (
          <button
            type="button"
            disabled={busy || !saved || changed}
            onClick={() => void connect()}
          >
            Connect Spotify
          </button>
        )}
      </p>
      {notice && <p role="status">{notice}</p>}
      {error && (
        <p role="alert" className="warning" data-testid="spotify-error">
          {error}
        </p>
      )}
      <p className="hint">
        To set it up: create a free app at developer.spotify.com/dashboard (it
        needs your Spotify Premium), tick “Web API”, set its Redirect URI to{" "}
        <code>http://127.0.0.1:43597/callback</code>, and paste its Client ID
        here. Choosing a song needs Spotify Premium and the Spotify app open.
      </p>
      <p className="hint">
        What is shared: the words you ask to play go to Spotify, and nothing is
        sent to Anthropic for “play …” commands. The sign-in is kept in your
        system keychain and can be removed with Disconnect.
      </p>
    </div>
  );
}
