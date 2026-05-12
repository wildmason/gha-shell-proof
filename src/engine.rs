//! Core shell-resolution and plan-construction engine.
//!
//! Templates and script wrapping mirror the open-source `actions/runner`
//! implementation in `src/Runner.Worker/Handlers/ScriptHandlerHelpers.cs`.

use crate::model::{
    BuiltinShell, Check, CheckStatus, Classification, Encoding, FailFast, Invocation, LineEnding,
    Plan, ResolvedShell, ResolvedWorkdir, RunnerOs, ScriptPlan, ShellSource, ShellSpec, Summary,
    WorkdirSource,
};
use anyhow::{Context, Result, bail, ensure};
use camino::{Utf8Path, Utf8PathBuf};

/// Default working-directory placeholder identifier when no explicit script
/// path is provided. The runner uses a freshly-generated GUID; we pin a stable
/// placeholder so receipts are byte-deterministic.
pub const SCRIPT_PLACEHOLDER_ID: &str = "00000000-0000-0000-0000-000000000000";

/// Shell -> argument format string. Verbatim from
/// `actions/runner` `ScriptHandlerHelpers._defaultArguments`.
pub fn builtin_args_format(shell: BuiltinShell) -> &'static str {
    match shell {
        BuiltinShell::Cmd => "/D /E:ON /V:OFF /S /C \"CALL \"{0}\"\"",
        BuiltinShell::Pwsh => "-command \". '{0}'\"",
        BuiltinShell::Powershell => "-command \". '{0}'\"",
        BuiltinShell::Bash => "--noprofile --norc -e -o pipefail {0}",
        BuiltinShell::Sh => "-e {0}",
        BuiltinShell::Python => "{0}",
    }
}

/// Shell -> script file extension. Verbatim from
/// `actions/runner` `ScriptHandlerHelpers._extensions`.
pub fn builtin_extension(shell: BuiltinShell) -> &'static str {
    match shell {
        BuiltinShell::Cmd => ".cmd",
        BuiltinShell::Pwsh | BuiltinShell::Powershell => ".ps1",
        BuiltinShell::Bash | BuiltinShell::Sh => ".sh",
        BuiltinShell::Python => ".py",
    }
}

/// Default shell for a runner OS. Mirrors `ScriptHandler` selection logic.
pub fn default_shell(os: RunnerOs) -> BuiltinShell {
    match os {
        RunnerOs::Linux | RunnerOs::Macos => BuiltinShell::Bash,
        RunnerOs::Windows => BuiltinShell::Pwsh,
    }
}

/// Fallback shell when the primary default is not present on the runner.
pub fn default_shell_fallback(os: RunnerOs) -> BuiltinShell {
    match os {
        RunnerOs::Linux | RunnerOs::Macos => BuiltinShell::Sh,
        RunnerOs::Windows => BuiltinShell::Powershell,
    }
}

/// The `actions/runner` `FixUpScriptContents` prepended/appended lines.
fn builtin_prologue(shell: BuiltinShell) -> Vec<String> {
    match shell {
        BuiltinShell::Cmd => vec!["@echo off".to_owned()],
        BuiltinShell::Pwsh | BuiltinShell::Powershell => {
            vec!["$ErrorActionPreference = 'stop'".to_owned()]
        }
        _ => Vec::new(),
    }
}

fn builtin_epilogue(shell: BuiltinShell) -> Vec<String> {
    match shell {
        BuiltinShell::Pwsh | BuiltinShell::Powershell => vec![
            r"if ((Test-Path -LiteralPath variable:\LASTEXITCODE)) { exit $LASTEXITCODE }"
                .to_owned(),
        ],
        _ => Vec::new(),
    }
}

/// Per-OS script line ending. Mirrors `ScriptHandler` normalization.
pub fn line_ending_for(os: RunnerOs) -> LineEnding {
    match os {
        RunnerOs::Windows => LineEnding::Crlf,
        RunnerOs::Linux | RunnerOs::Macos => LineEnding::Lf,
    }
}

/// Per-OS script encoding. Unix runners explicitly write UTF-8 without BOM.
pub fn encoding_for(os: RunnerOs) -> Encoding {
    match os {
        RunnerOs::Windows => Encoding::Utf8,
        RunnerOs::Linux | RunnerOs::Macos => Encoding::Utf8NoBom,
    }
}

