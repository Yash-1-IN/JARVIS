/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "mock" selects MockTransport in dev builds only. */
  readonly VITE_TRANSPORT?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
