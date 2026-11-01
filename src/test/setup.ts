import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Vitest globals are off, so Testing Library cannot register this itself.
afterEach(cleanup);

// jsdom has no canvas: say "no WebGL" quietly instead of logging "Not
// implemented" for every render. The app then shows the 2D orb, as it would on
// a machine without WebGL.
HTMLCanvasElement.prototype.getContext = (() =>
  null) as typeof HTMLCanvasElement.prototype.getContext;
