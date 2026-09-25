# Security policy

## Reporting a vulnerability

Report security issues **privately, through GitHub's private vulnerability
reporting** for this repository: open the repository's **Security** tab and
choose **Report a vulnerability**. That is the only reporting channel; there
is no security email address.

Please do not open a public issue, pull request or discussion for a suspected
vulnerability.

Private vulnerability reporting is a repository setting the repository owner
enables. If the **Report a vulnerability** button is not shown, the setting is
not yet on; do not fall back to a public channel.

## What is in scope

This repository's code: the crates under `crates/`, `backends/`,
`integrations/` and `tools/`, and the `rustev` command-line tool. A finding in
spec-spine, Aicortex, Rahi or a remote model provider belongs to that
project's own policy; a finding in how Rustev handles a provider's key,
request routing or responses is in scope here.

## Supported versions

Nothing is released yet, so reports are judged against the current `main`.
