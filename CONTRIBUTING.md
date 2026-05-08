# Contributing

Thank you for contributing to Forge Agent. This repository is a headless MCP-first writing agent, so changes should keep the protocol stable and the backend usable from a clean clone.

## Development Setup

1. Install the Rust stable toolchain.
2. Clone the repository.
3. Build the MCP server:

```powershell
cargo build -p forge-agent-mcp
```

4. Run the default test suite:

```powershell
cargo fmt --check
cargo check -p forge-agent-mcp
cargo test -p forge-agent-mcp
cargo test -p agent-harness-core
cargo test -p agent-writer --lib
```

## Quality Gates

Run clippy before opening a pull request:

```powershell
cargo clippy -p agent-harness-core --all-targets -- -D warnings
cargo clippy -p agent-writer --all-targets -- -D warnings
cargo clippy -p forge-agent-mcp --all-targets -- -D warnings
```

## MCP Contract Rules

- Keep `forge_backend_call` stable as the generic dispatch surface.
- Add a specific MCP tool when adding a backend action that should be discoverable.
- Keep tool annotations explicit. Do not infer write/read behavior from string fragments.
- Preserve the response envelope in `result.structuredContent`.
- Update `docs/CONTEXT_CONTRACT.md` when caller requirements, write safety, approval fields, or response shapes change.

## Data And Secrets

Do not commit:

- `.forge-agent-data/`
- `.env`
- provider API keys
- logs

## Pull Request Checklist

- The headless MCP build works in a clean clone.
- New or changed tools are covered by tests.
- Protocol-facing behavior is documented.
- Formatting and clippy checks pass.