/// Default temp directory used in receipt placeholders for each OS. The runner
/// uses `RUNNER_TEMP`; we use a deterministic literal so receipts are stable.
pub fn default_temp_dir(os: RunnerOs) -> &'static str {
    match os {
        RunnerOs::Windows => "D:\\a\\_temp",
        RunnerOs::Linux | RunnerOs::Macos => "/home/runner/work/_temp",
    }
}

/// Mirror of `ScriptHandlerHelpers.ParseShellOptionString`: split a custom
/// shell value on the first space.
pub fn parse_custom_shell(option: &str) -> Result<(String, String)> {
    let trimmed = option.trim();
    ensure!(!trimmed.is_empty(), "shell option is empty");
    if let Some((command, args)) = trimmed.split_once(' ') {
        Ok((command.to_owned(), args.trim_start().to_owned()))
    } else {
        Ok((trimmed.to_owned(), String::new()))
    }
}

#[derive(Debug, Clone, Default)]
pub struct StepInputs {
    pub shell: Option<String>,
    pub working_directory: Option<String>,
    pub job_defaults_run_shell: Option<String>,
    pub job_defaults_run_working_directory: Option<String>,
    pub workflow_defaults_run_shell: Option<String>,
    pub workflow_defaults_run_working_directory: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlanInputs {
    pub runner_os: RunnerOs,
    pub workspace: Utf8PathBuf,
    pub temp_dir: Option<Utf8PathBuf>,
    pub script_path: Option<Utf8PathBuf>,
    pub step: StepInputs,
}

/// Build a plan + audit checks for a single `run:` step.
pub fn make_plan(inputs: &PlanInputs) -> Result<(Plan, Vec<Check>)> {
    let mut checks = Vec::new();

    let shell = resolve_shell(inputs.runner_os, &inputs.step, &mut checks)?;
    let workdir = resolve_workdir(
        &inputs.step,
        &inputs.workspace,
        inputs.runner_os,
        &mut checks,
    );

    let script = build_script_plan(&shell, inputs)?;
    let invocation = build_invocation(&shell, &script, &workdir)?;
    let fail_fast = compute_fail_fast(&shell);
    let classification = classify(&shell, inputs.runner_os, &mut checks);

    audit_plan_extras(&shell, &script, &invocation, &workdir, &mut checks);

    let plan = Plan {
        runner_os: inputs.runner_os,
        shell,
        working_directory: workdir,
        script,
        invocation,
        fail_fast,
        classification,
    };
    Ok((plan, checks))
}

/// Compute the chained shell resolution: step > job defaults > workflow
/// defaults > runner default.
pub fn resolve_shell(
    runner_os: RunnerOs,
    step: &StepInputs,
    checks: &mut Vec<Check>,
) -> Result<ResolvedShell> {
    let (raw, source) = if let Some(value) = non_empty(&step.shell) {
        (value, ShellSource::Step)
    } else if let Some(value) = non_empty(&step.job_defaults_run_shell) {
        (value, ShellSource::JobDefaultsRun)
    } else if let Some(value) = non_empty(&step.workflow_defaults_run_shell) {
        (value, ShellSource::WorkflowDefaultsRun)
    } else {
        let builtin = default_shell(runner_os);
        let resolved = ResolvedShell {
            spec: ShellSpec::Builtin {
                name: builtin.as_str().to_owned(),
            },
            source: ShellSource::RunnerDefault,
            builtin: true,
            command: builtin.as_str().to_owned(),
            args_format: builtin_args_format(builtin).to_owned(),
            extension: builtin_extension(builtin).to_owned(),
        };
        checks.push(
            Check::passed(
                "shell.resolution.source",
                format!(
                    "shell defaulted to runner default `{}` for runner-os `{}`",
                    builtin.as_str(),
                    runner_os.as_str()
                ),
            )
            .with_detail(format!("source={}", ShellSource::RunnerDefault.as_str())),
        );
        return Ok(resolved);
    };

    if has_expression_marker(raw) {
        bail!(
            "shell value `{raw}` contains an unrendered `${{ ... }}` expression; \
             render expressions before invoking gha-shell-proof"
        );
    }

    let resolved = if let Some(builtin) = BuiltinShell::from_name(raw) {
        ResolvedShell {
            spec: ShellSpec::Builtin {
                name: builtin.as_str().to_owned(),
            },
            source,
            builtin: true,
            command: builtin.as_str().to_owned(),
            args_format: builtin_args_format(builtin).to_owned(),
            extension: builtin_extension(builtin).to_owned(),
        }
    } else {
        let (command, args) = parse_custom_shell(raw).context("parsing custom shell template")?;
        let template = if args.is_empty() {
            command.clone()
        } else {
            format!("{command} {args}")
        };
        ResolvedShell {
            spec: ShellSpec::Custom {
                command: command.clone(),
                args: args.clone(),
                template,
            },
            source,
            builtin: false,
            command,
            args_format: args,
            extension: String::new(),
        }
    };

    checks.push(
        Check::passed(
            "shell.resolution.source",
            format!(
                "shell `{}` resolved from {}",
                resolved.spec.name(),
                source.as_str()
            ),
        )
        .with_detail(format!("source={}", source.as_str())),
    );
    Ok(resolved)
}

/// Resolve the working directory through the step → job-defaults →
/// workflow-defaults → workspace chain. Absoluteness honors the runner OS
/// rather than the host the planner is running on.
pub fn resolve_workdir(
    step: &StepInputs,
    workspace: &Utf8Path,
    runner_os: RunnerOs,
    checks: &mut Vec<Check>,
) -> ResolvedWorkdir {
    let (requested, source) = if let Some(value) = non_empty(&step.working_directory) {
        (Some(value.to_owned()), WorkdirSource::Step)
    } else if let Some(value) = non_empty(&step.job_defaults_run_working_directory) {
        (Some(value.to_owned()), WorkdirSource::JobDefaultsRun)
    } else if let Some(value) = non_empty(&step.workflow_defaults_run_working_directory) {
        (Some(value.to_owned()), WorkdirSource::WorkflowDefaultsRun)
    } else {
        (None, WorkdirSource::Workspace)
    };

    let workspace_str = workspace.as_str().to_owned();
    let (resolved, absolute) = match &requested {
        None => (
            workspace_str.clone(),
            is_absolute_for(&workspace_str, runner_os),
        ),
        Some(value) => {
            if is_absolute_for(value, runner_os) {
                (value.clone(), true)
            } else {
                let joined = join_for(workspace, value, runner_os);
                (joined, false)
            }
        }
    };

    if let Some(value) = &requested
        && has_expression_marker(value)
    {
        checks.push(
            Check::warning(
                "shell.workdir.expression",
                "working-directory contained an unrendered `${{ ... }}` expression; \
                 the runner would evaluate this before launching the script",
            )
            .with_detail(format!("value={value}")),
        );
    }

    checks.push(
        Check::passed(
            "shell.workdir.source",
            format!("working-directory resolved from {}", source.as_str()),
        )
        .with_detail(format!("source={}", source.as_str())),
    );
    if absolute {
        checks.push(Check::passed(
            "shell.workdir.absolute",
            "working-directory is absolute",
        ));
    } else {
        checks.push(Check::passed(
            "shell.workdir.absolute",
            "working-directory is workspace-relative",
        ));
    }

    ResolvedWorkdir {
        source,
        requested,
        workspace: workspace_str,
        resolved,
        absolute,
    }
}

/// Build the script plan: extension, line ending, encoding, prologue/epilogue.
pub fn build_script_plan(shell: &ResolvedShell, inputs: &PlanInputs) -> Result<ScriptPlan> {
    let extension = shell.extension.clone();
    let line_ending = line_ending_for(inputs.runner_os);
    let encoding = encoding_for(inputs.runner_os);

    let temp_dir = inputs
        .temp_dir
        .clone()
        .unwrap_or_else(|| Utf8PathBuf::from(default_temp_dir(inputs.runner_os)));

    let temp_filename_pattern = format!(
        "{}{}<guid>{}",
        temp_dir.as_str(),
        path_separator(inputs.runner_os),
        if extension.is_empty() {
            String::new()
        } else {
            extension.clone()
        }
    );

    let script_path = if let Some(path) = &inputs.script_path {
        path.as_str().to_owned()
    } else {
        format!(
            "{}{}{}{}",
            temp_dir.as_str(),
            path_separator(inputs.runner_os),
            SCRIPT_PLACEHOLDER_ID,
            extension
        )
    };

    let (prologue, epilogue) = match &shell.spec {
        ShellSpec::Builtin { name } => {
            let builtin = BuiltinShell::from_name(name)
                .with_context(|| format!("unrecognised builtin shell `{name}`"))?;
            (builtin_prologue(builtin), builtin_epilogue(builtin))
        }
        ShellSpec::Custom { .. } => (Vec::new(), Vec::new()),
    };

    Ok(ScriptPlan {
        extension,
        line_ending,
        encoding,
        temp_filename_pattern,
        script_path,
        prologue,
        epilogue,
    })
}

/// Resolve the runner's `string.Format(argFormat, scriptPath)` substitution and
/// produce a best-effort argv split.
pub fn build_invocation(
    shell: &ResolvedShell,
    script: &ScriptPlan,
    workdir: &ResolvedWorkdir,
) -> Result<Invocation> {
    let substituted = shell.args_format.replace("{0}", &script.script_path);

    let argv = if matches!(&shell.spec, ShellSpec::Builtin { name } if name == "cmd") {
        vec![substituted.clone()]
    } else if substituted.is_empty() {
        Vec::new()
    } else {
        match shell_words::split(&substituted) {
            Ok(parts) => parts,
            Err(_) => vec![substituted.clone()],
        }
    };

    Ok(Invocation {
        command: shell.command.clone(),
        args_format: shell.args_format.clone(),
        argv,
        working_directory: workdir.resolved.clone(),
    })
}

/// Per-shell fail-fast metadata, mirrored from the runner's templates.
pub fn compute_fail_fast(shell: &ResolvedShell) -> FailFast {
    match &shell.spec {
        ShellSpec::Builtin { name } => match name.as_str() {
            "bash" => FailFast {
                flags: vec!["-e".into(), "-o".into(), "pipefail".into()],
                error_action_preference: None,
                propagates_lastexitcode: false,
            },
            "sh" => FailFast {
                flags: vec!["-e".into()],
                error_action_preference: None,
                propagates_lastexitcode: false,
            },
            "pwsh" | "powershell" => FailFast {
                flags: Vec::new(),
                error_action_preference: Some("stop".into()),
                propagates_lastexitcode: true,
            },
            "cmd" | "python" => FailFast::default(),
            _ => FailFast::default(),
        },
        ShellSpec::Custom { .. } => FailFast::default(),
    }
}

/// Per-(shell, OS) compatibility classification.
pub fn classify(
    shell: &ResolvedShell,
    runner_os: RunnerOs,
    checks: &mut Vec<Check>,
) -> Classification {
    let host_compat = match (&shell.spec, runner_os) {
        (ShellSpec::Builtin { name }, os) => match (name.as_str(), os) {
            ("bash", RunnerOs::Linux | RunnerOs::Macos) => Classification::Exact,
            ("bash", RunnerOs::Windows) => Classification::Compatible,
            ("sh", RunnerOs::Linux | RunnerOs::Macos) => Classification::Exact,
            ("sh", RunnerOs::Windows) => Classification::Compatible,
            ("pwsh", RunnerOs::Windows) => Classification::Exact,
            ("pwsh", RunnerOs::Linux | RunnerOs::Macos) => Classification::Compatible,
            ("powershell", RunnerOs::Windows) => Classification::Exact,
            ("powershell", _) => Classification::Unsupported,
            ("cmd", RunnerOs::Windows) => Classification::Exact,
            ("cmd", _) => Classification::Unsupported,
            ("python", _) => Classification::Exact,
            _ => Classification::Unsupported,
        },
        (ShellSpec::Custom { .. }, _) => Classification::Compatible,
    };

    let id = "shell.host.compat";
    let message = format!(
        "shell `{}` is {} on runner-os `{}`",
        shell.spec.name(),
        host_compat.as_str(),
        runner_os.as_str()
    );
    let check = match host_compat {
        Classification::Exact | Classification::Compatible => {
            Check::passed(id, message).with_classification(host_compat)
        }
        Classification::Simulated => Check::warning(id, message).with_classification(host_compat),
        Classification::Unsupported => Check::failed(id, message).with_classification(host_compat),
    };
    checks.push(check);

    host_compat
}

/// Emit the secondary plan-level checks (extension, prologue, epilogue,
/// invocation, custom-template placeholder, fail-fast).
pub fn audit_plan_extras(
    shell: &ResolvedShell,
    script: &ScriptPlan,
    invocation: &Invocation,
    workdir: &ResolvedWorkdir,
    checks: &mut Vec<Check>,
) {
    let _ = workdir;

    match &shell.spec {
        ShellSpec::Builtin { name } => {
            checks.push(
                Check::passed(
                    "shell.builtin.recognized",
                    format!("`{name}` is a built-in runner shell"),
                )
                .with_detail(format!("name={name}")),
            );
        }
        ShellSpec::Custom { template, args, .. } => {
            checks.push(
                Check::passed(
                    "shell.custom.template",
                    format!("custom shell template `{template}`"),
                )
                .with_detail(format!("template={template}")),
            );
            if args.contains("{0}") {
                checks.push(Check::passed(
                    "shell.custom.template.placeholder",
                    "custom shell template includes `{0}` script-path placeholder",
                ));
            } else {
                checks.push(Check::warning(
                    "shell.custom.template.placeholder",
                    "custom shell template is missing the `{0}` script-path placeholder; \
                     the runner will append nothing and execute the command alone",
                ));
            }
        }
    }

    if script.extension.is_empty() {
        checks.push(Check::warning(
            "shell.script.extension",
            "no script file extension; the runner writes a temp file with no suffix",
        ));
    } else {
        checks.push(
            Check::passed(
                "shell.script.extension",
                format!("script extension `{}` selected", script.extension),
            )
            .with_detail(format!("extension={}", script.extension)),
        );
    }

    checks.push(
        Check::passed(
            "shell.script.line-ending",
            format!(
                "script line ending normalized to `{}`",
                script.line_ending.as_str()
            ),
        )
        .with_detail(format!("line-ending={}", script.line_ending.as_str())),
    );
    checks.push(
        Check::passed(
            "shell.script.encoding",
            format!("script encoded as `{}`", script.encoding.as_str()),
        )
        .with_detail(format!("encoding={}", script.encoding.as_str())),
    );

    if !script.prologue.is_empty() {
        checks.push(
            Check::passed(
                "shell.script.prologue",
                format!("runner injects {} prologue line(s)", script.prologue.len()),
            )
            .with_detail(script.prologue.join("\\n")),
        );
    }
    if !script.epilogue.is_empty() {
        checks.push(
            Check::passed(
                "shell.script.epilogue",
                format!("runner injects {} epilogue line(s)", script.epilogue.len()),
            )
            .with_detail(script.epilogue.join("\\n")),
        );
    }

    if invocation.args_format.contains("{0}") || invocation.args_format.is_empty() {
        checks.push(Check::passed(
            "shell.invocation.placeholder",
            "shell argument format includes `{0}` script-path placeholder",
        ));
    } else {
        checks.push(Check::warning(
            "shell.invocation.placeholder",
            "shell argument format does not contain `{0}`; runner will execute \
             the shell with the literal arg string",
        ));
    }

    checks.push(
        Check::passed(
            "shell.invocation.command",
            format!("invocation command `{}`", invocation.command),
        )
        .with_detail(format!("command={}", invocation.command)),
    );

    let argv_render = invocation
        .argv
        .iter()
        .map(|s| format!("`{s}`"))
        .collect::<Vec<_>>()
        .join(" ");
    checks.push(Check::passed(
        "shell.invocation.argv",
        format!("invocation argv: {argv_render}"),
    ));
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn has_expression_marker(value: &str) -> bool {
    value.contains("${{")
}

fn path_separator(os: RunnerOs) -> &'static str {
    match os {
        RunnerOs::Windows => "\\",
        RunnerOs::Linux | RunnerOs::Macos => "/",
    }
}

/// Runner-OS-aware absolute-path test.
///
/// `Utf8Path::is_absolute` uses the host platform's rules, so a Linux-runner
/// plan computed on a Windows host would mis-classify `/srv/build`. This
/// function honors the runner OS instead.
pub fn is_absolute_for(path: &str, runner_os: RunnerOs) -> bool {
    match runner_os {
        RunnerOs::Linux | RunnerOs::Macos => path.starts_with('/'),
        RunnerOs::Windows => is_absolute_windows(path),
    }
}

fn is_absolute_windows(path: &str) -> bool {
    // UNC: \\server\share  or  //server/share
    if path.starts_with("\\\\") || path.starts_with("//") {
        return true;
    }
    // Drive root: C:\... or C:/...
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    false
}

/// Runner-OS-aware path join used when `working-directory:` is relative.
fn join_for(workspace: &Utf8Path, relative: &str, runner_os: RunnerOs) -> String {
    let sep = path_separator(runner_os);
    let workspace_str = workspace.as_str();
    let trimmed_workspace = workspace_str.trim_end_matches(['/', '\\']);
    let trimmed_relative = relative.trim_start_matches(['/', '\\']);
    if trimmed_workspace.is_empty() {
        trimmed_relative.to_owned()
    } else {
        format!("{trimmed_workspace}{sep}{trimmed_relative}")
    }
}

/// Apply the runner's per-OS line-ending normalization to a script body.
pub fn normalize_line_endings(body: &str, line_ending: LineEnding) -> String {
    let lf_only: String = body.replace("\r\n", "\n");
    match line_ending {
        LineEnding::Lf => lf_only,
        LineEnding::Crlf => lf_only.replace('\n', "\r\n"),
    }
}

/// Apply prologue/epilogue, replicating
/// `actions/runner` `ScriptHandlerHelpers.FixUpScriptContents` and
/// per-OS line-ending normalization.
pub fn wrap_script(plan: &Plan, body: &str) -> String {
    let mut wrapped = String::new();
    let eol = plan.script.line_ending.literal();

    for line in &plan.script.prologue {
        wrapped.push_str(line);
        wrapped.push_str(eol);
    }

    let body_normalized = normalize_line_endings(body, plan.script.line_ending);
    wrapped.push_str(&body_normalized);

    if !plan.script.epilogue.is_empty() {
        if !wrapped.ends_with(eol) {
            wrapped.push_str(eol);
        }
        for line in &plan.script.epilogue {
            wrapped.push_str(line);
            wrapped.push_str(eol);
        }
    }

    wrapped
}

/// Roll the per-plan check list into a [`Summary`] for receipt totals.
pub fn summarize(checks: &[Check]) -> Summary {
    let mut summary = Summary::default();
    for check in checks {
        summary.record(check.status);
    }
    summary
}

/// Convenience: every plan-level check status that should be considered a
/// failure for `--strict` purposes.
pub fn has_blocking_failure(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.status == CheckStatus::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> Utf8PathBuf {
        Utf8PathBuf::from("/work/repo")
    }

    fn step_with(shell: Option<&str>) -> StepInputs {
        StepInputs {
            shell: shell.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn linux_default_is_bash() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Linux,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: StepInputs::default(),
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        assert_eq!(plan.shell.command, "bash");
        assert_eq!(plan.shell.source, ShellSource::RunnerDefault);
        assert_eq!(plan.script.extension, ".sh");
        assert_eq!(
            plan.invocation.args_format,
            "--noprofile --norc -e -o pipefail {0}"
        );
        assert!(plan.invocation.argv.iter().any(|a| a == "--noprofile"));
        assert!(plan.invocation.argv.iter().any(|a| a == "-o"));
        assert!(plan.invocation.argv.iter().any(|a| a == "pipefail"));
    }

    #[test]
    fn windows_default_is_pwsh_with_prologue() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Windows,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: StepInputs::default(),
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        assert_eq!(plan.shell.command, "pwsh");
        assert_eq!(plan.script.extension, ".ps1");
        assert_eq!(plan.script.line_ending, LineEnding::Crlf);
        assert!(
            plan.script
                .prologue
                .iter()
                .any(|l| l.contains("ErrorActionPreference"))
        );
        assert!(
            plan.script
                .epilogue
                .iter()
                .any(|l| l.contains("LASTEXITCODE"))
        );
    }

    #[test]
    fn cmd_argv_is_single_substituted_string() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Windows,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: step_with(Some("cmd")),
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        assert_eq!(plan.shell.command, "cmd");
        assert_eq!(plan.invocation.argv.len(), 1);
        assert!(plan.invocation.argv[0].contains("CALL"));
        assert!(plan.script.prologue.iter().any(|l| l == "@echo off"));
    }

