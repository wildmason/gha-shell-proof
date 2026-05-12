//! Minimal workflow YAML scanner. Owns just enough parsing to find `run:`
//! steps, the chain of `defaults.run` blocks, and the runner OS implied by
//! `runs-on:`. Anything more elaborate (expressions, matrices, reusable
//! workflows) is out of scope by design — see `gha-workflow-proof`.

use crate::engine::{PlanInputs, StepInputs, has_blocking_failure, make_plan, summarize};
use crate::model::{Check, CheckStatus, PlanRecord, RunnerOs};
use anyhow::{Context, Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use serde_yaml::Value;
use std::fs;

#[derive(Debug, Clone)]
pub struct WorkflowScanOptions {
    pub workspace: Utf8PathBuf,
    pub temp_dir: Option<Utf8PathBuf>,
    pub override_runner_os: Option<RunnerOs>,
}

pub fn scan_workflow(
    workflow_path: &Utf8Path,
    options: &WorkflowScanOptions,
) -> Result<Vec<PlanRecord>> {
    let raw = fs::read_to_string(workflow_path)
        .with_context(|| format!("reading workflow {workflow_path}"))?;
    let trimmed = strip_bom(&raw);
    let doc: Value = serde_yaml::from_str(trimmed)
        .with_context(|| format!("parsing workflow {workflow_path}"))?;

    let workflow_defaults = extract_defaults_run(&doc);

    let jobs = doc
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("{workflow_path}: workflow has no `jobs` mapping"))?;

    let mut records = Vec::new();

    for (job_key, job_value) in jobs {
        let job_name = job_key.as_str().unwrap_or("(job)").to_owned();

        let job_defaults = extract_defaults_run(job_value);
        let runs_on = extract_runs_on(job_value);
        let inferred_os = options
            .override_runner_os
            .or_else(|| infer_runner_os(&runs_on));

        let steps = job_value
            .get("steps")
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default();

        for (step_idx, step_value) in steps.iter().enumerate() {
            let run = step_value.get("run").and_then(value_to_string);
            if run.is_none() {
                continue;
            }

            let step_id = step_value.get("id").and_then(value_to_string);
            let step_name = step_value.get("name").and_then(value_to_string);
            let step_shell = step_value.get("shell").and_then(value_to_string);
            let step_workdir = step_value
                .get("working-directory")
                .and_then(value_to_string);

            let mut record_checks: Vec<Check> = Vec::new();

            let runner_os = match inferred_os {
                Some(os) => os,
                None => {
                    let mut record = unresolvable_record(
                        workflow_path,
                        &job_name,
                        step_idx,
                        step_id.clone(),
                        step_name.clone(),
                        &runs_on,
                    );
                    record.checks.append(&mut record_checks);
                    record.summary = summarize(&record.checks);
                    records.push(record);
                    continue;
                }
            };

            if options.override_runner_os.is_none() && runs_on_has_expression(&runs_on) {
                record_checks.push(Check::warning(
                    "workflow.runs-on.expression",
                    "runs-on contains an unrendered `${{ ... }}` expression; \
                     using the inferred OS where possible — pass --runner-os to override",
                ));
            }

            let inputs = PlanInputs {
                runner_os,
                workspace: options.workspace.clone(),
                temp_dir: options.temp_dir.clone(),
                script_path: None,
                step: StepInputs {
                    shell: step_shell.clone(),
                    working_directory: step_workdir.clone(),
                    job_defaults_run_shell: job_defaults.shell.clone(),
                    job_defaults_run_working_directory: job_defaults.working_directory.clone(),
                    workflow_defaults_run_shell: workflow_defaults.shell.clone(),
                    workflow_defaults_run_working_directory: workflow_defaults
                        .working_directory
                        .clone(),
                },
            };

            match make_plan(&inputs) {
                Ok((plan, mut checks)) => {
                    checks.append(&mut record_checks);
                    let summary = summarize(&checks);
                    records.push(PlanRecord {
                        workflow: Some(workflow_path.as_str().to_owned()),
                        job: Some(job_name.clone()),
                        step_index: Some(step_idx),
                        step_id,
                        step_name,
                        plan,
                        checks,
                        summary,
                        rendered_script: None,
                    });
                }
                Err(err) => {
                    record_checks.push(Check::failed(
                        "workflow.step.plan",
                        format!("failed to plan step: {err}"),
                    ));
                    let summary = summarize(&record_checks);
                    records.push(PlanRecord {
                        workflow: Some(workflow_path.as_str().to_owned()),
                        job: Some(job_name.clone()),
                        step_index: Some(step_idx),
                        step_id,
                        step_name,
                        plan: placeholder_plan(runner_os),
                        checks: record_checks,
                        summary,
                        rendered_script: None,
                    });
                }
            }
        }
    }

    Ok(records)
}

