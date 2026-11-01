import { useEffect, useState } from "react";

/** True while the window is hidden, so animation can stop (0 frames when hidden). */
export function useDocumentHidden(): boolean {
  const [hidden, setHidden] = useState(
    () => document.visibilityState === "hidden",
  );
  useEffect(() => {
    const on = () => setHidden(document.visibilityState === "hidden");
    document.addEventListener("visibilitychange", on);
    return () => document.removeEventListener("visibilitychange", on);
  }, []);
  return hidden;
}
