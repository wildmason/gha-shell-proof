# Security

Report security issues privately to Wildmason.

`gha-shell-proof` reads workflow YAML, user-supplied script text, and CLI arguments. It does not execute workflow steps, fetch remote actions, evaluate untrusted shell, or call GitHub APIs. The `render` command writes the wrapped script body to the path given on the command line; it never executes the rendered file. Treat receipts as diagnostics, not as authorization decisions.
