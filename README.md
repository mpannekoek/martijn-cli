# martijn-cli

`martijn-cli` is a small Rust CLI workspace with non-interactive commands and a tiny startup screen.

This repository is both a working tool and a learning project. The code is intentionally written in a clear, step-by-step style so it stays approachable for people who are learning Rust.

## Requirements

- Rust toolchain
- Azure CLI installed as `az` on Linux/macOS or `az.cmd` on Windows

## Build

```bash
cargo build
```

Run the binary directly:

```bash
cargo run
```

Running the binary without a command shows the temporary interactive start screen:

```text
MARTIJN CLI
Welcome to Martijn CLI. Ready when you are. 🚀
Run `martijn --help` to see available commands.
```

Run Azure tasks as normal CLI commands:

```bash
cargo run -- azure status
```

## Commands

The CLI currently has two command groups:

- `azure`: Azure login, logout, status, inventory, report, and snapshot commands
- `dummy`: minimal example commands used for learning and testing

Use `cargo run -- --help`, `cargo run -- azure --help`, or `cargo run -- dummy --help` to discover commands.

## Azure Commands

The main Azure commands are:

- `azure login [tenant]`
- `azure login --service-principal [--client-id <uuid>] [--client-secret <secret>] [tenant]`
- `azure logout`
- `azure status`
- `azure inventory resources list [--save [name]]`
- `azure inventory resources tree [--save [name]]`
- `azure inventory groups list [--save [name]]`
- `azure snapshot create resources`
- `azure snapshot create groups`
- `azure snapshot create all`
- `azure snapshot list`
- `azure snapshot delete <name>`
- `azure report list`
- `azure report show <name>`
- `azure report delete <name>`

### Inventory behavior

The inventory commands print human-readable output to the terminal by default:

```text
azure inventory resources list
azure inventory resources tree
azure inventory groups list
```

Add `--save` to also write a Markdown report with an automatic name:

```text
azure inventory resources list --save
```

Add `--save <name>` to choose a safe report name:

```text
azure inventory groups list --save daily-groups
```

Reports are saved below:

```text
~/.martijn/cli/inventory/resources/list/
~/.martijn/cli/inventory/resources/tree/
~/.martijn/cli/inventory/groups/list/
```

Use the report commands to manage saved inventory reports:

```text
azure report list
azure report show daily-groups
azure report delete daily-groups
```

### Snapshot behavior

The Azure commands can write JSON snapshots for resources, resource groups, or both:

```text
azure snapshot create resources
azure snapshot create groups
azure snapshot create all
```

Snapshots are saved below:

```text
~/.martijn/cli/snapshot/resources/
~/.martijn/cli/snapshot/groups/
```

On Windows this resolves through the user's home directory, for example:

```text
%USERPROFILE%\.martijn\cli\snapshot\resources\
%USERPROFILE%\.martijn\cli\snapshot\groups\
```

Use `azure snapshot list` and `azure snapshot delete <name>` to manage saved snapshots. Each snapshot entry contains normalized fields, a SHA-256 fingerprint of those normalized fields, and the original raw Azure JSON.

### Login behavior

The Azure command group supports two login modes through the same `login` command.

Interactive user login:

```text
azure login <tenant>
```

Service-principal login:

```text
azure login --service-principal --client-id <uuid> --client-secret <secret> <tenant>
```

Config-aware login:

- `azure login` can use default values from `~/.martijn/cli/config.toml`
- explicit CLI values always override config values
- bare `azure login` auto-detects service-principal mode only when a complete service-principal config is present
- otherwise bare `azure login` falls back to interactive user login with the configured tenant

## Config File

The CLI looks for configuration in:

```text
~/.martijn/cli/config.toml
```

Example:

```toml
[azure]
tenant = "00000000-0000-0000-0000-000000000000"

[azure.service_principal]
client_id = "11111111-1111-1111-1111-111111111111"
client_secret = "replace-this-with-a-real-secret"
```

How these values are used:

- `[azure].tenant` is the shared default tenant for both login modes
- `[azure.service_principal].client_id` is the default client ID for service-principal login
- `[azure.service_principal].client_secret` is the default client secret for service-principal login

Because the client secret is stored as plain text, treat this file as sensitive.

## Examples

Interactive login with an explicit tenant:

```text
azure login 00000000-0000-0000-0000-000000000000
```

Interactive login using the tenant from config:

```text
azure login
```

Service-principal login using only config defaults:

```text
azure login
```

Service-principal login with one CLI override:

```text
azure login --service-principal --client-id 22222222-2222-2222-2222-222222222222
```

## Verification

The repository uses these commands as the main verification steps:

```bash
cargo fmt
cargo build
cargo build --target x86_64-pc-windows-gnu
```
