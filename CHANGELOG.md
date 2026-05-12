# Changelog

## 1.0.0 - 2026-05-12

- Initial public release.
- Added `plan`, `render`, and `check-workflow` commands.
- Modeled the runner script-handler boundary verbatim from `actions/runner` (`ScriptHandlerHelpers.cs`): `bash`, `sh`, `pwsh`, `powershell`, `cmd`, `python`, and custom `<command> {0}` shells, including extensions, argument-format strings, `FixUpScriptContents` prologue/epilogue, and per-OS line-ending normalization.
- Added the resolution chain: step `shell` / `working-directory` → job `defaults.run` → workflow `defaults.run` → runner default.
- Added per-`(shell, runner-os)` compatibility classification (`exact` / `compatible` / `simulated` / `unsupported`) with stable check IDs.
- Added text, JSON, and Markdown receipts.
- Added composite GitHub Action wrapper, examples, docs, and CI.
