import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ERROR_INFO } from "../state/errors";
import type { ErrorCode } from "../types/generated/ErrorCode";
import { ErrorBanner } from "./ErrorBanner";

describe("ErrorBanner", () => {
  it("has a title and hint for every ErrorCode", () => {
    for (const code of Object.keys(ERROR_INFO) as ErrorCode[]) {
      const { unmount } = render(
        <ErrorBanner
          error={{ code, message: "m", retryable: false, requestId: null }}
        />,
      );
      expect(screen.getByRole("alert")).toHaveTextContent(
        ERROR_INFO[code].title,
      );
      expect(screen.getByRole("alert")).toHaveTextContent(
        ERROR_INFO[code].hint,
      );
      unmount();
    }
  });

  it("offers Retry only when retryable and a handler is given", () => {
    const onRetry = vi.fn();
    const err = {
      code: "PROVIDER_RATE_LIMIT",
      message: "m",
      retryable: true,
      requestId: "r",
    } as const;
    const { rerender } = render(<ErrorBanner error={err} />);
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();

    rerender(
      <ErrorBanner error={{ ...err, retryable: false }} onRetry={onRetry} />,
    );
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();

    rerender(<ErrorBanner error={err} onRetry={onRetry} />);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });
});
