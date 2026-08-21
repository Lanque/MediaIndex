# Security policy

MediaIndex is an evolving portfolio project and is not presented as a production service until the relevant hardening work is complete.

## Development rules

- Never commit cloud credentials, API keys, tokens, or real media metadata.
- Use local environment/configuration for development secrets.
- Use small synthetic or openly redistributable fixtures in tests.
- Treat client-provided paths, file types, and sizes as untrusted input.
- Keep original footage local by default.

## Planned cloud controls

The cloud phases must include authentication, object-level authorization, least-privilege IAM, short-lived signed URLs, secret storage through AWS Secrets Manager, input validation, structured logs, and cost/budget alarms.

## Reporting

For a private repository, report suspected vulnerabilities directly to the repository owner through a private GitHub channel rather than opening a public issue. Include reproduction steps, affected component, impact, and a suggested mitigation when known.
