use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use clap::{Args, Parser, Subcommand};
use gha_shell_proof::{
    Check, LineEnding, OutputFormat, PlanInputs, PlanRecord, Receipt, RenderedScript, RunnerOs,
    SCHEMA_VERSION, StepInputs, Summary, TOOL_NAME, TOOL_VERSION, ToolStamp, WorkflowScanOptions,
    make_plan, render_receipt, scan_workflow, summarize, wrap_script,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Plan and validate GitHub Actions `run:`-step shell invocations"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, global = true, value_enum, default_value = "text")]
    format: OutputFormat,

    #[arg(long, global = true, value_name = "PATH")]
    output: Option<Utf8PathBuf>,

    #[arg(long, global = true)]
    strict: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    Plan(PlanArgs),
    Render(RenderArgs),
    CheckWorkflow(CheckWorkflowArgs),
}

#[derive(Debug, Args, Clone)]
struct CommonPlanArgs {
    #[arg(long, value_enum)]
    runner_os: RunnerOs,

    #[arg(long, value_name = "SHELL")]
    shell: Option<String>,

    #[arg(long, value_name = "PATH")]
    working_directory: Option<String>,

    #[arg(long, value_name = "PATH", default_value = ".")]
    workspace: Utf8PathBuf,

    #[arg(long, value_name = "PATH")]
    temp_dir: Option<Utf8PathBuf>,

    #[arg(long, value_name = "PATH")]
    script_path: Option<Utf8PathBuf>,

    #[arg(long, value_name = "SHELL")]
    job_defaults_run_shell: Option<String>,

    #[arg(long, value_name = "PATH")]
    job_defaults_run_working_directory: Option<String>,

    #[arg(long, value_name = "SHELL")]
    defaults_run_shell: Option<String>,

    #[arg(long, value_name = "PATH")]
    defaults_run_working_directory: Option<String>,
}

#[derive(Debug, Args)]
struct PlanArgs {
    #[command(flatten)]
    common: CommonPlanArgs,
}

#[derive(Debug, Args)]
struct RenderArgs {
    #[command(flatten)]
    common: CommonPlanArgs,

    #[arg(long, value_name = "PATH")]
    script: Utf8PathBuf,

    #[arg(long, value_name = "PATH")]
    output_script: Utf8PathBuf,
}

#[derive(Debug, Args)]
struct CheckWorkflowArgs {
    #[arg(long, value_name = "PATH", required = true)]
    workflow: Vec<Utf8PathBuf>,

    #[arg(long, value_name = "PATH", default_value = ".")]
    workspace: Utf8PathBuf,

    #[arg(long, value_name = "PATH")]
    temp_dir: Option<Utf8PathBuf>,

    #[arg(long, value_enum)]
    runner_os: Option<RunnerOs>,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let receipt = match &cli.command {
        Command::Plan(args) => run_plan(args)?,
        Command::Render(args) => run_render(args)?,
        Command::CheckWorkflow(args) => run_check_workflow(args)?,
    };

    let rendered = render_receipt(&receipt, cli.format)?;
    if let Some(output) = &cli.output {
        if let Some(parent) = output.parent()
            && !parent.as_str().is_empty()
        {
            fs::create_dir_all(parent).with_context(|| format!("creating {parent}"))?;
        }
        fs::write(output, rendered).with_context(|| format!("writing {output}"))?;
    } else {
        print!("{rendered}");
    }

