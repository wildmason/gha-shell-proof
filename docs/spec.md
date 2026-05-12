# gha-shell-proof 1.0 Spec

`gha-shell-proof` is a local compatibility oracle for GitHub Actions `run:`-step shell handling. It exists so offline runners can attach the planned shell invocation, script extension, prologue/epilogue, line-ending normalization, and argv to a receipt instead of reimplementing the runner's script handler in every executor.

## Goals

- Reproduce the shell-name → argument-format-string and shell-name → file-extension tables from `actions/runner` `ScriptHandlerHelpers.cs` exactly.
- Reproduce `FixUpScriptContents` for `cmd`, `pwsh`, and `powershell` exactly, including line-ending normalization for the runner OS.
- Resolve the `step → job defaults.run → workflow defaults.run → runner default` chain for both `shell` and `working-directory`.
- Classify each `(shell, runner-os)` pair as `exact`, `compatible`, `simulated`, or `unsupported`.
- Emit deterministic text, JSON, and Markdown receipts with stable check IDs.

## Non-Goals

- Executing step scripts. `gha-shell-proof` plans; it never runs.
- Evaluating `${{ ... }}` expressions. `gha-expression-proof` owns that boundary; the scanner detects unrendered expressions and either flags them (working-directory) or refuses (shell).
- Validating workflow YAML structure beyond `run:` steps, `runs-on:`, and `defaults.run`. `gha-workflow-proof` owns that boundary.
- Modeling workflow commands, `GITHUB_*` env files, or runner annotations. `gha-command-proof` owns that boundary.
- Probing the actual `bash` / `pwsh` / `python` binary on the host. A `probe` mode is reserved for v1.1.

## Commands

### `plan`

Produce a single plan from explicit inputs. Requires `--runner-os`. Optional `--shell`, `--working-directory`, `--workspace`, `--temp-dir`, `--script-path`, plus the four `--*defaults-run-*` flags for the resolution chain.

### `render`

Run `plan` and additionally materialize the wrapped script body to disk. Requires `--script <path>` (the user's body) and `--output-script <path>` (where to write the wrapped result). The output contains the runner's prologue/epilogue and per-OS line endings.

### `check-workflow`

Parse one or more workflow YAML files, walk every `run:` step, and emit one plan per step. The runner OS is inferred from `runs-on:` labels:

- `ubuntu-*` or `linux` → linux
- `macos-*` or `macOS-*` → macos
- `windows-*` → windows
- `${{ matrix.* }}` or unrecognized → emit a `workflow.runs-on.runner-os-unresolved` failure unless `--runner-os` overrides.

## Resolution chain

```
step.shell         → ShellSource::Step
job.defaults.run   → ShellSource::JobDefaultsRun
workflow.defaults.run → ShellSource::WorkflowDefaultsRun
runner default     → ShellSource::RunnerDefault   (bash on linux/macos, pwsh on windows)
```

The same chain applies to `working-directory`, with `WorkdirSource::Workspace` as the floor.

## Compatibility classification

| shell | linux | macos | windows |
| --- | --- | --- | --- |
| bash | exact | exact | compatible |
| sh | exact | exact | compatible |
| pwsh | compatible | compatible | exact |
| powershell | unsupported | unsupported | exact |
| cmd | unsupported | unsupported | exact |
| python | exact | exact | exact |
| custom `<cmd> {0}` | compatible | compatible | compatible |

A plan's `classification` field reflects the host-compat result for the resolved shell. Custom shells are `compatible` because the runner executes them faithfully but cannot guarantee semantics.

## Script materialization

For each plan the receipt records:

- `extension` — from the runner's `_extensions` table.
- `line_ending` — `crlf` for windows, `lf` for linux/macos. Mirrors the runner's `Replace("\r\n","\n").Replace("\n","\r\n")` on Windows and Unix `WriteAllText` defaults.
- `encoding` — `utf-8` on windows, `utf-8-no-bom` on linux/macos.
- `temp_filename_pattern` — `<temp_dir><sep><guid>.<ext>` shape.
- `script_path` — concrete script path (deterministic placeholder GUID `00000000-0000-0000-0000-000000000000` unless `--script-path` overrides).
- `prologue` — `["@echo off"]` for cmd, `["$ErrorActionPreference = 'stop'"]` for pwsh/powershell, `[]` otherwise.
- `epilogue` — `["if ((Test-Path -LiteralPath variable:\\LASTEXITCODE)) { exit $LASTEXITCODE }"]` for pwsh/powershell, `[]` otherwise.

## Invocation argv

`invocation.argv` is `args_format.replace("{0}", script_path)` then split with POSIX shell-words rules. For `cmd`, the substituted string is kept as a single element because Windows command-line parsing differs from POSIX.

## Receipt schema

Schema version: `1`. The full shape is documented in [README.md](../README.md). Stable check IDs live in [RULES.md](RULES.md).

## Determinism

Receipts are byte-stable for fixed inputs. The only non-deterministic field is `generated_at` (RFC 3339 timestamp). Tests that compare receipts should normalize that field.
