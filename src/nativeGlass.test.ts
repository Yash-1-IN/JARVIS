import { describe, expect, it } from "vitest";
import css from "./nativeGlass.css?raw";

describe("nativeGlass.css (#166)", () => {
  it("only applies when the backend marked the page", () => {
    const clean = css.replace(/\/\*[\s\S]*?\*\//g, "");
    const selectors = [...clean.matchAll(/([^{}]+)\{/g)]
      .map((m) => m[1].trim())
      .filter((s) => !s.startsWith("@"));
    expect(selectors.length).toBeGreaterThan(0);
    for (const s of selectors) {
      for (const part of s.split(",")) {
        expect(part.trim()).toMatch(/^html\[data-native-glass\]/);
      }
    }
  });

  it("never touches the approval or memory cards", () => {
    const clean = css.replace(/\/\*[\s\S]*?\*\//g, "");
    expect(clean).not.toMatch(/approval|memory/);
  });
});