    let failed = receipt.summary.failed > 0;
    let warnings = receipt.summary.warnings > 0;
    if failed || (cli.strict && warnings) {
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

fn step_inputs_from(common: &CommonPlanArgs) -> StepInputs {
    StepInputs {
        shell: common.shell.clone(),
        working_directory: common.working_directory.clone(),
        job_defaults_run_shell: common.job_defaults_run_shell.clone(),
        job_defaults_run_working_directory: common.job_defaults_run_working_directory.clone(),
        workflow_defaults_run_shell: common.defaults_run_shell.clone(),
        workflow_defaults_run_working_directory: common.defaults_run_working_directory.clone(),
    }
}

fn plan_inputs_from(common: &CommonPlanArgs) -> PlanInputs {
    PlanInputs {
        runner_os: common.runner_os,
        workspace: common.workspace.clone(),
        temp_dir: common.temp_dir.clone(),
        script_path: common.script_path.clone(),
        step: step_inputs_from(common),
    }
}

fn run_plan(args: &PlanArgs) -> Result<Receipt> {
    let inputs = plan_inputs_from(&args.common);
    let (plan, checks) = make_plan(&inputs)?;
    let summary = summarize(&checks);
    let record = PlanRecord {
        workflow: None,
        job: None,
        step_index: None,
        step_id: None,
        step_name: None,
        plan,
        checks,
        summary: summary.clone(),
        rendered_script: None,
    };
    Ok(receipt_for("plan", vec![record], Vec::new(), summary))
}

fn run_render(args: &RenderArgs) -> Result<Receipt> {
    let inputs = plan_inputs_from(&args.common);
    let (plan, mut checks) = make_plan(&inputs)?;

    let body =
        fs::read_to_string(&args.script).with_context(|| format!("reading {0}", args.script))?;
    let wrapped = wrap_script(&plan, &body);
    let bytes = wrapped.as_bytes();

    if let Some(parent) = args.output_script.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {parent}"))?;
    }
    write_script_file(&args.output_script, bytes, plan.script.encoding)?;

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sha256 = hex_lower(&hasher.finalize());
    let rendered_script = RenderedScript {
        path: args.output_script.as_str().to_owned(),
        line_ending: plan.script.line_ending,
        encoding: plan.script.encoding,
        bytes: bytes.len() as u64,
        sha256,
    };

    checks.push(
        Check::passed(
            "shell.script.rendered",
            format!(
                "rendered wrapped script to {} ({} bytes, line-ending={})",
                args.output_script,
                bytes.len(),
                plan.script.line_ending.as_str()
            ),
        )
        .with_detail(format!("path={}", args.output_script)),
    );

    sanity_check_line_endings(bytes, plan.script.line_ending, &mut checks);

    let summary = summarize(&checks);
    let record = PlanRecord {
        workflow: None,
        job: None,
        step_index: None,
        step_id: None,
        step_name: None,
        plan,
        checks,
        summary: summary.clone(),
        rendered_script: Some(rendered_script),
    };
    Ok(receipt_for("render", vec![record], Vec::new(), summary))
}

fn run_check_workflow(args: &CheckWorkflowArgs) -> Result<Receipt> {
    let mut all_records: Vec<PlanRecord> = Vec::new();
    let mut global_checks: Vec<Check> = Vec::new();
    let mut overall = Summary::default();

    for workflow_path in &args.workflow {
        if !workflow_path.exists() {
            bail!("workflow {workflow_path} does not exist");
        }
        let options = WorkflowScanOptions {
            workspace: args.workspace.clone(),
            temp_dir: args.temp_dir.clone(),
            override_runner_os: args.runner_os,
        };
        let records = scan_workflow(workflow_path, &options)?;
        if records.is_empty() {
            global_checks.push(
                Check::warning(
                    "workflow.run-steps.found",
                    format!("workflow {workflow_path} contained no `run:` steps"),
                )
                .with_detail(format!("workflow={workflow_path}")),
            );
        } else {
            global_checks.push(
                Check::passed(
                    "workflow.run-steps.found",
                    format!(
                        "workflow {} planned {} run-step(s)",
                        workflow_path,
                        records.len()
                    ),
                )
                .with_detail(format!("workflow={workflow_path}")),
            );
        }

        for record in records {
            overall.extend(&record.summary);
            all_records.push(record);
        }
    }

    let global_summary = summarize(&global_checks);
    overall.extend(&global_summary);
    Ok(receipt_for(
        "check-workflow",
        all_records,
        global_checks,
        overall,
    ))
}

fn receipt_for(
    mode: &str,
    plans: Vec<PlanRecord>,
    global_checks: Vec<Check>,
    summary: Summary,
) -> Receipt {
    Receipt {
        schema_version: SCHEMA_VERSION,
        tool: ToolStamp {
            name: TOOL_NAME.to_owned(),
            version: TOOL_VERSION.to_owned(),
        },
        generated_at: Utc::now(),
        mode: mode.to_owned(),
        plans,
        checks: global_checks,
        summary,
    }
}

fn sanity_check_line_endings(bytes: &[u8], expected: LineEnding, checks: &mut Vec<Check>) {
    let stray = match expected {
        LineEnding::Crlf => stray_lf(bytes),
        LineEnding::Lf => bytes.contains(&b'\r'),
    };
    if stray {
        checks.push(Check::warning(
            "shell.script.line-ending.consistent",
            format!(
                "rendered script contains line endings inconsistent with expected `{}`",
                expected.as_str()
            ),
        ));
    } else {
        checks.push(Check::passed(
            "shell.script.line-ending.consistent",
            format!("rendered script line endings match `{}`", expected.as_str()),
        ));
    }
}

fn stray_lf(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' && (i == 0 || bytes[i - 1] != b'\r') {
            return true;
        }
        i += 1;
    }
    false
}

fn write_script_file(
    path: &Utf8Path,
    bytes: &[u8],
    encoding: gha_shell_proof::Encoding,
) -> Result<()> {
    use gha_shell_proof::Encoding;
    let _ = encoding; // BOM behavior is observational; we never inject a BOM here.
    fs::write(path, bytes).with_context(|| format!("writing {path}"))?;
    // We never emit a BOM regardless of OS — `Encoding::Utf8NoBom` describes
    // what the runner does; we just describe it. The runner-os difference for
    // the encoding receipt is observational.
    let _ = Encoding::Utf8NoBom;
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod selftests {
    use super::*;

    #[test]
    fn stray_lf_detects_bare_lf() {
        assert!(stray_lf(b"a\nb"));
        assert!(!stray_lf(b"a\r\nb"));
        assert!(!stray_lf(b"abc"));
    }
}
