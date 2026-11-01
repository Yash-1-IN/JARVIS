import { useEffect, useState } from "react";

const QUERY = "(prefers-color-scheme: dark)";

/** True while the OS uses a dark colour scheme, live (the app follows it). */
export function useDarkScheme(): boolean {
  const [dark, setDark] = useState(
    () =>
      typeof window.matchMedia === "function" &&
      window.matchMedia(QUERY).matches,
  );
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia(QUERY);
    const on = () => setDark(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return dark;
}
