use crate::model::{Check, CheckStatus, Plan, PlanRecord, Receipt, Summary};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Markdown,
}

pub fn render_receipt(receipt: &Receipt, format: OutputFormat) -> Result<String> {
    Ok(match format {
        OutputFormat::Json => format!("{}\n", serde_json::to_string_pretty(receipt)?),
        OutputFormat::Markdown => render_markdown(receipt),
        OutputFormat::Text => render_text(receipt),
    })
}

fn render_text(receipt: &Receipt) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} {} ({})\n",
        receipt.tool.name,
        receipt.tool.version,
        receipt.generated_at.to_rfc3339()
    ));
    out.push_str(&format!("mode: {}\n", receipt.mode));
    out.push_str(&format!(
        "summary: {} passed / {} warnings / {} failed / {} skipped\n",
        receipt.summary.passed,
        receipt.summary.warnings,
        receipt.summary.failed,
        receipt.summary.skipped
    ));
    out.push('\n');

    if !receipt.checks.is_empty() {
        out.push_str("global checks:\n");
        for check in &receipt.checks {
            push_check_line(&mut out, check, "  ");
        }
        out.push('\n');
    }

    for record in &receipt.plans {
        out.push_str(&record_header(record));
        out.push_str(&render_plan_text(&record.plan));
        if !record.checks.is_empty() {
            out.push_str("  checks:\n");
            for check in &record.checks {
                push_check_line(&mut out, check, "    ");
            }
        }
        if let Some(rendered) = &record.rendered_script {
            out.push_str(&format!(
                "  rendered-script: {} ({} bytes, {} sha256={})\n",
                rendered.path,
                rendered.bytes,
                rendered.line_ending.as_str(),
                rendered.sha256
            ));
        }
        out.push_str(&format!(
            "  plan-summary: {} passed / {} warnings / {} failed / {} skipped\n\n",
            record.summary.passed,
            record.summary.warnings,
            record.summary.failed,
            record.summary.skipped
        ));
    }

    out
}

fn render_plan_text(plan: &Plan) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "  runner-os: {}\n  shell: {} (source={}, builtin={}, classification={})\n",
        plan.runner_os.as_str(),
        plan.shell.spec.name(),
        plan.shell.source.as_str(),
        plan.shell.builtin,
        plan.classification.as_str(),
    ));
    out.push_str(&format!(
        "  args-format: {}\n  argv: {}\n",
        plan.invocation.args_format,
        plan.invocation
            .argv
            .iter()
            .map(|s| format!("`{s}`"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    out.push_str(&format!(
        "  script-extension: {}\n  line-ending: {}\n  encoding: {}\n  script-path: {}\n",
        if plan.script.extension.is_empty() {
            "(none)"
        } else {
            &plan.script.extension
        },
        plan.script.line_ending.as_str(),
        plan.script.encoding.as_str(),
        plan.script.script_path,
    ));
    out.push_str(&format!(
        "  working-directory: {} (source={}, absolute={})\n",
        plan.working_directory.resolved,
        plan.working_directory.source.as_str(),
        plan.working_directory.absolute,
    ));
    if !plan.script.prologue.is_empty() {
        out.push_str("  prologue:\n");
        for line in &plan.script.prologue {
            out.push_str(&format!("    {line}\n"));
        }
    }
    if !plan.script.epilogue.is_empty() {
        out.push_str("  epilogue:\n");
        for line in &plan.script.epilogue {
            out.push_str(&format!("    {line}\n"));
        }
    }
    if !plan.fail_fast.flags.is_empty() {
        out.push_str(&format!(
            "  fail-fast-flags: {}\n",
            plan.fail_fast.flags.join(" ")
        ));
    }
    if let Some(pref) = &plan.fail_fast.error_action_preference {
        out.push_str(&format!("  error-action-preference: {pref}\n"));
    }
    if plan.fail_fast.propagates_lastexitcode {
        out.push_str("  propagates-lastexitcode: true\n");
    }
    out
}

fn render_markdown(receipt: &Receipt) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {} {}\n\n",
        receipt.tool.name, receipt.tool.version
    ));
    out.push_str(&format!(
        "- generated: `{}`\n",
        receipt.generated_at.to_rfc3339()
    ));
    out.push_str(&format!("- mode: `{}`\n", receipt.mode));
    out.push_str(&format!(
        "- summary: {}\n\n",
        summary_pill(&receipt.summary)
    ));

    if !receipt.checks.is_empty() {
        out.push_str("## Global checks\n\n");
        out.push_str("| status | id | message |\n| --- | --- | --- |\n");
        for check in &receipt.checks {
            out.push_str(&markdown_check_row(check));
        }
        out.push('\n');
    }

    for (idx, record) in receipt.plans.iter().enumerate() {
        out.push_str(&format!("## Plan {}\n\n", idx + 1));
        if let Some(workflow) = &record.workflow {
            out.push_str(&format!("- workflow: `{workflow}`\n"));
        }
        if let Some(job) = &record.job {
            out.push_str(&format!("- job: `{job}`\n"));
        }
        if let Some(idx) = record.step_index {
            out.push_str(&format!("- step-index: {idx}\n"));
        }
        if let Some(id) = &record.step_id {
            out.push_str(&format!("- step-id: `{id}`\n"));
        }
        if let Some(name) = &record.step_name {
            out.push_str(&format!("- step-name: `{name}`\n"));
        }
        out.push_str(&format!(
            "- runner-os: `{}`\n- shell: `{}` (source `{}`, builtin `{}`)\n- classification: `{}`\n- args-format: `{}`\n- script-extension: `{}`\n- line-ending: `{}`\n- encoding: `{}`\n- script-path: `{}`\n- working-directory: `{}` (source `{}`)\n",
            record.plan.runner_os.as_str(),
            record.plan.shell.spec.name(),
            record.plan.shell.source.as_str(),
            record.plan.shell.builtin,
            record.plan.classification.as_str(),
            record.plan.invocation.args_format,
            if record.plan.script.extension.is_empty() {
                "(none)"
            } else {
                &record.plan.script.extension
            },
            record.plan.script.line_ending.as_str(),
            record.plan.script.encoding.as_str(),
            record.plan.script.script_path,
            record.plan.working_directory.resolved,
            record.plan.working_directory.source.as_str(),
        ));

        if !record.plan.invocation.argv.is_empty() {
            out.push_str("- argv:\n");
            for arg in &record.plan.invocation.argv {
                out.push_str(&format!("  - `{arg}`\n"));
            }
        }
        if !record.plan.script.prologue.is_empty() {
            out.push_str("- prologue:\n");
            for line in &record.plan.script.prologue {
                out.push_str(&format!("  - `{line}`\n"));
            }
        }
        if !record.plan.script.epilogue.is_empty() {
            out.push_str("- epilogue:\n");
            for line in &record.plan.script.epilogue {
                out.push_str(&format!("  - `{line}`\n"));
            }
        }
        if !record.plan.fail_fast.flags.is_empty() {
            out.push_str(&format!(
                "- fail-fast-flags: `{}`\n",
                record.plan.fail_fast.flags.join(" ")
            ));
        }
        if let Some(pref) = &record.plan.fail_fast.error_action_preference {
            out.push_str(&format!("- error-action-preference: `{pref}`\n"));
        }
        if record.plan.fail_fast.propagates_lastexitcode {
            out.push_str("- propagates-lastexitcode: `true`\n");
        }
        if let Some(rendered) = &record.rendered_script {
            out.push_str(&format!(
                "- rendered-script: `{}` ({} bytes, sha256 `{}`)\n",
                rendered.path, rendered.bytes, rendered.sha256
            ));
        }

        out.push_str(&format!("- summary: {}\n\n", summary_pill(&record.summary)));

        if !record.checks.is_empty() {
            out.push_str("| status | id | message |\n| --- | --- | --- |\n");
            for check in &record.checks {
                out.push_str(&markdown_check_row(check));
            }
            out.push('\n');
        }
    }

    out
}

