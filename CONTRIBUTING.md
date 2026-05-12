# Contributing

Run the local gates before sending changes:

```powershell
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo doc --locked --no-deps
```

Keep the shell-handler behavior anchored to the open-source `actions/runner` implementation. New shells, prologue/epilogue lines, file extensions, or argument-format strings must cite the corresponding code in `src/Runner.Worker/Handlers/` and ship with a unit test.

If a rule depends on workflow YAML structure beyond `run:` steps and `defaults.run`, prefer using `gha-workflow-proof` rather than expanding the scanner here. If a rule depends on expression evaluation, use `gha-expression-proof`. The proof tools compose; nobody owns more than one boundary.
