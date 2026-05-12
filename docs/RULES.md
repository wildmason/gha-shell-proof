# Shell Rules

`gha-shell-proof` check IDs are stable for downstream receipts. Every check is namespaced as `shell.*` (per-step) or `workflow.*` (per-workflow scan).

## Resolution Checks

- `shell.resolution.source` — records where the resolved shell came from (`step`, `job-defaults-run`, `workflow-defaults-run`, `runner-default`).
- `shell.workdir.source` — records where the resolved working directory came from.
- `shell.workdir.absolute` — records whether the working directory is absolute (per the **runner OS**'s rules) or workspace-relative.
- `shell.workdir.expression` — warning when `working-directory:` contains an unrendered `${{ ... }}` expression.

## Shell Recognition Checks

- `shell.builtin.recognized` — the resolved name is one of the runner's six built-in shells.
- `shell.custom.template` — the resolved shell is a custom `<command> [args]` template.
- `shell.custom.template.placeholder` — passed when a custom template includes the `{0}` script-path placeholder; warning when it doesn't.

## Script Plan Checks

- `shell.script.extension` — extension picked from the runner's `_extensions` table; warning when the extension is empty (custom shells without a known mapping).
- `shell.script.line-ending` — `crlf` on Windows, `lf` on Unix.
- `shell.script.encoding` — `utf-8` on Windows, `utf-8-no-bom` on Unix.
- `shell.script.prologue` — runner injects a prologue (`@echo off` for cmd, `$ErrorActionPreference = 'stop'` for pwsh/powershell).
- `shell.script.epilogue` — runner injects an epilogue (`exit $LASTEXITCODE` propagation for pwsh/powershell).
- `shell.script.rendered` — emitted by `render` when the wrapped script has been written to disk.
- `shell.script.line-ending.consistent` — emitted by `render` when the on-disk file's line endings match the planned line ending; warning otherwise.

## Invocation Checks

- `shell.invocation.placeholder` — `args_format` contains the `{0}` script-path placeholder.
- `shell.invocation.command` — records the executable to launch.
- `shell.invocation.argv` — records the resolved argv (post-`{0}` substitution and shell-words splitting; single-element for `cmd`).

## Compatibility Check

- `shell.host.compat` — the per-`(shell, runner-os)` classification. Status:
  - `passed` for `exact` and `compatible`
  - `warning` for `simulated`
  - `failed` for `unsupported`

  The `classification` field on the check carries the same value.

## Workflow Scan Checks

- `workflow.run-steps.found` — records how many `run:` steps the scanner planned for each workflow file; warning when zero.
- `workflow.runs-on.runner-os-unresolved` — failure when `runs-on:` cannot be mapped to a runner OS and `--runner-os` was not provided.
- `workflow.runs-on.expression` — warning when `runs-on:` contains an unrendered `${{ ... }}` expression but `--runner-os` is supplied.
- `workflow.step.plan` — failure when a step couldn't be planned (e.g. step `shell:` contained an unrendered expression).