    #[test]
    fn step_overrides_defaults_chain() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Linux,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: StepInputs {
                shell: Some("python".into()),
                job_defaults_run_shell: Some("bash".into()),
                workflow_defaults_run_shell: Some("sh".into()),
                ..Default::default()
            },
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        assert_eq!(plan.shell.command, "python");
        assert_eq!(plan.shell.source, ShellSource::Step);
    }

    #[test]
    fn job_defaults_chain_used_when_step_silent() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Linux,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: StepInputs {
                job_defaults_run_shell: Some("python".into()),
                workflow_defaults_run_shell: Some("sh".into()),
                ..Default::default()
            },
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        assert_eq!(plan.shell.command, "python");
        assert_eq!(plan.shell.source, ShellSource::JobDefaultsRun);
    }

    #[test]
    fn custom_shell_template_parsed() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Linux,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: step_with(Some("perl {0}")),
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        assert!(!plan.shell.builtin);
        assert_eq!(plan.shell.command, "perl");
        assert_eq!(plan.shell.args_format, "{0}");
        assert_eq!(plan.shell.extension, "");
        assert_eq!(plan.classification, Classification::Compatible);
    }

    #[test]
    fn powershell_unsupported_on_linux() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Linux,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: step_with(Some("powershell")),
        };
        let (plan, checks) = make_plan(&inputs).unwrap();
        assert_eq!(plan.classification, Classification::Unsupported);
        assert!(
            checks
                .iter()
                .any(|c| c.id == "shell.host.compat" && c.status == CheckStatus::Failed)
        );
    }

    #[test]
    fn line_ending_normalizes_for_windows() {
        let body = "line one\nline two\r\nline three";
        let out = normalize_line_endings(body, LineEnding::Crlf);
        assert_eq!(out, "line one\r\nline two\r\nline three");
    }

    #[test]
    fn line_ending_normalizes_for_unix() {
        let body = "line one\r\nline two";
        let out = normalize_line_endings(body, LineEnding::Lf);
        assert_eq!(out, "line one\nline two");
    }

    #[test]
    fn wrap_script_pwsh_has_prologue_and_epilogue() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Windows,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: step_with(Some("pwsh")),
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        let wrapped = wrap_script(&plan, "Write-Host 'hello'");
        assert!(wrapped.starts_with("$ErrorActionPreference = 'stop'\r\n"));
        assert!(wrapped.contains("Write-Host 'hello'"));
        assert!(wrapped.trim_end().ends_with("exit $LASTEXITCODE }"));
    }

    #[test]
    fn wrap_script_cmd_has_echo_off_prologue() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Windows,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: step_with(Some("cmd")),
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        let wrapped = wrap_script(&plan, "echo hello");
        assert!(wrapped.starts_with("@echo off\r\n"));
        assert!(wrapped.contains("echo hello"));
    }

    #[test]
    fn wrap_script_bash_unchanged_body() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Linux,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: StepInputs::default(),
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        let wrapped = wrap_script(&plan, "echo hello\n");
        assert_eq!(wrapped, "echo hello\n");
    }

    #[test]
    fn workdir_step_overrides_defaults() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Linux,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: StepInputs {
                working_directory: Some("subdir".into()),
                job_defaults_run_working_directory: Some("other".into()),
                ..Default::default()
            },
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        assert_eq!(plan.working_directory.source, WorkdirSource::Step);
        assert!(plan.working_directory.resolved.ends_with("subdir"));
        assert!(!plan.working_directory.absolute);
    }

    #[test]
    fn workdir_absolute_path_kept_verbatim() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Linux,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: StepInputs {
                working_directory: Some("/srv/build".into()),
                ..Default::default()
            },
        };
        let (plan, _) = make_plan(&inputs).unwrap();
        assert!(plan.working_directory.absolute);
        assert_eq!(plan.working_directory.resolved, "/srv/build");
    }

    #[test]
    fn parse_custom_shell_split_on_first_space() {
        let (cmd, args) = parse_custom_shell("perl -x {0}").unwrap();
        assert_eq!(cmd, "perl");
        assert_eq!(args, "-x {0}");

        let (cmd, args) = parse_custom_shell("python3").unwrap();
        assert_eq!(cmd, "python3");
        assert_eq!(args, "");
    }

    #[test]
    fn rejects_unrendered_expressions_in_shell() {
        let inputs = PlanInputs {
            runner_os: RunnerOs::Linux,
            workspace: workspace(),
            temp_dir: None,
            script_path: None,
            step: step_with(Some("${{ matrix.shell }}")),
        };
        let err = make_plan(&inputs).unwrap_err();
        assert!(err.to_string().contains("expression"));
    }
}
