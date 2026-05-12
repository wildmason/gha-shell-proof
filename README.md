# gha-shell-proof

`gha-shell-proof` plans and validates GitHub Actions `run:`-step shell invocations with receipt-backed evidence.

It is the shell-handler boundary for offline CI work:

- use `gha-workflow-proof` to validate workflow structure;
- use `gha-shell-proof` to prove which shell, script extension, prologue/epilogue, line endings, and argv a given `run:` step will produce on a target runner OS;
- use `gha-command-proof` to validate the workflow-command and `GITHUB_*` env-file boundary that the rendered script writes to.

The shell selection, argument-format strings, file extensions, and script-content fixups are mirrored verbatim from `actions/runner` (`src/Runner.Worker/Handlers/ScriptHandlerHelpers.cs`).

## Install

```powershell
cargo install gha-shell-proof --locked
```

## Commands

### Plan a single step

```powershell
gha-shell-proof plan `
  --runner-os linux `
  --shell bash `
  --working-directory app `
  --workspace . `
  --format json
```

### Plan with the workflow / job `defaults.run` chain

```powershell
gha-shell-proof plan `
  --runner-os windows `
  --defaults-run-shell pwsh `
  --job-defaults-run-working-directory src `
  --format markdown
```

### Render a wrapped script to disk

```powershell
gha-shell-proof render `
  --runner-os windows `
  --shell pwsh `
  --script step.ps1 `
  --output-script rendered.ps1 `
  --format json
```

The rendered file contains the runner's `FixUpScriptContents` prologue (`$ErrorActionPreference = 'stop'`) and epilogue (`if ((Test-Path -LiteralPath variable:\LASTEXITCODE)) { exit $LASTEXITCODE }`) plus per-OS line-ending normalization (CRLF for Windows, LF for Linux/macOS). For `cmd`, an `@echo off` prologue is prepended.

### Scan a workflow for `run:` steps

```powershell
gha-shell-proof check-workflow `
  --workflow .github/workflows/ci.yml `
  --workspace . `
  --format markdown
```

The scanner walks every job, follows `runs-on:` to infer the runner OS (`ubuntu-*` → linux, `macos-*` → macos, `windows-*` → windows), and produces one plan per `run:` step. Pass `--runner-os` to override an unresolvable `runs-on` such as `${{ matrix.os }}`.

## What it models

Verbatim from the runner:

| shell | argument-format | extension | script fixup |
| --- | --- | --- | --- |
| `bash` | `--noprofile --norc -e -o pipefail {0}` | `.sh` | (none) |
| `sh` | `-e {0}` | `.sh` | (none) |
| `pwsh` | `-command ". '{0}'"` | `.ps1` | prologue `$ErrorActionPreference = 'stop'`; epilogue `if ((Test-Path -LiteralPath variable:\LASTEXITCODE)) { exit $LASTEXITCODE }` |
| `powershell` | `-command ". '{0}'"` | `.ps1` | same as `pwsh` |
| `cmd` | `/D /E:ON /V:OFF /S /C "CALL "{0}""` | `.cmd` | prologue `@echo off` |
| `python` | `{0}` | `.py` | (none) |
| custom | as written; first space splits `<command> <args>` | (none) | (none) |

Plus:

- the `step → job defaults.run → workflow defaults.run → runner default` resolution chain for both `shell` and `working-directory`;
- runner default selection: `bash` on linux/macOS, `pwsh` on windows (with `sh` / `powershell` listed as runner fallbacks);
- per-OS script line-ending normalization (CRLF on Windows, LF on Linux/macOS) and encoding (`utf-8`/`utf-8-no-bom`);
- per-`(shell, runner-os)` compatibility classification:

  | shell | linux | macos | windows |
  | --- | --- | --- | --- |
  | `bash` | exact | exact | compatible |
  | `sh` | exact | exact | compatible |
  | `pwsh` | compatible | compatible | exact |
  | `powershell` | unsupported | unsupported | exact |
  | `cmd` | unsupported | unsupported | exact |
  | `python` | exact | exact | exact |
  | custom `<cmd> {0}` | compatible | compatible | compatible |

- runner-OS-aware absolute-path detection for `working-directory:` (POSIX root `/...` for linux/macOS; drive-letter and UNC for windows) so plans computed on a Windows host classify a Linux runner correctly.

## Receipt shape

Receipts have a stable schema (`schema_version: 1`):

```jsonc
{
  "schema_version": 1,
  "tool": { "name": "gha-shell-proof", "version": "1.0.0" },
  "generated_at": "2026-05-12T22:34:00Z",
  "mode": "plan",
  "plans": [
    {
      "workflow": "...", "job": "...", "step_index": 1, "step_id": "...", "step_name": "...",
      "plan": {
        "runner_os": "linux",
        "shell": { "spec": { "kind": "builtin", "name": "bash" }, "source": "runner-default", "builtin": true,
                   "command": "bash", "args_format": "--noprofile --norc -e -o pipefail {0}", "extension": ".sh" },
        "working_directory": { "source": "workspace", "workspace": ".", "resolved": ".", "absolute": false },
        "script": { "extension": ".sh", "line_ending": "lf", "encoding": "utf-8-no-bom",
                    "temp_filename_pattern": "/home/runner/work/_temp/<guid>.sh",
                    "script_path": "/home/runner/work/_temp/00000000-0000-0000-0000-000000000000.sh",
                    "prologue": [], "epilogue": [] },
        "invocation": { "command": "bash", "args_format": "--noprofile --norc -e -o pipefail {0}",
                        "argv": ["--noprofile", "--norc", "-e", "-o", "pipefail",
                                 "/home/runner/work/_temp/00000000-0000-0000-0000-000000000000.sh"],
                        "working_directory": "." },
        "fail_fast": { "flags": ["-e", "-o", "pipefail"], "propagates_lastexitcode": false },
        "classification": "exact"
      },
      "checks": [ { "id": "shell.host.compat", "status": "passed",
                    "message": "shell `bash` is exact on runner-os `linux`",
                    "classification": "exact" } ],
      "summary": { "passed": 11, "warnings": 0, "failed": 0, "skipped": 0 }
    }
  ],
  "checks": [],
  "summary": { "passed": 11, "warnings": 0, "failed": 0, "skipped": 0 }
}
```

Stable check IDs are listed in [`docs/RULES.md`](docs/RULES.md). All check IDs are namespaced under `shell.*` (per-step) or `workflow.*` (per-workflow scan).

## Scope

`gha-shell-proof` is not a runner. It does **not**:

- execute step scripts;
- evaluate `${{ ... }}` expressions (use `gha-expression-proof`);
- parse workflow YAML beyond `run:` steps, `runs-on:`, and `defaults.run` (use `gha-workflow-proof` for full structural validation);
- model workflow commands or `GITHUB_*` env-file behavior (use `gha-command-proof`);
- discover or probe the actual `bash` / `pwsh` binary on the host (a `probe` mode is reserved for v1.1).

Every receipt is a description of what GitHub's runner *would* do for the given inputs, not what your local host *does*.

## References

- Open-source runner: <https://github.com/actions/runner>
- Script handler helpers: <https://github.com/actions/runner/blob/main/src/Runner.Worker/Handlers/ScriptHandlerHelpers.cs>
- Workflow `defaults.run` reference: <https://docs.github.com/en/actions/reference/workflow-syntax-for-github-actions#defaultsrun>
- Workflow `jobs.<job_id>.steps[*].shell` reference: <https://docs.github.com/en/actions/reference/workflow-syntax-for-github-actions#jobsjob_idstepsshell>
