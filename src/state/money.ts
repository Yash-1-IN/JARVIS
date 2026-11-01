/** Contract money is integer micro-dollars (1e-6 USD). */
const MICRO = 1_000_000;

/** "$1.23"; amounts under a cent keep 4 decimals so small spend isn't shown as $0.00. */
export function formatUsd(micro: number): string {
  const usd = micro / MICRO;
  if (usd > 0 && usd < 0.01) return `$${usd.toFixed(4)}`;
  return `$${usd.toFixed(2)}`;
}

/**
 * Cap as typed in the form: empty means "no cap". Keeps every significant
 * decimal (down to micro-dollars) so a sub-cent cap is not shown as 0.00.
 */
export function capToInput(micro: number | null): string {
  if (micro === null) return "";
  const s = (micro / MICRO).toFixed(6).replace(/0+$/, "");
  const [whole, frac = ""] = s.split(".");
  return `${whole}.${frac.padEnd(2, "0")}`;
}

/**
 * Parses a dollar amount typed by the user, up to 6 decimals (micro-dollars,
 * the contract's unit). Returns null for "no cap", a micro-dollar integer, or
 * "invalid".
 */
export function parseCapInput(raw: string): number | null | "invalid" {
  const s = raw.trim().replace(/^\$/, "");
  if (s === "") return null;
  if (!/^\d+(\.\d{1,6})?$/.test(s)) return "invalid";
  const micro = Math.round(Number(s) * MICRO);
  return Number.isSafeInteger(micro) ? micro : "invalid";
}

/** Next reset in the user's local time, e.g. "5:30 AM" or "1 Dec, 5:30 AM". */
export function formatReset(ms: number, now = Date.now()): string {
  const d = new Date(ms);
  const time = d.toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
  const sameDay = new Date(now).toDateString() === d.toDateString();
  if (sameDay) return time;
  const day = d.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
  return `${day}, ${time}`;
}
