# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.x     | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly.

**Do not open a public issue.**

Instead, email: **security@opencontextorchestrator.dev**

Or use [GitHub's private vulnerability reporting](https://github.com/open-context-orchestrator/oco/security/advisories/new).

### What to Include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### Response Timeline

- **Acknowledgment** within 48 hours
- **Assessment** within 7 days
- **Fix or mitigation** within 30 days for confirmed vulnerabilities

## Security Considerations

OCO executes shell commands and file operations as part of its tool runtime. Users should:

- Run OCO with least-privilege permissions
- Review `oco.toml` budget limits before deployment
- Never expose the MCP server to untrusted networks without authentication
- Use environment variables (not config files) for API keys
- Review tool execution policies before enabling in production
