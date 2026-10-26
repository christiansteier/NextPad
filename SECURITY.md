# Security Policy

## Supported versions

The latest released version receives security fixes.

## Reporting a vulnerability

Security-relevant issues are not reported through public issues. Use the platform's
private vulnerability reporting (on GitHub: "Report a vulnerability" in the Security tab),
or contact the maintainer directly.

A report should include a description of the issue, steps to reproduce, and the affected
version. Reports are acknowledged and addressed as quickly as the maintainer's capacity
allows.

## Scope

NextPad reproduces the Etherpad HTTP API and handles API keys and LTI platform secrets.
Keys and secrets are sensitive: they are never written to logs and are excluded from
version control by `.gitignore`. Security findings about running servers, hostnames,
version states and raw measurements are kept in a separate, permanently private
repository and must not appear here. Reports concerning accidental credential exposure or
authentication bypasses in this codebase are in scope.
