# Security

Please report vulnerabilities privately through
[GitHub's security advisories](https://github.com/vkfolio/tonefold/security/advisories/new),
not in a public issue. You should get a reply within a week.

Areas worth a close look:

- The local WebSocket between the app and the composer sidecar (`crates/tonefold-ipc`,
  `agent/src/index.ts`).
- The installer (`scripts/install.ps1`), which runs one elevated step when installing the plugins.
- Tools the composer can call (`agent/src/tools.ts`): they should only touch the session, never
  the file system or network beyond what is documented.

Only the latest release gets security fixes.
