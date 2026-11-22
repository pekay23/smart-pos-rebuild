# Contributing

This project is licensed under AGPL-3.0-or-later.

- Open an issue before large changes.
- Keep changes focused and tested.
- Run `npm run build`, `npm run lint`, and `pwsh -File tools/cargo-build.ps1 -Command check` before submitting.
- Do not commit secrets, local databases, or build artifacts.

## Local Rust builds

Use `pwsh -File tools/cargo-build.ps1 -Command build` (or `check`, `test`, `clippy`, or `run`) for local Rust builds. Cargo uses the single repository `target/` directory with incremental compilation enabled and level-1 debug info for local `dev` and `test` profiles. This keeps source line tables while reducing full debug metadata. The wrapper reserves a counter slot before each Cargo command and clears that generated directory and known local CMake output folders at 21.5 GiB or after 100 commands, including after an interrupted command. The size limit is primary and the command limit is a fallback. If a POS process is running from a managed build directory, cleanup is deferred and the wrapper blocks the next command until the app is closed. A clean target still causes a cold compile; incremental compilation helps only while the managed target is retained.

## Optional local code map

The Graphify CLI installed from `C:\Projects\graphify` can build a local, AST-based map of this repository:

```powershell
graphify update . --no-cluster
graphify query "checkout reservation allocation" --graph graphify-out/graph.json --budget 1200
```

Graphify writes generated files to the ignored `graphify-out/` directory. Keep the CLI and its source repository separate from the POS runtime and shipped app. Use the repository's CodeGraph index first for focused symbol lookup and call paths, as directed by `AGENTS.md`; Graphify is an optional broader map and query tool. SQL parsing is optional in Graphify and is not enabled in the default local installation.

