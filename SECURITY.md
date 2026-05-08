# Security Policy

## Reporting A Vulnerability

Report security issues privately to the repository owner. Do not open a public issue for vulnerabilities involving credentials, project data, prompt injection paths, arbitrary file access, or MCP tool misuse.

Include:

- Affected commit or release.
- Reproduction steps.
- Impact and expected attacker capability.
- Any relevant logs with secrets removed.

## Supported Versions

The `main` branch is the supported development line until tagged releases are published.

## Operational Guidance

- Treat `FORGE_AGENT_DATA_DIR` as private user data.
- Do not commit `.forge-agent-data/`, `.env`, logs, provider keys, or generated desktop artifacts.
- Prefer least-privilege MCP host configuration.
- Review write-capable MCP tools before allowing unattended scheduling.
- Require explicit approval metadata for provider spend and write-sensitive operations.

## MCP Tool Safety

Tool annotations are scheduling hints, not authorization. Clients must still follow `docs/CONTEXT_CONTRACT.md` for revision checks, dirty editor state, write approvals, and budget approvals.