fn summary_pill(summary: &Summary) -> String {
    format!(
        "{} passed / {} warnings / {} failed / {} skipped",
        summary.passed, summary.warnings, summary.failed, summary.skipped
    )
}

fn push_check_line(out: &mut String, check: &Check, indent: &str) {
    let label = match check.status {
        CheckStatus::Passed => "PASS",
        CheckStatus::Warning => "WARN",
        CheckStatus::Failed => "FAIL",
        CheckStatus::Skipped => "SKIP",
    };
    out.push_str(&format!(
        "{indent}{label} {} :: {}\n",
        check.id, check.message
    ));
    if let Some(detail) = &check.detail {
        out.push_str(&format!("{indent}  ↳ {detail}\n"));
    }
    if let Some(class) = &check.classification {
        out.push_str(&format!("{indent}  ↳ classification={}\n", class.as_str()));
    }
}

fn markdown_check_row(check: &Check) -> String {
    let status = match check.status {
        CheckStatus::Passed => "pass",
        CheckStatus::Warning => "warn",
        CheckStatus::Failed => "fail",
        CheckStatus::Skipped => "skip",
    };
    let mut message = check.message.replace('|', "\\|");
    if let Some(detail) = &check.detail {
        message.push_str(&format!(" — {}", detail.replace('|', "\\|")));
    }
    if let Some(class) = &check.classification {
        message.push_str(&format!(" (classification `{}`)", class.as_str()));
    }
    format!("| `{status}` | `{}` | {} |\n", check.id, message)
}

fn record_header(record: &PlanRecord) -> String {
    let mut header = String::from("plan:");
    if let Some(workflow) = &record.workflow {
        header.push_str(&format!(" workflow={workflow}"));
    }
    if let Some(job) = &record.job {
        header.push_str(&format!(" job={job}"));
    }
    if let Some(idx) = record.step_index {
        header.push_str(&format!(" step-index={idx}"));
    }
    if let Some(id) = &record.step_id {
        header.push_str(&format!(" step-id={id}"));
    }
    if let Some(name) = &record.step_name {
        header.push_str(&format!(" step-name={name:?}"));
    }
    header.push('\n');
    header
}
