use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum RunnerOs {
    Linux,
    Macos,
    Windows,
}

impl RunnerOs {
    pub fn as_str(self) -> &'static str {
        match self {
            RunnerOs::Linux => "linux",
            RunnerOs::Macos => "macos",
            RunnerOs::Windows => "windows",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuiltinShell {
    Bash,
    Sh,
    Pwsh,
    Powershell,
    Cmd,
    Python,
}

impl BuiltinShell {
    pub fn as_str(self) -> &'static str {
        match self {
            BuiltinShell::Bash => "bash",
            BuiltinShell::Sh => "sh",
            BuiltinShell::Pwsh => "pwsh",
            BuiltinShell::Powershell => "powershell",
            BuiltinShell::Cmd => "cmd",
            BuiltinShell::Python => "python",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "bash" => Some(BuiltinShell::Bash),
            "sh" => Some(BuiltinShell::Sh),
            "pwsh" => Some(BuiltinShell::Pwsh),
            "powershell" => Some(BuiltinShell::Powershell),
            "cmd" => Some(BuiltinShell::Cmd),
            "python" => Some(BuiltinShell::Python),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellSource {
    Step,
    JobDefaultsRun,
    WorkflowDefaultsRun,
    RunnerDefault,
}

impl ShellSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ShellSource::Step => "step",
            ShellSource::JobDefaultsRun => "job-defaults-run",
            ShellSource::WorkflowDefaultsRun => "workflow-defaults-run",
            ShellSource::RunnerDefault => "runner-default",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkdirSource {
    Step,
    JobDefaultsRun,
    WorkflowDefaultsRun,
    Workspace,
}

impl WorkdirSource {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkdirSource::Step => "step",
            WorkdirSource::JobDefaultsRun => "job-defaults-run",
            WorkdirSource::WorkflowDefaultsRun => "workflow-defaults-run",
            WorkdirSource::Workspace => "workspace",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Classification {
    Exact,
    Compatible,
    Simulated,
    Unsupported,
}

impl Classification {
    pub fn as_str(self) -> &'static str {
        match self {
            Classification::Exact => "exact",
            Classification::Compatible => "compatible",
            Classification::Simulated => "simulated",
            Classification::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Passed,
    Warning,
    Failed,
    Skipped,
}

impl CheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Passed => "passed",
            CheckStatus::Warning => "warning",
            CheckStatus::Failed => "failed",
            CheckStatus::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "lf",
            LineEnding::Crlf => "crlf",
        }
    }

    pub fn literal(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "utf-8-no-bom")]
    Utf8NoBom,
}

impl Encoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf8NoBom => "utf-8-no-bom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ShellSpec {
    Builtin {
        name: String,
    },
    Custom {
        command: String,
        args: String,
        template: String,
    },
}

impl ShellSpec {
    pub fn name(&self) -> &str {
        match self {
            ShellSpec::Builtin { name } => name,
            ShellSpec::Custom { command, .. } => command,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedShell {
    pub spec: ShellSpec,
    pub source: ShellSource,
    pub builtin: bool,
    pub command: String,
    pub args_format: String,
    pub extension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedWorkdir {
    pub source: WorkdirSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested: Option<String>,
    pub workspace: String,
    pub resolved: String,
    pub absolute: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptPlan {
    pub extension: String,
    pub line_ending: LineEnding,
    pub encoding: Encoding,
    pub temp_filename_pattern: String,
    pub script_path: String,
    pub prologue: Vec<String>,
    pub epilogue: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invocation {
    pub command: String,
    pub args_format: String,
    pub argv: Vec<String>,
    pub working_directory: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailFast {
    pub flags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_action_preference: Option<String>,
    pub propagates_lastexitcode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub runner_os: RunnerOs,
    pub shell: ResolvedShell,
    pub working_directory: ResolvedWorkdir,
    pub script: ScriptPlan,
    pub invocation: Invocation,
    pub fail_fast: FailFast,
    pub classification: Classification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub id: String,
    pub status: CheckStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Check {
    pub fn passed(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: CheckStatus::Passed,
            message: message.into(),
            classification: None,
            detail: None,
        }
    }

    pub fn warning(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: CheckStatus::Warning,
            message: message.into(),
            classification: None,
            detail: None,
        }
    }

    pub fn failed(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: CheckStatus::Failed,
            message: message.into(),
            classification: None,
            detail: None,
        }
    }

    pub fn skipped(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: CheckStatus::Skipped,
            message: message.into(),
            classification: None,
            detail: None,
        }
    }

    pub fn with_classification(mut self, classification: Classification) -> Self {
        self.classification = Some(classification);
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub passed: u32,
    pub warnings: u32,
    pub failed: u32,
    pub skipped: u32,
}

impl Summary {
    pub fn record(&mut self, status: CheckStatus) {
        match status {
            CheckStatus::Passed => self.passed += 1,
            CheckStatus::Warning => self.warnings += 1,
            CheckStatus::Failed => self.failed += 1,
            CheckStatus::Skipped => self.skipped += 1,
        }
    }

    pub fn extend(&mut self, other: &Summary) {
        self.passed += other.passed;
        self.warnings += other.warnings;
        self.failed += other.failed;
        self.skipped += other.skipped;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_name: Option<String>,
    pub plan: Plan,
    pub checks: Vec<Check>,
    pub summary: Summary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_script: Option<RenderedScript>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedScript {
    pub path: String,
    pub line_ending: LineEnding,
    pub encoding: Encoding,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStamp {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub tool: ToolStamp,
    pub generated_at: DateTime<Utc>,
    pub mode: String,
    pub plans: Vec<PlanRecord>,
    pub checks: Vec<Check>,
    pub summary: Summary,
}
