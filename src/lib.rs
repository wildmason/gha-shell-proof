//! GitHub Actions `run:`-step shell planner and compatibility checker for
//! offline CI.
//!
//! `gha-shell-proof` mirrors the open-source `actions/runner` script handler
//! to deterministically resolve which shell, script extension, prologue,
//! epilogue, line endings, and argv a step would actually use on a given
//! runner OS, and emits text/JSON/Markdown receipts that other tools (such
//! as `ci-forge`) can attach to step provenance.
//!
//! # Quick start
//!
//! ```no_run
//! use camino::Utf8PathBuf;
//! use gha_shell_proof::{PlanInputs, RunnerOs, StepInputs, make_plan, render_receipt, OutputFormat};
//! use gha_shell_proof::{Receipt, Summary, ToolStamp, PlanRecord, summarize, SCHEMA_VERSION, TOOL_NAME, TOOL_VERSION};
//! use chrono::Utc;
//!
//! let inputs = PlanInputs {
//!     runner_os: RunnerOs::Linux,
//!     workspace: Utf8PathBuf::from("/work/repo"),
//!     temp_dir: None,
//!     script_path: None,
//!     step: StepInputs::default(),
//! };
//! let (plan, checks) = make_plan(&inputs).unwrap();
//! let summary = summarize(&checks);
//! let receipt = Receipt {
//!     schema_version: SCHEMA_VERSION,
//!     tool: ToolStamp { name: TOOL_NAME.into(), version: TOOL_VERSION.into() },
//!     generated_at: Utc::now(),
//!     mode: "plan".into(),
//!     plans: vec![PlanRecord {
//!         workflow: None, job: None, step_index: None, step_id: None, step_name: None,
//!         plan, checks, summary, rendered_script: None,
//!     }],
//!     checks: Vec::new(),
//!     summary: Summary::default(),
//! };
//! let _ = render_receipt(&receipt, OutputFormat::Json).unwrap();
//! ```

pub mod engine;
pub mod model;
pub mod render;
pub mod workflow;

pub use engine::{
    PlanInputs, SCRIPT_PLACEHOLDER_ID, StepInputs, builtin_args_format, builtin_extension,
    default_shell, default_shell_fallback, default_temp_dir, encoding_for, has_blocking_failure,
    line_ending_for, make_plan, normalize_line_endings, parse_custom_shell, summarize, wrap_script,
};
pub use model::{
    BuiltinShell, Check, CheckStatus, Classification, Encoding, FailFast, Invocation, LineEnding,
    Plan, PlanRecord, Receipt, RenderedScript, ResolvedShell, ResolvedWorkdir, RunnerOs,
    SCHEMA_VERSION, ScriptPlan, ShellSource, ShellSpec, Summary, ToolStamp, WorkdirSource,
};
pub use render::{OutputFormat, render_receipt};
pub use workflow::{
    WorkflowScanOptions, records_have_failures, records_have_warnings, scan_workflow,
};

pub const TOOL_NAME: &str = "gha-shell-proof";
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
