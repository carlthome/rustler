# Crab Rustler: Copilot Cloud Agent Guide

`AGENTS.md` is the central source of truth for repository guidance. Read and follow it before starting work.

This file only records Copilot cloud-agent-specific onboarding details that are not maintained in `AGENTS.md`.

## Cloud environment notes

`.github/workflows/copilot-setup-steps.yml` provides Cargo, the Rust toolchain, and this
project's native build dependencies before a Copilot session starts. It also fetches
dependencies and verifies a build. Do not install Rust or system dependencies ad hoc.

Use the documented development environment and validation commands from `AGENTS.md`.
If the cloud environment lacks required system capabilities, report that separately from
code failures; do not alter project dependencies merely to work around an environment
limitation.
