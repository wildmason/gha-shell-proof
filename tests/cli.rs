use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

fn cmd() -> Command {
    Command::cargo_bin("gha-shell-proof").expect("binary built")
}

#[test]
fn plan_linux_default_is_bash_and_passes() {
    let assert = cmd()
        .args(["plan", "--runner-os", "linux", "--format", "json"])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let receipt: Value = serde_json::from_str(&out).expect("valid json");
    let plan = &receipt["plans"][0]["plan"];
    assert_eq!(plan["shell"]["command"], "bash");
    assert_eq!(plan["shell"]["source"], "runner-default");
    assert_eq!(plan["shell"]["extension"], ".sh");
    assert_eq!(plan["script"]["line_ending"], "lf");
    assert_eq!(plan["script"]["encoding"], "utf-8-no-bom");
    assert_eq!(plan["classification"], "exact");
    assert_eq!(receipt["summary"]["failed"], 0);
}

#[test]
fn plan_windows_pwsh_has_prologue_epilogue_and_crlf() {
    let assert = cmd()
        .args([
            "plan",
            "--runner-os",
            "windows",
            "--shell",
            "pwsh",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let receipt: Value = serde_json::from_str(&out).unwrap();
    let plan = &receipt["plans"][0]["plan"];
    assert_eq!(plan["shell"]["command"], "pwsh");
    assert_eq!(plan["shell"]["extension"], ".ps1");
    assert_eq!(plan["script"]["line_ending"], "crlf");
    assert_eq!(plan["script"]["encoding"], "utf-8");
    assert_eq!(
        plan["script"]["prologue"][0],
        "$ErrorActionPreference = 'stop'"
    );
    assert!(
        plan["script"]["epilogue"][0]
            .as_str()
            .unwrap()
            .contains("LASTEXITCODE")
    );
    assert_eq!(plan["fail_fast"]["error_action_preference"], "stop");
    assert_eq!(plan["fail_fast"]["propagates_lastexitcode"], true);
    assert_eq!(plan["classification"], "exact");
}

#[test]
fn plan_windows_cmd_argv_is_single_substituted_string() {
    let assert = cmd()
        .args([
            "plan",
            "--runner-os",
            "windows",
            "--shell",
            "cmd",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let receipt: Value = serde_json::from_str(&out).unwrap();
    let plan = &receipt["plans"][0]["plan"];
    assert_eq!(plan["shell"]["extension"], ".cmd");
    let argv = plan["invocation"]["argv"].as_array().unwrap();
    assert_eq!(argv.len(), 1);
    assert!(argv[0].as_str().unwrap().contains("CALL"));
    assert_eq!(plan["script"]["prologue"][0], "@echo off");
}

#[test]
fn plan_powershell_on_linux_is_unsupported_and_fails() {
    cmd()
        .args([
            "plan",
            "--runner-os",
            "linux",
            "--shell",
            "powershell",
            "--format",
            "text",
        ])
        .assert()
        .failure()
        .stdout(contains("classification=unsupported").or(contains("`unsupported`")));
}

#[test]
fn plan_custom_shell_template_is_compatible() {
    let assert = cmd()
        .args([
            "plan",
            "--runner-os",
            "linux",
            "--shell",
            "perl {0}",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let receipt: Value = serde_json::from_str(&out).unwrap();
    let plan = &receipt["plans"][0]["plan"];
    assert_eq!(plan["shell"]["builtin"], false);
    assert_eq!(plan["shell"]["command"], "perl");
    assert_eq!(plan["shell"]["args_format"], "{0}");
    assert_eq!(plan["classification"], "compatible");
    assert!(
        receipt["plans"][0]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "shell.custom.template.placeholder")
    );
}

#[test]
fn plan_custom_shell_missing_placeholder_warns() {
    let assert = cmd()
        .args([
            "plan",
            "--runner-os",
            "linux",
            "--shell",
            "perl",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let receipt: Value = serde_json::from_str(&out).unwrap();
    let warnings = receipt["plans"][0]["summary"]["warnings"].as_u64().unwrap();
    assert!(warnings >= 1);
}

#[test]
fn plan_resolves_shell_from_workflow_defaults_when_step_silent() {
    let assert = cmd()
        .args([
            "plan",
            "--runner-os",
            "linux",
            "--defaults-run-shell",
            "sh",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let receipt: Value = serde_json::from_str(&out).unwrap();
    let plan = &receipt["plans"][0]["plan"];
    assert_eq!(plan["shell"]["command"], "sh");
    assert_eq!(plan["shell"]["source"], "workflow-defaults-run");
}

#[test]
fn plan_step_shell_overrides_job_and_workflow_defaults() {
    let assert = cmd()
        .args([
            "plan",
            "--runner-os",
            "linux",
            "--shell",
            "python",
            "--job-defaults-run-shell",
            "bash",
            "--defaults-run-shell",
            "sh",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let receipt: Value = serde_json::from_str(&out).unwrap();
    let plan = &receipt["plans"][0]["plan"];
    assert_eq!(plan["shell"]["command"], "python");
    assert_eq!(plan["shell"]["source"], "step");
}

#[test]
fn plan_strict_promotes_warning_to_failure() {
    cmd()
        .args([
            "plan",
            "--runner-os",
            "linux",
            "--shell",
            "perl",
            "--strict",
        ])
        .assert()
        .failure();
}

#[test]
fn plan_writes_output_file() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("receipt.json");
    cmd()
        .args([
            "plan",
            "--runner-os",
            "linux",
            "--format",
            "json",
            "--output",
        ])
        .arg(&out)
        .assert()
        .success();
    let body = fs::read_to_string(&out).unwrap();
    let receipt: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(receipt["mode"], "plan");
}

#[test]
fn render_pwsh_writes_wrapped_script_with_crlf() {
    let dir = tempdir().unwrap();
    let script = dir.path().join("step.ps1");
    fs::write(&script, "Write-Host 'hello'\nWrite-Host 'world'\n").unwrap();
    let out_script = dir.path().join("rendered.ps1");
    cmd()
        .args([
            "render",
            "--runner-os",
            "windows",
            "--shell",
            "pwsh",
            "--script",
        ])
        .arg(&script)
        .arg("--output-script")
        .arg(&out_script)
        .args(["--format", "json"])
        .assert()
        .success();
    let body = fs::read(&out_script).unwrap();
    let text = String::from_utf8_lossy(&body).into_owned();
    assert!(text.starts_with("$ErrorActionPreference = 'stop'\r\n"));
    assert!(text.contains("Write-Host 'hello'\r\n"));
    assert!(text.trim_end().ends_with("exit $LASTEXITCODE }"));
    // Confirm CRLF is the actual on-disk line ending.
    assert!(body.windows(2).any(|w| w == [b'\r', b'\n']));
}

#[test]
fn render_bash_leaves_body_with_lf() {
    let dir = tempdir().unwrap();
    let script = dir.path().join("step.sh");
    fs::write(&script, "echo hello\necho world\n").unwrap();
    let out_script = dir.path().join("rendered.sh");
    cmd()
        .args(["render", "--runner-os", "linux", "--script"])
        .arg(&script)
        .arg("--output-script")
        .arg(&out_script)
        .args(["--format", "json"])
        .assert()
        .success();
    let body = fs::read(&out_script).unwrap();
    assert_eq!(body, b"echo hello\necho world\n");
}

#[test]
fn render_cmd_prepends_echo_off() {
    let dir = tempdir().unwrap();
    let script = dir.path().join("step.cmd");
    fs::write(&script, "echo step ran\n").unwrap();
    let out_script = dir.path().join("rendered.cmd");
    cmd()
        .args([
            "render",
            "--runner-os",
            "windows",
            "--shell",
            "cmd",
            "--script",
        ])
        .arg(&script)
        .arg("--output-script")
        .arg(&out_script)
        .args(["--format", "json"])
        .assert()
        .success();
    let text = fs::read_to_string(&out_script).unwrap();
    assert!(text.starts_with("@echo off\r\n"));
    assert!(text.contains("echo step ran"));
}

#[test]
fn check_workflow_produces_one_record_per_run_step() {
    let dir = tempdir().unwrap();
    let workflow = dir.path().join("ci.yml");
    fs::write(
        &workflow,
        r#"name: CI
on: [push]
defaults:
  run:
    shell: bash
jobs:
  build:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: app
    steps:
      - uses: actions/checkout@v4
      - name: build
        run: cargo build
      - name: test
        shell: pwsh
        working-directory: /srv/x
        run: |
          Write-Host 'hello'
  release:
    runs-on: windows-latest
    steps:
      - name: package
        run: dir
"#,
    )
    .unwrap();
    let assert = cmd()
        .args(["check-workflow", "--workflow"])
        .arg(&workflow)
        .args(["--format", "json"])
        .assert()
        .success();
    let receipt: Value = serde_json::from_slice(&assert.get_output().stdout).expect("valid json");
    let plans = receipt["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 3, "expected three run-steps planned");

    let first = &plans[0];
    assert_eq!(first["job"], "build");
    assert_eq!(first["plan"]["runner_os"], "linux");
    assert_eq!(first["plan"]["shell"]["command"], "bash");
    assert_eq!(
        first["plan"]["shell"]["source"], "workflow-defaults-run",
        "step inherits workflow-level defaults.run.shell"
    );
    assert!(
        first["plan"]["working_directory"]["resolved"]
            .as_str()
            .unwrap()
            .ends_with("app"),
        "step inherits job-level defaults.run.working-directory"
    );

    let second = &plans[1];
    assert_eq!(second["plan"]["shell"]["command"], "pwsh");
    assert_eq!(second["plan"]["shell"]["source"], "step");
    assert_eq!(
        second["plan"]["working_directory"]["resolved"], "/srv/x",
        "step-level working-directory wins"
    );
    assert_eq!(
        second["plan"]["working_directory"]["absolute"], true,
        "linux-runner absolute path detected"
    );
    assert_eq!(
        second["plan"]["classification"], "compatible",
        "pwsh on linux is compatible"
    );

    let third = &plans[2];
    assert_eq!(third["plan"]["runner_os"], "windows");
    // Workflow-level `defaults.run.shell: bash` overrides the windows-runner
    // default of pwsh; bash on windows is compatible (Git for Windows).
    assert_eq!(third["plan"]["shell"]["command"], "bash");
    assert_eq!(third["plan"]["shell"]["source"], "workflow-defaults-run");
    assert_eq!(third["plan"]["classification"], "compatible");
    assert_eq!(third["plan"]["script"]["line_ending"], "crlf");
}

#[test]
fn check_workflow_windows_job_without_workflow_defaults_uses_pwsh() {
    let dir = tempdir().unwrap();
    let workflow = dir.path().join("win.yml");
    fs::write(
        &workflow,
        r#"name: win
on: [push]
jobs:
  release:
    runs-on: windows-latest
    steps:
      - name: package
        run: dir
"#,
    )
    .unwrap();
    let assert = cmd()
        .args(["check-workflow", "--workflow"])
        .arg(&workflow)
        .args(["--format", "json"])
        .assert()
        .success();
    let receipt: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let plan = &receipt["plans"][0]["plan"];
    assert_eq!(plan["runner_os"], "windows");
    assert_eq!(plan["shell"]["command"], "pwsh");
    assert_eq!(plan["shell"]["source"], "runner-default");
    assert_eq!(plan["classification"], "exact");
}

#[test]
fn check_workflow_warns_on_no_run_steps() {
    let dir = tempdir().unwrap();
    let workflow = dir.path().join("uses-only.yml");
    fs::write(
        &workflow,
        r#"name: uses-only
on: [push]
jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
"#,
    )
    .unwrap();
    let assert = cmd()
        .args(["check-workflow", "--workflow"])
        .arg(&workflow)
        .args(["--format", "json"])
        .assert()
        .success();
    let receipt: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert!(receipt["plans"].as_array().unwrap().is_empty());
    let global = receipt["checks"].as_array().unwrap();
    assert!(
        global
            .iter()
            .any(|c| c["id"] == "workflow.run-steps.found" && c["status"] == "warning")
    );
}

#[test]
fn check_workflow_unresolvable_runs_on_without_override_fails() {
    let dir = tempdir().unwrap();
    let workflow = dir.path().join("matrix.yml");
    fs::write(
        &workflow,
        r#"name: matrix
on: [push]
jobs:
  ci:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]
    steps:
      - run: echo hello
"#,
    )
    .unwrap();
    let assert = cmd()
        .args(["check-workflow", "--workflow"])
        .arg(&workflow)
        .args(["--format", "json"])
        .assert()
        .failure();
    let receipt: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let plans = receipt["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 1);
    assert!(
        plans[0]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "workflow.runs-on.runner-os-unresolved" && c["status"] == "failed")
    );
}

#[test]
fn check_workflow_runner_os_override_wins_over_unresolvable_runs_on() {
    let dir = tempdir().unwrap();
    let workflow = dir.path().join("override.yml");
    fs::write(
        &workflow,
        r#"name: override
on: [push]
jobs:
  ci:
    runs-on: ${{ matrix.os }}
    steps:
      - run: echo hello
"#,
    )
    .unwrap();
    let assert = cmd()
        .args(["check-workflow", "--runner-os", "windows", "--workflow"])
        .arg(&workflow)
        .args(["--format", "json"])
        .assert()
        .success();
    let receipt: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let plan = &receipt["plans"][0]["plan"];
    assert_eq!(plan["runner_os"], "windows");
    assert_eq!(plan["shell"]["command"], "pwsh");
}

#[test]
fn markdown_output_contains_check_table() {
    cmd()
        .args(["plan", "--runner-os", "linux", "--format", "markdown"])
        .assert()
        .success()
        .stdout(contains("| status | id | message |"))
        .stdout(contains("`shell.host.compat`"))
        .stdout(contains("classification `exact`"));
}
