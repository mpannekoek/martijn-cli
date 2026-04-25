# martijn-cli

`martijn-cli` is a small Rust CLI workspace with interactive shells.

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

Or start the Azure shell immediately:

```bash
cargo run -- azure
```

## Shells

The CLI currently has two interactive shells:

- `azure`: Azure login, logout, status, inventory, and snapshot commands
- `dummy`: a minimal example shell used for learning and testing

If you start the CLI without a subcommand, it opens the root shell. From there you can type `azure`, `dummy`, or `help`.

## Azure Shell

Inside the Azure shell, the main commands are:

- `login [tenant]`
- `login --service-principal [--client-id <uuid>] [--client-secret <secret>] [tenant]`
- `logout`
- `status`
- `inventory`
- `snapshot`
- `help`
- `exit`

### Snapshot behavior

The Azure shell can write a JSON snapshot of all resources in the active subscription:

```text
snapshot generate
```

Snapshots are saved below:

```text
~/.martijn/cli/snapshot/
```

On Windows this resolves through the user's home directory, for example:

```text
%USERPROFILE%\.martijn\cli\snapshot\
```

Each resource entry contains normalized fields, a SHA-256 fingerprint of those normalized fields, and the original raw Azure JSON.

### Login behavior

The Azure shell supports two login modes through the same `login` command.

Interactive user login:

```text
login <tenant>
```

Service-principal login:

```text
login --service-principal --client-id <uuid> --client-secret <secret> <tenant>
```

Config-aware login:

- `login` can use default values from `~/.martijn/cli/config.toml`
- explicit CLI values always override config values
- bare `login` auto-detects service-principal mode only when a complete service-principal config is present
- otherwise bare `login` falls back to interactive user login with the configured tenant

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
login 00000000-0000-0000-0000-000000000000
```

Interactive login using the tenant from config:

```text
login
```

Service-principal login using only config defaults:

```text
login
```

Service-principal login with one CLI override:

```text
login --service-principal --client-id 22222222-2222-2222-2222-222222222222
```

## Verification

The repository uses these commands as the main verification steps:

```bash
cargo fmt
cargo build
cargo build --target x86_64-pc-windows-gnu
```
