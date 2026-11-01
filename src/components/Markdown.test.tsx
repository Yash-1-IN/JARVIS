import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Markdown } from "./Markdown";

describe("Markdown (untrusted model output)", () => {
  it("renders basic markdown", () => {
    const { container } = render(
      <Markdown text={"**bold** and `code`\n\n- one\n- two"} />,
    );
    expect(container.querySelector("strong")).toHaveTextContent("bold");
    expect(container.querySelector("code")).toHaveTextContent("code");
    expect(container.querySelectorAll("li")).toHaveLength(2);
  });

  it("drops raw HTML instead of rendering or escaping it", () => {
    const { container } = render(
      <Markdown
        text={
          'hi <img src=x onerror="alert(1)"> <script>alert(2)</script> <b>x</b>'
        }
      />,
    );
    expect(container.querySelector("img, script, b")).toBeNull();
    // Tags and attributes are gone; text between tags stays as inert text.
    expect(container.innerHTML).not.toMatch(/onerror|<script|<img|<b>/);
  });

  it("never renders clickable links, including javascript: URLs", () => {
    const { container } = render(
      <Markdown
        text={
          "[docs](https://example.com) [x](javascript:alert(1)) <https://auto.link>"
        }
      />,
    );
    expect(container.querySelector("a")).toBeNull();
    expect(screen.getByText(/\(https:\/\/example\.com\)/)).toBeInTheDocument();
    expect(container.innerHTML).not.toContain("javascript:");
  });

  it("does not load images; shows alt text", () => {
    const { container } = render(
      <Markdown text={"![a cat](https://tracker.example/p.png)"} />,
    );
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("[image: a cat]")).toBeInTheDocument();
  });
});
