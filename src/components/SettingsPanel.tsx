import { useEffect, useState, type FormEvent } from "react";
import { describeError } from "../state/errors";
import { PolicySettings } from "./PolicySettings";
import {
  capToInput,
  formatReset,
  formatUsd,
  parseCapInput,
} from "../state/money";
import type { Transport } from "../transport";
import type { ApiKeyStatus } from "../types/generated/ApiKeyStatus";
import type { Settings } from "../types/generated/Settings";
import type { SettingsPatch } from "../types/generated/SettingsPatch";
import type { UsageSummary } from "../types/generated/UsageSummary";

function KeyStatus({ status }: { status: ApiKeyStatus }) {
  if (!status.isSet) return <p data-testid="key-status">No API key set.</p>;
  return (
    <>
      <p data-testid="key-status">
        API key set{status.hint && <> (ending {status.hint})</>}
        {status.storage === "keychain" && ", stored in your system keychain."}
      </p>
      {status.storage === "session_only" && (
        <p role="alert" className="warning">
          Your system has no secure credential store, so the key is kept only
          until JARVIS quits. You'll need to enter it again next time.
        </p>
      )}
    </>
  );
}

function UsageView({ usage }: { usage: UsageSummary }) {
  const line = (spent: number, cap: number | null) =>
    cap === null
      ? `${formatUsd(spent)} (no cap)`
      : `${formatUsd(spent)} of ${formatUsd(cap)}`;
  return (
    <dl className="usage" data-testid="usage">
      <dt>Today</dt>
      <dd>
        {line(usage.todayMicroUsd, usage.dailyCapMicroUsd)} · resets{" "}
        {formatReset(usage.dayResetsAt)}
      </dd>
      <dt>This month</dt>
      <dd>
        {line(usage.monthMicroUsd, usage.monthlyCapMicroUsd)} · resets{" "}
        {formatReset(usage.monthResetsAt)}
      </dd>
      <dt>Tokens today</dt>
      <dd>
        {usage.todayInputTokens.toLocaleString()} in ·{" "}
        {usage.todayOutputTokens.toLocaleString()} out
      </dd>
    </dl>
  );
}

/** Settings: API key, model, spend caps, usage (contract #11). */
export function SettingsPanel({ transport }: { transport: Transport }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [usage, setUsage] = useState<UsageSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [daily, setDaily] = useState("");
  const [monthly, setMonthly] = useState("");

  useEffect(() => {
    let live = true;
    const off = transport.onUsage((u) => live && setUsage(u));
    Promise.all([transport.getSettings(), transport.getUsage()])
      .then(([s, u]) => {
        if (!live) return;
        setSettings(s);
        setUsage(u);
        setDaily(capToInput(s.dailyCapMicroUsd));
        setMonthly(capToInput(s.monthlyCapMicroUsd));
      })
      .catch((e: unknown) => live && setError(describeError(e)));
    return () => {
      live = false;
      off();
    };
  }, [transport]);

  /** Runs a backend action with shared busy/notice/error handling; true if it succeeded. */
  const run = async (
    action: () => Promise<void>,
    done: string,
  ): Promise<boolean> => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await action();
      setNotice(done);
      return true;
    } catch (e) {
      setError(describeError(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const saveKey = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    // Read from the uncontrolled field and reset the form at once, so the key
    // never sits in React state (CLAUDE.md security rules).
    const form = e.currentTarget;
    const key = String(new FormData(form).get("apiKey") ?? "");
    form.reset();
    if (!key.trim()) return;
    void run(async () => {
      const apiKey = await transport.setApiKey(key);
      setSettings((s) => s && { ...s, apiKey });
    }, "API key saved.");
  };

  const clearKey = () =>
    run(async () => {
      const apiKey = await transport.clearApiKey();
      setSettings((s) => s && { ...s, apiKey });
    }, "API key removed.");

  const savePatch = (patch: SettingsPatch, done: string) =>
    run(async () => setSettings(await transport.updateSettings(patch)), done);

  const saveModel = (model: string) =>
    run(
      async () => setSettings(await transport.updateSettings({ model })),
      "Model updated.",
    );

  const saveCaps = (e: FormEvent) => {
    e.preventDefault();
    if (!settings) return;
    const d = parseCapInput(daily);
    const m = parseCapInput(monthly);
    if (d === "invalid" || m === "invalid") {
      setNotice(null);
      setError(
        "Enter caps as dollar amounts like 2, 2.50 or 0.0018 (up to 6 decimals), or leave blank for no cap.",
      );
      return;
    }
    // Send only what changed; omitted fields stay as they are.
    const patch: SettingsPatch = {};
    if (d !== settings.dailyCapMicroUsd) patch.dailyCapMicroUsd = d;
    if (m !== settings.monthlyCapMicroUsd) patch.monthlyCapMicroUsd = m;
    if (Object.keys(patch).length === 0) return;
    void run(async () => {
      const s = await transport.updateSettings(patch);
      setSettings(s);
      setDaily(capToInput(s.dailyCapMicroUsd));
      setMonthly(capToInput(s.monthlyCapMicroUsd));
    }, "Spend caps saved.");
  };

  return (
    <section aria-label="Settings" className="settings">
      <h2>Settings</h2>
      {error && <p role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}
      {!settings ? (
        !error && <p>Loading settings…</p>
      ) : (
        <>
          <h3>Anthropic API key</h3>
          <KeyStatus status={settings.apiKey} />
          <form onSubmit={saveKey} autoComplete="off">
            <label htmlFor="api-key">
              {settings.apiKey.isSet ? "Replace key" : "API key"}
            </label>{" "}
            <input
              id="api-key"
              name="apiKey"
              type="password"
              autoComplete="off"
              spellCheck={false}
            />{" "}
            <button type="submit" disabled={busy}>
              Save key
            </button>{" "}
            {settings.apiKey.isSet && (
              <button type="button" disabled={busy} onClick={clearKey}>
                Remove key
              </button>
            )}
          </form>
          <p className="hint">
            The key is checked with the provider, then kept in your system
            keychain. JARVIS never shows it again.
          </p>
          <h3>Model</h3>
          <label htmlFor="model">Model</label>{" "}
          <select
            id="model"
            value={settings.model}
            disabled={busy}
            onChange={(e) => void saveModel(e.target.value)}
          >
            {settings.availableModels.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          <h3>Spend caps</h3>
          <form onSubmit={saveCaps}>
            <label htmlFor="cap-daily">Daily cap (USD)</label>{" "}
            <input
              id="cap-daily"
              inputMode="decimal"
              placeholder="No cap"
              value={daily}
              onChange={(e) => setDaily(e.target.value)}
            />{" "}
            <label htmlFor="cap-monthly">Monthly cap (USD)</label>{" "}
            <input
              id="cap-monthly"
              inputMode="decimal"
              placeholder="No cap"
              value={monthly}
              onChange={(e) => setMonthly(e.target.value)}
            />{" "}
            <button type="submit" disabled={busy}>
              Save caps
            </button>
          </form>
          <p className="hint">
            A cap stops new cloud requests once reached; one long reply can go
            slightly over. Local commands keep working.
          </p>
          <PolicySettings
            transport={transport}
            settings={settings}
            save={savePatch}
            busy={busy}
          />
          <h3>Usage</h3>
          {usage && <UsageView usage={usage} />}
        </>
      )}
    </section>
  );
}
