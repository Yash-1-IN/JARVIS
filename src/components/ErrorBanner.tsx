import { ERROR_INFO } from "../state/errors";
import type { JarvisError } from "../types/generated/JarvisError";

interface Props {
  error: JarvisError;
  /** Present only when the failed turn can be resent. */
  onRetry?: () => void;
}

export function ErrorBanner({ error, onRetry }: Props) {
  const info = ERROR_INFO[error.code];
  return (
    <div role="alert" className="error-banner" data-code={error.code}>
      <strong>{info.title}.</strong> {error.message} <span>{info.hint}</span>
      {error.retryable && onRetry && (
        <button type="button" onClick={onRetry}>
          Retry
        </button>
      )}
    </div>
  );
}
