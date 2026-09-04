# Backend Development Guidelines

> Best practices for backend development in this project.

---

## Overview

This directory contains guidelines for backend development. Fill in each file with your project's specific conventions.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module organization and file layout | To fill |
| [Database Guidelines](./database-guidelines.md) | ORM patterns, queries, migrations | To fill |
| [Error Handling](./error-handling.md) | Error types, handling strategies | To fill |
| [Quality Guidelines](./quality-guidelines.md) | Code standards, forbidden patterns | To fill |
| [Logging Guidelines](./logging-guidelines.md) | Structured logging, log levels | To fill |
| [File Workbench Contract](./file-workbench-contract.md) | Local/remote document identity, focus, search, and rich-text safety | Active |
| [Workbench Identity Contract](./workbench-identity-contract.md) | Stable worktree, pane, terminal-session, and incarnation routing | Active |
| [Worktree Context Contract](./worktree-context-contract.md) | Worktree-scoped Files/Git/Sessions state, exact Agent targets, and runtime diagnostics | Active |
| [Remote Runtime Reconciliation Contract](./remote-runtime-reconciliation-contract.md) | Project-scoped remote probing, stale-result fencing, and authoritative rebind | Active |
| [Remote Agent Reconciliation Contract](./remote-agent-reconciliation-contract.md) | Exact-route SSH agent probes, epoch fencing, and legacy projection | Active |
| [GitHub Project Tasks Contract](./github-project-tasks-contract.md) | Execution-host gh routing, repository/account fencing, and worktree-scoped read-only tasks | Active |
| [Global Agent Activity Contract](./global-agent-activity-contract.md) | Exact-run feed grouping, two-phase activation, acknowledgement, focus, and rollback | Active |
| [Release Staging Contract](./release-staging-contract.md) | Locked dual-workspace Actions builds, job-owned staging, PE validation, and installer payload proof | Active |
| [Project Onboarding Contract](./project-onboarding-contract.md) | Unified local/SSH folder, clone, Git initialization, registration, and stale-result safety | Active |

---

## How to Fill These Guidelines

For each guideline file:

1. Document your project's **actual conventions** (not ideals)
2. Include **code examples** from your codebase
3. List **forbidden patterns** and why
4. Add **common mistakes** your team has made

The goal is to help AI assistants and new team members understand how YOUR project works.

---

**Language**: All documentation should be written in **English**.
