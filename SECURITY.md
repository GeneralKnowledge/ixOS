# Security policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | yes       |

## Reporting

If you discover a security issue, please open a private report via your GitHub security advisory workflow for this repository, or contact the maintainers directly. Do not file public issues for undisclosed vulnerabilities.

## Scope

- Rust FFI boundary (UniFFI), CLI subprocess invocation, and persistence file handling are in scope.
- Apple signing credentials are managed by xtool locally — never commit API keys, passwords, or provisioning profiles.

## Expectations

- Do not store secrets in `forge.toml` or the repo.
- JSON persistence is not suitable for credentials; use future platform secure-storage adapters.
