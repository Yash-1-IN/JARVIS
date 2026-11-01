# Security

JARVIS is early alpha software and has not been audited.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Use GitHub's private vulnerability reporting
on this repository (Security tab, "Report a vulnerability"). Include the version or commit, the steps to
reproduce, and what an attacker gains.

## What the project aims to guarantee

- The model proposes actions; a permission gateway in Rust decides.
- File access is limited to folders you allow, checked again right before the action runs.
- Approvals are tied to the exact arguments, single-use, and expire after 60 seconds.
- API keys live in the operating system keychain only, never in files, logs or the UI.
- Audio is never written to disk.

The known limits of these defences are listed in [`docs/threat-model.md`](docs/threat-model.md).
