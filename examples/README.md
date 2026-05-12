# Examples

`workflows/shell-proof.yml` exercises every built-in shell on its native runner OS, plus the `defaults.run` propagation chain. Use it as a `check-workflow` smoke target:

```powershell
cargo run --locked -- check-workflow `
  --workflow examples\workflows\shell-proof.yml `
  --workspace examples `
  --format markdown
```

`scripts/hello.ps1` and `scripts/hello.sh` are minimal user scripts to feed into `render`:

```powershell
cargo run --locked -- render `
  --runner-os windows --shell pwsh `
  --script examples\scripts\hello.ps1 `
  --output-script target\hello.rendered.ps1 `
  --format json
```
