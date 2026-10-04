---
title: Quickstart
order: 3
---

Get Graphite running on your repo in under a minute.

## Build

```bash
git clone https://github.com/daviguides/graphite.git
cd graphite
cargo build --release
```

Single binary output at `./target/release/graphite-cli`.

## Start the daemon

```bash
# From your project's root directory
./path/to/graphite-cli daemon
```

The daemon indexes your repo immediately via Tree-sitter, sets up a file watcher, and listens on a unix socket. Every subsequent command talks to this daemon.

## Core commands

```bash
# What will my current diff break?
graphite-cli diff-impact

# Blast radius of a specific symbol
graphite-cli blast my_function

# Symbol context: source, callers, callees, tests
graphite-cli context MyClass

# Search symbols by name
graphite-cli search "handler"

# Grep with graph annotations
graphite-cli grep "authenticate"
```

## Install interception hooks

For Claude Code, install hooks that transparently enrich `grep`/`find`/`cat` with graph context:

```bash
graphite-cli hooks install
```

This registers PreToolUse hooks in your repo's `.claude/settings.json`. The agent's existing commands get graph-aware answers without any workflow change.

## Output formats

- **Default**: text format optimized for AI agent consumption
- **`--json`**: structured JSON for programs and integrations
- **`--human`**: grouped and colored for terminal reading

## Without daemon (probe mode)

Every command works without a running daemon. It probes the filesystem, incrementally syncs changed files, then queries. Slower per invocation, but fine for CI or one-off use.

```bash
# No daemon needed
graphite-cli blast my_function
```

## Supported languages

| Language | Status |
|----------|--------|
| Python | v1.0 (current) |
| Rust | v1.1 (next) |
| TypeScript/JavaScript | v1.1 |
