import { useEffect, useState, type FormEvent } from "react";
import { describeError } from "../state/errors";
import type { Transport } from "../transport";
import type { AllowedApp } from "../types/generated/AllowedApp";
import type { Settings } from "../types/generated/Settings";
import type { SettingsPatch } from "../types/generated/SettingsPatch";

interface Props {
  transport: Transport;
  settings: Settings;
  /** Sends a patch through the parent so status and errors stay in one place. */
  save: (patch: SettingsPatch, done: string) => Promise<boolean>;
  busy: boolean;
}

/**
 * What JARVIS may touch (spec §9.4, contract #19): allowed folders, allowed apps and the
 * medium-risk auto-allow switch. Apps are picked from the backend's installed list by id;
 * the id is shown next to every name, because a shortcut or bundle can be *named* anything
 * and the id is what actually gets allowed (Dev A's note on #21).
 */
export function PolicySettings({ transport, settings, save, busy }: Props) {
  const [folder, setFolder] = useState("");
  const [installed, setInstalled] = useState<AllowedApp[] | null>(null);
  const [appsError, setAppsError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  useEffect(() => {
    let live = true;
    transport
      .appsInstalled()
      .then((a) => live && setInstalled(a))
      .catch((e: unknown) => live && setAppsError(describeError(e)));
    return () => {
      live = false;
    };
  }, [transport]);

  const allowedIds = new Set(settings.allowedApps.map((a) => a.id));

  const addFolder = (e: FormEvent) => {
    e.preventDefault();
    const f = folder.trim();
    if (!f || settings.allowedFolders.includes(f)) return;
    // Keep what was typed if the backend refuses it (not absolute, missing, too many).
    void save(
      { allowedFolders: [...settings.allowedFolders, f] },
      "Folder added.",
    ).then((ok) => ok && setFolder(""));
  };

  const removeFolder = (f: string) =>
    save(
      { allowedFolders: settings.allowedFolders.filter((x) => x !== f) },
      "Folder removed.",
    );

  const toggleApp = (app: AllowedApp, on: boolean) => {
    const next = on
      ? [...settings.allowedApps, app]
      : settings.allowedApps.filter((a) => a.id !== app.id);
    // The backend keeps only installed ids and takes names from its own list.
    void save(
      { allowedApps: next },
      on ? `${app.name} allowed.` : `${app.name} removed.`,
    );
  };

  const shown = (installed ?? []).filter((a) => {
    const q = filter.trim().toLowerCase();
    return (
      !q || a.name.toLowerCase().includes(q) || a.id.toLowerCase().includes(q)
    );
  });
  // Allowed apps that are no longer installed stay visible so they can be removed.
  const orphans = settings.allowedApps.filter(
    (a) => !(installed ?? []).some((i) => i.id === a.id),
  );

  return (
    <>
      <h3>Allowed folders</h3>
      <p className="hint">
        JARVIS can search, read, move and rename files only inside these
        folders. Reading and moving still ask you first unless you allow it
        below.
      </p>
      {settings.allowedFolders.length === 0 ? (
        <p data-testid="no-folders">No folders allowed yet.</p>
      ) : (
        <ul className="policy-list" aria-label="Allowed folders">
          {settings.allowedFolders.map((f) => (
            <li key={f}>
              <code>{f}</code>{" "}
              <button
                type="button"
                disabled={busy}
                onClick={() => void removeFolder(f)}
                aria-label={`Remove folder ${f}`}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      <form onSubmit={addFolder}>
        <label htmlFor="add-folder">Add folder (full path)</label>{" "}
        <input
          id="add-folder"
          value={folder}
          onChange={(e) => setFolder(e.target.value)}
          placeholder="e.g. C:\Users\you\Documents"
          spellCheck={false}
          autoComplete="off"
        />{" "}
        <button type="submit" disabled={busy || !folder.trim()}>
          Add folder
        </button>
      </form>

      <h3>Allowed apps</h3>
      <p className="hint">
        JARVIS can open only the apps you tick. Check the path or id under each
        name: that is what actually runs.
      </p>
      {appsError && <p role="alert">{appsError}</p>}
      {installed === null && !appsError && <p>Looking for installed apps…</p>}
      {installed && (
        <>
          <label htmlFor="app-filter">Filter apps</label>{" "}
          <input
            id="app-filter"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            autoComplete="off"
          />
          {installed.length === 0 ? (
            <p>No installed apps were found.</p>
          ) : (
            <ul className="policy-list app-list" aria-label="Installed apps">
              {shown.map((a) => (
                <li key={a.id}>
                  <label>
                    <input
                      type="checkbox"
                      checked={allowedIds.has(a.id)}
                      disabled={busy}
                      onChange={(e) => toggleApp(a, e.target.checked)}
                    />{" "}
                    {a.name}
                    <br />
                    <code className="app-id">{a.id}</code>
                  </label>
                </li>
              ))}
            </ul>
          )}
          {orphans.length > 0 && (
            <>
              <p>Allowed but no longer installed:</p>
              <ul
                className="policy-list"
                aria-label="Allowed apps not installed"
              >
                {orphans.map((a) => (
                  <li key={a.id}>
                    {a.name} <code className="app-id">{a.id}</code>{" "}
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => toggleApp(a, false)}
                    >
                      Remove
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}
        </>
      )}

      <h3>Asking before actions</h3>
      <label>
        <input
          type="checkbox"
          checked={settings.autoAllowMedium}
          disabled={busy}
          onChange={(e) =>
            void save(
              { autoAllowMedium: e.target.checked },
              e.target.checked
                ? "Medium-risk actions will run without asking."
                : "JARVIS will ask first.",
            )
          }
        />{" "}
        Let medium-risk actions (reading, moving or renaming a single file) run
        without asking
      </label>
      <p className="hint">
        Even with this on, JARVIS still asks after it has read a file or web
        page in this conversation, and always asks for high-risk actions such as
        moving a whole folder.
      </p>
    </>
  );
}
