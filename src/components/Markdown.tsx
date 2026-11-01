import ReactMarkdown, { type Components } from "react-markdown";

// Model output is untrusted (CLAUDE.md security rules):
// - raw HTML is dropped entirely (`skipHtml`), never rendered;
// - links are not clickable: a click would navigate the app's own webview, and
//   we have no opener plugin by design. The URL is shown as text instead;
// - images are not loaded (no remote fetches triggered by the model); the alt
//   text is shown instead.
const components: Components = {
  a: ({ children, href }) => (
    <span className="md-link">
      {children}
      {href && <span className="md-link-url"> ({href})</span>}
    </span>
  ),
  img: ({ alt }) => (
    <span className="md-img">[image: {alt || "no description"}]</span>
  ),
};

export function Markdown({ text }: { text: string }) {
  return (
    <ReactMarkdown skipHtml components={components}>
      {text}
    </ReactMarkdown>
  );
}