#[derive(Debug, Clone, Default)]
struct DefaultsRun {
    shell: Option<String>,
    working_directory: Option<String>,
}

fn extract_defaults_run(value: &Value) -> DefaultsRun {
    let Some(run) = value.get("defaults").and_then(|d| d.get("run")) else {
        return DefaultsRun::default();
    };
    DefaultsRun {
        shell: run.get("shell").and_then(value_to_string),
        working_directory: run.get("working-directory").and_then(value_to_string),
    }
}

fn extract_runs_on(job_value: &Value) -> Vec<String> {
    let Some(node) = job_value.get("runs-on") else {
        return Vec::new();
    };
    match node {
        Value::String(s) => vec![s.clone()],
        Value::Sequence(items) => items.iter().filter_map(value_to_string).collect(),
        // `runs-on: { group: ..., labels: [...] }` (GitHub's runner-groups syntax).
        Value::Mapping(_) => match node.get("labels") {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Sequence(items)) => items.iter().filter_map(value_to_string).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn infer_runner_os(labels: &[String]) -> Option<RunnerOs> {
    for label in labels {
        let low = label.to_ascii_lowercase();
        if low.contains("windows") {
            return Some(RunnerOs::Windows);
        }
        if low.contains("macos") || low.contains("macos-") {
            return Some(RunnerOs::Macos);
        }
        if low.contains("ubuntu") || low == "linux" || low.starts_with("linux-") {
            return Some(RunnerOs::Linux);
        }
    }
    None
}

fn runs_on_has_expression(labels: &[String]) -> bool {
    labels.iter().any(|l| l.contains("${{"))
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn strip_bom(raw: &str) -> &str {
    raw.strip_prefix('\u{feff}').unwrap_or(raw)
}

fn unresolvable_record(
    workflow_path: &Utf8Path,
    job_name: &str,
    step_idx: usize,
    step_id: Option<String>,
    step_name: Option<String>,
    runs_on: &[String],
) -> PlanRecord {
    let mut checks = vec![Check::failed(
        "workflow.runs-on.runner-os-unresolved",
        format!(
            "could not infer runner OS from runs-on labels {:?}; pass --runner-os to override",
            runs_on
        ),
    )];
    if runs_on_has_expression(runs_on) {
        checks.push(Check::warning(
            "workflow.runs-on.expression",
            "runs-on contains an unrendered `${{ ... }}` expression; \
             pass --runner-os to evaluate this step",
        ));
    }
    let summary = summarize(&checks);
    PlanRecord {
        workflow: Some(workflow_path.as_str().to_owned()),
        job: Some(job_name.to_owned()),
        step_index: Some(step_idx),
        step_id,
        step_name,
        plan: placeholder_plan(RunnerOs::Linux),
        checks,
        summary,
        rendered_script: None,
    }
}

fn placeholder_plan(runner_os: RunnerOs) -> crate::model::Plan {
    use crate::model::{
        Classification, Encoding, FailFast, Invocation, LineEnding, Plan, ResolvedShell,
        ResolvedWorkdir, ScriptPlan, ShellSource, ShellSpec, WorkdirSource,
    };
    Plan {
        runner_os,
        shell: ResolvedShell {
            spec: ShellSpec::Builtin {
                name: "bash".to_owned(),
            },
            source: ShellSource::RunnerDefault,
            builtin: true,
            command: "bash".to_owned(),
            args_format: String::new(),
            extension: ".sh".to_owned(),
        },
        working_directory: ResolvedWorkdir {
            source: WorkdirSource::Workspace,
            requested: None,
            workspace: ".".to_owned(),
            resolved: ".".to_owned(),
            absolute: false,
        },
        script: ScriptPlan {
            extension: ".sh".to_owned(),
            line_ending: LineEnding::Lf,
            encoding: Encoding::Utf8NoBom,
            temp_filename_pattern: String::new(),
            script_path: String::new(),
            prologue: Vec::new(),
            epilogue: Vec::new(),
        },
        invocation: Invocation {
            command: "bash".to_owned(),
            args_format: String::new(),
            argv: Vec::new(),
            working_directory: ".".to_owned(),
        },
        fail_fast: FailFast::default(),
        classification: Classification::Unsupported,
    }
}

/// True if any record's checks include a `failed` entry.
pub fn records_have_failures(records: &[PlanRecord]) -> bool {
    records.iter().any(|r| has_blocking_failure(&r.checks))
}

/// True if any record's checks include a `warning` entry.
pub fn records_have_warnings(records: &[PlanRecord]) -> bool {
    records
        .iter()
        .any(|r| r.checks.iter().any(|c| c.status == CheckStatus::Warning))
}
