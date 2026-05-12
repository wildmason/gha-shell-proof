# Release Playbook

1. Run local gates:

   ```powershell
   cargo fmt --check
   cargo test --locked
   cargo clippy --locked --all-targets --all-features -- -D warnings
   cargo doc --locked --no-deps
   cargo package --locked
   cargo publish --dry-run --locked
   ```

2. Check the action manifest:

   ```powershell
   cargo run --manifest-path ..\action-proof\Cargo.toml -- --repo-root . --manifest action.yml --strict
   ```

3. Smoke the example workflow scan:

   ```powershell
   cargo run --locked -- check-workflow `
     --workflow examples\workflows\shell-proof.yml `
     --workspace examples `
     --format json `
     --output target\shell-proof.json
   ```

4. Smoke a render:

   ```powershell
   cargo run --locked -- render `
     --runner-os windows --shell pwsh `
     --script examples\scripts\hello.ps1 `
     --output-script target\rendered.ps1 `
     --format json
   ```

5. Commit, tag, and push:

   ```powershell
   git tag -a v1.0.0 -m "gha-shell-proof 1.0.0"
   git tag -f v1 v1.0.0
   git push origin main
   git push origin v1.0.0 v1
   ```

6. Publish:

   ```powershell
   cargo publish --locked
   ```

7. Create the GitHub Release and run `release-proof` against the public surfaces.
