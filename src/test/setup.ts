import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// The app opens panels as a pop-up by default. Most tests are about what is in a panel, so they run with
// the in-page placement (a region); the placement tests set it themselves.
try {
  localStorage.setItem("jarvis.panelSplit", "rail");
} catch {
  // No storage in this environment: the default applies.
}

// Vitest globals are off, so Testing Library cannot register this itself.
afterEach(cleanup);

// jsdom has no canvas: say "no WebGL" quietly instead of logging "Not
// implemented" for every render. The app then shows the 2D orb, as it would on
// a machine without WebGL.
HTMLCanvasElement.prototype.getContext = (() =>
  null) as typeof HTMLCanvasElement.prototype.getContext;
