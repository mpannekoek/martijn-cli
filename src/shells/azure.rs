// Import the Azure account model that this shell caches and displays.
use crate::azure::model::AzureAccount;
// Import the Azure service layer so this shell can delegate real Azure work.
use crate::azure::service::{
    fetch_azure_account, generate_inventory_report, run_az_interactive_command,
    run_az_service_principal_login,
};
// Import the shared config loader so login can use defaults from `config.toml`.
use crate::config::load_app_config;
// Import the shared application result type.
use crate::AppResult;
// Import the shared shell engine pieces used by this shell.
use crate::shells::engine::{self, CommandFuture, ShellAction};
// Import Clap helpers so this shell can describe its interactive command model.
use clap::{Args, Parser, Subcommand};
// Import terminal color helpers so the intro stands out.
use owo_colors::OwoColorize;
// Import `Uuid` so we can try parse the tenant identifier as a UUID for better error messages.
use uuid::Uuid;

// Keep the Azure shell's mutable state in one place.
#[derive(Debug, Default)]
struct SessionState {
    // Store the currently detected Azure account.
    // We use `Option` because the user may not be logged in.
    account: Option<AzureAccount>,
}

// Describe the argument shape for one Azure-shell command line.
#[derive(Parser, Debug)]
#[command(
    name = "azure",
    disable_help_flag = true,
    disable_help_subcommand = true
)]
struct AzureShellCli {
    // Store the one subcommand that the user typed in the Azure shell.
    #[command(subcommand)]
    command: AzureCommand,
}

// List the commands that the Azure shell understands.
#[derive(Subcommand, Debug)]
enum AzureCommand {
    /// Login to Azure CLI as a user or service principal.
    Login(LoginArguments),
    /// Run `az logout`.
    Logout,
    /// Show the current Azure login state.
    Status,
    /// Export a Markdown inventory of resources grouped by resource group.
    Inventory,
    /// Show the Azure shell help message.
    Help,
    /// Close the current shell session.
    #[command(alias = "quit")]
    Exit,
}

// Hold the arguments that belong to the `login` subcommand.
#[derive(Args, Debug, Clone, PartialEq, Eq)]
struct LoginArguments {
    // Switch to service-principal authentication instead of interactive user login.
    #[arg(long = "service-principal")]
    service_principal: bool,
    // Accept an optional client ID for service-principal login.
    #[arg(long = "client-id", requires = "service_principal")]
    client_id: Option<String>,
    // Accept an optional client secret for service-principal login.
    #[arg(long = "client-secret", requires = "service_principal")]
    client_secret: Option<String>,
    // Accept an optional tenant for both login modes.
    tenant: Option<String>,
}

// Represent the fully resolved login request after CLI arguments and config defaults are combined.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedLogin {
    // Store the tenant for interactive user login.
    InteractiveUser {
        // Keep the resolved tenant as an owned string because the command outlives the config borrow.
        tenant: String,
    },
    // Store all values needed for service-principal login.
    ServicePrincipal {
        // Keep the resolved tenant as an owned string because the command outlives the config borrow.
        tenant: String,
        // Keep the client ID as an owned string because the command outlives the config borrow.
        client_id: String,
        // Keep the client secret as an owned string because the command outlives the config borrow.
        client_secret: String,
        // Track whether the shell auto-selected this mode from config so we can explain it clearly.
        auto_detected: bool,
    },
}

// Start the Azure shell.
pub(crate) async fn run() -> AppResult<()> {
    // Create fresh state with no cached account information yet.
    let mut state = SessionState::default();
    // Populate the state once before the shell starts so the intro shows real status.
    refresh_session_state(&mut state).await;

    // Reuse the shared shell engine with the Azure-specific intro and handler.
    engine::run_shell(state, print_intro, handle_command, "azure").await
}

// Handle one tokenized command entered in the Azure shell.
fn handle_command<'a>(state: &'a mut SessionState, tokens: &'a [String]) -> CommandFuture<'a> {
    // Box the async block so it matches the shared `CommandFuture` type alias.
    Box::pin(async move {
        // Parse the shell tokens through Clap so commands and arguments stay typed.
        match parse_command(tokens) {
            Ok(AzureCommand::Help) => {
                // Print the Azure shell help text.
                engine::print_shell_help::<AzureShellCli>()?;
            }
            Ok(AzureCommand::Status) => {
                // Refresh the cached account information before showing it.
                refresh_and_print_status(state).await;
            }
            Ok(AzureCommand::Inventory) => {
                // Build the Markdown inventory report and save it to disk.
                handle_inventory(state).await;
            }
            Ok(AzureCommand::Login(arguments)) => {
                // Run the login flow after resolving CLI arguments and config defaults together.
                handle_login(state, &arguments).await;
            }
            Ok(AzureCommand::Logout) => {
                // Run the logout flow and then show the updated status.
                handle_logout(state).await;
            }
            Ok(AzureCommand::Exit) => {
                // Tell the user that the shell is about to close.
                println!("Closing shell.");
                // Ask the shared shell engine to stop the loop.
                return Ok(ShellAction::Exit);
            }
            Err(error) => {
                // Reuse the shared parse error printer so every shell responds consistently.
                engine::print_parse_error(error);
            }
        }

        // Keep the Azure shell open after every non-exit command.
        Ok(ShellAction::Continue)
    })
}

// Print the intro for the Azure shell.
fn print_intro(state: &SessionState) {
    // Identify the shell the user is currently in.
    println!("{}", "Interactive Azure shell".bright_cyan());
    // Point the user to the help command for discoverability.
    println!(
        "{}",
        "Type `help` to see available commands.".bright_yellow()
    );
    // Show the current login status immediately.
    print_status(state);
}

// Convert tokenized Azure-shell input into one typed command.
fn parse_command(tokens: &[String]) -> Result<AzureCommand, clap::Error> {
    // Reuse the shared helper so every shell performs the same Clap parsing steps.
    let cli = engine::parse_shell_command::<AzureShellCli>("azure", tokens)?;
    // Return only the subcommand because that is all the handler needs.
    Ok(cli.command)
}

// Print either the current Azure account or a message that no account is active.
fn print_status(state: &SessionState) {
    // Match on the optional account because the user may or may not be logged in.
    match &state.account {
        Some(account) => {
            // Show the user, subscription name and subscription identifier.
            println!(
                "Logged in as {} ({}) on subscription {}",
                account.user, account.name, account.subscription_id
            );
        }
        None => {
            // Explain clearly that no Azure login session was detected.
            println!("Not logged in to Azure CLI.");
        }
    }
}

// Run `az login`, report the outcome and refresh the visible state afterwards.
async fn handle_login(state: &mut SessionState, arguments: &LoginArguments) {
    // Load the optional config file first so CLI arguments can override those defaults cleanly.
    let config = match load_app_config() {
        Ok(config) => config,
        Err(error) => {
            // Stop early when the config exists but cannot be read or parsed correctly.
            println!("Unable to load `~/.martijn/cli/config.toml`: {error}");
            return;
        }
    };

    // Resolve the final login request by combining explicit CLI input with config defaults.
    let resolved_login = match resolve_login(arguments, &config) {
        Ok(resolved_login) => resolved_login,
        Err(error) => {
            // Stop early when required values are still missing or invalid after resolution.
            println!("{error}");
            return;
        }
    };

    // Run the Azure CLI command that matches the resolved login mode.
    match resolved_login {
        ResolvedLogin::InteractiveUser { tenant } => {
            // Run the existing interactive user login flow with the final tenant.
            run_interactive_user_login(&tenant).await;
        }
        ResolvedLogin::ServicePrincipal {
            tenant,
            client_id,
            client_secret,
            auto_detected,
        } => {
            // Explain the auto-detected mode so a bare `login` command is not surprising.
            if auto_detected {
                println!(
                    "Using service-principal login from `~/.martijn/cli/config.toml` defaults."
                );
            }

            // Run the service-principal login flow with the final resolved values.
            run_service_principal_login(&tenant, &client_id, &client_secret).await;
        }
    }

    // Refresh and print the cached status so the shell reflects the newest state.
    refresh_and_print_status(state).await;
}

// Run `az logout`, report the outcome and refresh the visible state afterwards.
async fn handle_logout(state: &mut SessionState) {
    // Run the Azure CLI logout command and inspect whether it succeeded.
    match run_az_interactive_command(&["logout"]).await {
        Ok(true) => {
            // Tell the user that Azure CLI reported a successful logout.
            println!("Logged out of Azure CLI.");
        }
        Ok(false) => {
            // Tell the user that the command ran but did not report success.
            println!("`az logout` did not complete successfully.");
        }
        Err(error) => {
            // Show the concrete error when the process could not even be started.
            println!("Unable to run `az logout`: {error}");
        }
    }

    // Refresh and print the cached status so the shell reflects the newest state.
    refresh_and_print_status(state).await;
}

// Build the Azure inventory report, save it as Markdown and tell the user where it lives.
async fn handle_inventory(state: &mut SessionState) {
    // Refresh the login state first so the command works with the latest Azure session.
    refresh_session_state(state).await;

    // Stop early when no Azure account is active.
    let Some(account) = state.account.as_ref() else {
        println!("Not logged in to Azure CLI. Run `login <tenant>` first.");
        return;
    };

    // Ask the service layer to build and write the inventory report.
    match generate_inventory_report(account).await {
        Ok(output_file_path) => {
            // Confirm success and show the final path to the newly created report.
            println!("Azure inventory saved to {}", output_file_path.display());
        }
        Err(error) => {
            // Explain clearly why the report could not be generated.
            println!("Unable to generate the Azure inventory report: {error}");
        }
    }
}

// Refresh the cached Azure account and immediately print the visible status.
async fn refresh_and_print_status(state: &mut SessionState) {
    // Update the in-memory account data first.
    refresh_session_state(state).await;
    // Print the new status after the refresh.
    print_status(state);
}

// Refresh the cached Azure account state by asking Azure CLI for the current account.
async fn refresh_session_state(state: &mut SessionState) {
    // Fetch the account data and handle both success and failure explicitly.
    match fetch_azure_account().await {
        Ok(account) => {
            // Replace the cached account with the freshly fetched value.
            state.account = account;
        }
        Err(error) => {
            // Clear the cached account when the status check itself failed.
            state.account = None;
            // Show the concrete error so the user understands why status is unavailable.
            println!("Azure status check failed: {error}");
        }
    }
}

// Validate whether the tenant string is a real UUID.
fn is_guid(value: &str) -> bool {
    // Ask the `uuid` crate to validate whether the text is a well-formed UUID.
    Uuid::try_parse(value).is_ok()
}

// Resolve the final login mode and values from CLI arguments plus config defaults.
fn resolve_login(
    arguments: &LoginArguments,
    config: &crate::config::AppConfig,
) -> Result<ResolvedLogin, String> {
    // Force service-principal mode when the explicit flag is present.
    if arguments.service_principal {
        // Resolve the tenant from CLI first and then from config.
        let tenant = resolve_string_value(arguments.tenant.as_deref(), config.azure_tenant())
            .ok_or_else(|| {
                String::from(
                    "Service-principal login requires a tenant. Provide `login --service-principal <tenant>` or set `[azure].tenant` in `~/.martijn/cli/config.toml`.",
                )
            })?;
        // Resolve the client ID from CLI first and then from config.
        let client_id = resolve_string_value(
            arguments.client_id.as_deref(),
            config.azure_service_principal_client_id(),
        )
        .ok_or_else(|| {
            String::from(
                "Service-principal login requires a client ID. Provide `--client-id` or set `[azure.service_principal].client_id` in `~/.martijn/cli/config.toml`.",
            )
        })?;
        // Resolve the client secret from CLI first and then from config.
        let client_secret = resolve_string_value(
            arguments.client_secret.as_deref(),
            config.azure_service_principal_client_secret(),
        )
        .ok_or_else(|| {
            String::from(
                "Service-principal login requires a client secret. Provide `--client-secret` or set `[azure.service_principal].client_secret` in `~/.martijn/cli/config.toml`.",
            )
        })?;

        // Validate the tenant before returning the resolved request.
        validate_tenant(&tenant)?;
        // Validate the client ID before returning the resolved request.
        validate_client_id(&client_id)?;
        // Validate the client secret before returning the resolved request.
        validate_client_secret(&client_secret)?;

        // Return the fully resolved service-principal login request.
        return Ok(ResolvedLogin::ServicePrincipal {
            tenant,
            client_id,
            client_secret,
            auto_detected: false,
        });
    }

    // Detect whether the user typed the bare `login` command with no extra values.
    let is_bare_login = arguments.tenant.is_none()
        && arguments.client_id.is_none()
        && arguments.client_secret.is_none();

    // Auto-select service-principal mode only for bare `login` when config has a full default set.
    if is_bare_login && config.has_complete_service_principal_defaults() {
        // Resolve the tenant from config because a complete SP config guarantees it exists.
        let tenant = config
            .azure_tenant()
            .expect("complete service-principal defaults should include a tenant")
            .to_owned();
        // Resolve the client ID from config because a complete SP config guarantees it exists.
        let client_id = config
            .azure_service_principal_client_id()
            .expect("complete service-principal defaults should include a client ID")
            .to_owned();
        // Resolve the client secret from config because a complete SP config guarantees it exists.
        let client_secret = config
            .azure_service_principal_client_secret()
            .expect("complete service-principal defaults should include a client secret")
            .to_owned();

        // Validate the tenant before returning the resolved request.
        validate_tenant(&tenant)?;
        // Validate the client ID before returning the resolved request.
        validate_client_id(&client_id)?;
        // Validate the client secret before returning the resolved request.
        validate_client_secret(&client_secret)?;

        // Return the fully resolved auto-detected service-principal login request.
        return Ok(ResolvedLogin::ServicePrincipal {
            tenant,
            client_id,
            client_secret,
            auto_detected: true,
        });
    }

    // Resolve the tenant for interactive user login from CLI first and then from config.
    let tenant = resolve_string_value(arguments.tenant.as_deref(), config.azure_tenant())
        .ok_or_else(|| {
            String::from(
                "Interactive Azure login requires a tenant. Provide `login <tenant>` or set `[azure].tenant` in `~/.martijn/cli/config.toml`.",
            )
        })?;

    // Validate the tenant before returning the resolved request.
    validate_tenant(&tenant)?;

    // Return the fully resolved interactive user login request.
    Ok(ResolvedLogin::InteractiveUser { tenant })
}

// Prefer the explicit CLI value and fall back to the config value only when needed.
fn resolve_string_value(cli_value: Option<&str>, config_value: Option<&str>) -> Option<String> {
    // Keep the CLI value when it exists and contains visible text.
    if let Some(cli_value) = cli_value.and_then(non_empty_string) {
        return Some(cli_value.to_owned());
    }

    // Otherwise fall back to the config value when it exists and contains visible text.
    config_value.and_then(non_empty_string).map(str::to_owned)
}

// Return only non-empty strings so blank values behave like missing values.
fn non_empty_string(value: &str) -> Option<&str> {
    // Remove surrounding whitespace before checking whether anything remains.
    let trimmed = value.trim();
    // Return `None` for blank strings so callers can treat them as absent.
    if trimmed.is_empty() {
        None
    } else {
        // Return the trimmed string slice so later validation sees clean input.
        Some(trimmed)
    }
}

// Validate that one tenant value is a real UUID.
fn validate_tenant(tenant: &str) -> Result<(), String> {
    // Reuse the UUID helper so all tenant validation stays consistent.
    if is_guid(tenant) {
        Ok(())
    } else {
        Err(String::from(
            "Invalid tenant ID format. Please provide a valid UUID.",
        ))
    }
}

// Validate that one client ID value is a real UUID.
fn validate_client_id(client_id: &str) -> Result<(), String> {
    // Reuse the UUID helper so client IDs are validated with the same rule as tenants.
    if is_guid(client_id) {
        Ok(())
    } else {
        Err(String::from(
            "Invalid client ID format. Please provide a valid UUID.",
        ))
    }
}

// Validate that one client secret contains visible text.
fn validate_client_secret(client_secret: &str) -> Result<(), String> {
    // Reject blank secrets because Azure CLI still needs a real secret value.
    if client_secret.trim().is_empty() {
        Err(String::from(
            "Invalid client secret. Please provide a non-empty value.",
        ))
    } else {
        // Report success when the secret contains visible text.
        Ok(())
    }
}

// Run the existing interactive user login flow with the already validated tenant.
async fn run_interactive_user_login(tenant: &str) {
    // Run the Azure CLI login command and inspect whether it succeeded.
    match run_az_interactive_command(&["login", "--tenant", tenant]).await {
        Ok(true) => {
            // Tell the user that Azure CLI reported a successful interactive login.
            println!("Interactive login completed.");
        }
        Ok(false) => {
            // Tell the user that the command ran but did not report success.
            println!("`az login` did not complete successfully.");
        }
        Err(error) => {
            // Show the concrete error when the process could not even be started.
            println!("Unable to run `az login`: {error}");
        }
    }
}

// Run service-principal login with already resolved and validated values.
async fn run_service_principal_login(tenant: &str, client_id: &str, client_secret: &str) {
    // Run the dedicated service-principal login helper and inspect whether it succeeded.
    match run_az_service_principal_login(tenant, client_id, client_secret).await {
        Ok(true) => {
            // Tell the user that Azure CLI reported a successful service-principal login.
            println!("Service-principal login completed.");
        }
        Ok(false) => {
            // Tell the user that the command ran but did not report success.
            println!("`az login --service-principal` did not complete successfully.");
        }
        Err(error) => {
            // Show the concrete error when the process could not even be started.
            println!("Unable to run service-principal `az login`: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    // Import the shared config types so tests can resolve login defaults explicitly.
    use crate::config::{AppConfig, AzureConfig, AzureServicePrincipalConfig};
    // Import the Azure parser and resolver helpers so the tests can validate command behavior.
    use super::{AzureCommand, LoginArguments, ResolvedLogin, parse_command, resolve_login};

    #[test]
    fn parses_login_with_one_tenant() {
        // Parse a valid login command with exactly one tenant argument.
        let parsed_command = parse_command(&[String::from("login"), String::from("my-tenant-id")])
            .expect("command should parse");

        // Confirm that Clap keeps the tenant value and returns the login variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Login(LoginArguments {
                tenant: Some(tenant),
                service_principal: false,
                client_id: None,
                client_secret: None,
            }) if tenant == "my-tenant-id"
        ));
    }

    #[test]
    fn parses_bare_login_without_arguments() {
        // Parse a bare login command that relies on later config resolution.
        let parsed_command = parse_command(&[String::from("login")]).expect("command should parse");

        // Confirm that Clap represents the missing values as `None`.
        assert!(matches!(
            parsed_command,
            AzureCommand::Login(LoginArguments {
                tenant: None,
                service_principal: false,
                client_id: None,
                client_secret: None,
            })
        ));
    }

    #[test]
    fn parses_service_principal_login_with_flags() {
        // Parse a service-principal login command with explicit flags.
        let parsed_command = parse_command(&[
            String::from("login"),
            String::from("--service-principal"),
            String::from("--client-id"),
            String::from("client-id"),
            String::from("--client-secret"),
            String::from("secret-value"),
            String::from("tenant-id"),
        ])
        .expect("command should parse");

        // Confirm that Clap keeps all service-principal values in the typed command.
        assert!(matches!(
            parsed_command,
            AzureCommand::Login(LoginArguments {
                tenant: Some(tenant),
                service_principal: true,
                client_id: Some(client_id),
                client_secret: Some(client_secret),
            }) if tenant == "tenant-id" && client_id == "client-id" && client_secret == "secret-value"
        ));
    }

    #[test]
    fn rejects_service_principal_fields_without_service_principal_flag() {
        // Parse a login command that provides a service-principal field without the mode flag.
        let error = parse_command(&[
            String::from("login"),
            String::from("--client-id"),
            String::from("client-id"),
        ])
        .expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap explains the missing required `--service-principal` flag.
        assert!(rendered_error.contains("--service-principal"));
    }

    #[test]
    fn rejects_login_with_extra_arguments() {
        // Parse a login command that provides more than one value after `login`.
        let error = parse_command(&[
            String::from("login"),
            String::from("tenant-one"),
            String::from("tenant-two"),
        ])
        .expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap reports the unexpected extra argument.
        assert!(rendered_error.contains("unexpected argument"));
    }

    #[test]
    fn bare_login_auto_detects_service_principal_from_complete_config() {
        // Build the parsed login arguments that represent a bare `login` command.
        let arguments = LoginArguments {
            service_principal: false,
            client_id: None,
            client_secret: None,
            tenant: None,
        };
        // Build a config that contains the full set of service-principal defaults.
        let config = AppConfig {
            azure: Some(AzureConfig {
                tenant: Some(String::from("00000000-0000-0000-0000-000000000000")),
                service_principal: Some(AzureServicePrincipalConfig {
                    client_id: Some(String::from("11111111-1111-1111-1111-111111111111")),
                    client_secret: Some(String::from("secret-value")),
                }),
            }),
        };

        // Resolve the final login request from the CLI arguments and config.
        let resolved_login = resolve_login(&arguments, &config).expect("login should resolve");

        // Confirm that bare `login` auto-selects service-principal mode from config.
        assert!(matches!(
            resolved_login,
            ResolvedLogin::ServicePrincipal {
                tenant,
                client_id,
                client_secret,
                auto_detected: true,
            } if tenant == "00000000-0000-0000-0000-000000000000"
                && client_id == "11111111-1111-1111-1111-111111111111"
                && client_secret == "secret-value"
        ));
    }

    #[test]
    fn bare_login_falls_back_to_interactive_user_login_when_sp_config_is_incomplete() {
        // Build the parsed login arguments that represent a bare `login` command.
        let arguments = LoginArguments {
            service_principal: false,
            client_id: None,
            client_secret: None,
            tenant: None,
        };
        // Build a config that contains only the shared tenant and no full SP defaults.
        let config = AppConfig {
            azure: Some(AzureConfig {
                tenant: Some(String::from("00000000-0000-0000-0000-000000000000")),
                service_principal: Some(AzureServicePrincipalConfig {
                    client_id: Some(String::from("11111111-1111-1111-1111-111111111111")),
                    client_secret: None,
                }),
            }),
        };

        // Resolve the final login request from the CLI arguments and config.
        let resolved_login = resolve_login(&arguments, &config).expect("login should resolve");

        // Confirm that the shell falls back to normal interactive login with the shared tenant.
        assert!(matches!(
            resolved_login,
            ResolvedLogin::InteractiveUser { tenant }
                if tenant == "00000000-0000-0000-0000-000000000000"
        ));
    }

    #[test]
    fn explicit_service_principal_flag_forces_service_principal_mode() {
        // Build parsed login arguments that explicitly request service-principal mode.
        let arguments = LoginArguments {
            service_principal: true,
            client_id: Some(String::from("11111111-1111-1111-1111-111111111111")),
            client_secret: Some(String::from("secret-value")),
            tenant: Some(String::from("00000000-0000-0000-0000-000000000000")),
        };
        // Use an otherwise empty config so the result only depends on the CLI input.
        let config = AppConfig::default();

        // Resolve the final login request from the explicit CLI values.
        let resolved_login = resolve_login(&arguments, &config).expect("login should resolve");

        // Confirm that the explicit mode flag always yields service-principal login.
        assert!(matches!(
            resolved_login,
            ResolvedLogin::ServicePrincipal {
                auto_detected: false,
                ..
            }
        ));
    }

    #[test]
    fn cli_values_override_config_defaults() {
        // Build parsed login arguments with an explicit tenant and client ID.
        let arguments = LoginArguments {
            service_principal: true,
            client_id: Some(String::from("22222222-2222-2222-2222-222222222222")),
            client_secret: None,
            tenant: Some(String::from("33333333-3333-3333-3333-333333333333")),
        };
        // Build a config that provides fallback values for all service-principal fields.
        let config = AppConfig {
            azure: Some(AzureConfig {
                tenant: Some(String::from("00000000-0000-0000-0000-000000000000")),
                service_principal: Some(AzureServicePrincipalConfig {
                    client_id: Some(String::from("11111111-1111-1111-1111-111111111111")),
                    client_secret: Some(String::from("secret-value")),
                }),
            }),
        };

        // Resolve the final login request from both CLI values and config defaults.
        let resolved_login = resolve_login(&arguments, &config).expect("login should resolve");

        // Confirm that explicit CLI values win over config defaults.
        assert!(matches!(
            resolved_login,
            ResolvedLogin::ServicePrincipal {
                tenant,
                client_id,
                client_secret,
                auto_detected: false,
            } if tenant == "33333333-3333-3333-3333-333333333333"
                && client_id == "22222222-2222-2222-2222-222222222222"
                && client_secret == "secret-value"
        ));
    }

    #[test]
    fn rejects_interactive_login_without_any_tenant_source() {
        // Build the parsed login arguments that represent a bare `login` command.
        let arguments = LoginArguments {
            service_principal: false,
            client_id: None,
            client_secret: None,
            tenant: None,
        };
        // Use an empty config so no tenant can be resolved from defaults either.
        let config = AppConfig::default();

        // Try to resolve the login and capture the expected validation error.
        let error = resolve_login(&arguments, &config).expect_err("login should fail");

        // Confirm that the error explains the missing tenant source clearly.
        assert!(error.contains("Interactive Azure login requires a tenant"));
    }

    #[test]
    fn rejects_invalid_tenant_uuid_in_both_modes() {
        // Build login arguments for interactive login with an invalid tenant value.
        let user_login_arguments = LoginArguments {
            service_principal: false,
            client_id: None,
            client_secret: None,
            tenant: Some(String::from("not-a-guid")),
        };
        // Build login arguments for service-principal login with the same invalid tenant value.
        let service_principal_arguments = LoginArguments {
            service_principal: true,
            client_id: Some(String::from("11111111-1111-1111-1111-111111111111")),
            client_secret: Some(String::from("secret-value")),
            tenant: Some(String::from("not-a-guid")),
        };
        // Use an empty config so the test only covers the explicit tenant values above.
        let config = AppConfig::default();

        // Confirm that the interactive login path rejects the invalid tenant.
        assert!(
            resolve_login(&user_login_arguments, &config)
                .expect_err("interactive login should fail")
                .contains("Invalid tenant ID format")
        );
        // Confirm that the service-principal path rejects the invalid tenant too.
        assert!(
            resolve_login(&service_principal_arguments, &config)
                .expect_err("service-principal login should fail")
                .contains("Invalid tenant ID format")
        );
    }

    #[test]
    fn rejects_invalid_client_id_uuid_for_service_principal_login() {
        // Build service-principal login arguments with an invalid client ID value.
        let arguments = LoginArguments {
            service_principal: true,
            client_id: Some(String::from("not-a-guid")),
            client_secret: Some(String::from("secret-value")),
            tenant: Some(String::from("00000000-0000-0000-0000-000000000000")),
        };
        // Use an empty config so the test only covers the explicit client ID value above.
        let config = AppConfig::default();

        // Try to resolve the login and capture the expected validation error.
        let error = resolve_login(&arguments, &config).expect_err("login should fail");

        // Confirm that the error explains the invalid client ID clearly.
        assert!(error.contains("Invalid client ID format"));
    }

    #[test]
    fn rejects_empty_client_secret_for_service_principal_login() {
        // Build service-principal login arguments with a blank client secret value.
        let arguments = LoginArguments {
            service_principal: true,
            client_id: Some(String::from("11111111-1111-1111-1111-111111111111")),
            client_secret: Some(String::from("   ")),
            tenant: Some(String::from("00000000-0000-0000-0000-000000000000")),
        };
        // Use an empty config so the test only covers the explicit secret value above.
        let config = AppConfig::default();

        // Try to resolve the login and capture the expected validation error.
        let error = resolve_login(&arguments, &config).expect_err("login should fail");

        // Confirm that the error explains the blank client secret clearly.
        assert!(error.contains("requires a client secret"));
    }

    #[test]
    fn parses_help_as_a_real_command() {
        // Parse the explicit help command that users can type inside the shell.
        let parsed_command = parse_command(&[String::from("help")]).expect("command should parse");

        // Confirm that help is represented as its own typed variant.
        assert!(matches!(parsed_command, AzureCommand::Help));
    }

    #[test]
    fn parses_inventory_as_a_real_command() {
        // Parse the explicit inventory command that users can type inside the shell.
        let parsed_command =
            parse_command(&[String::from("inventory")]).expect("command should parse");

        // Confirm that inventory is represented as its own typed variant.
        assert!(matches!(parsed_command, AzureCommand::Inventory));
    }

    #[test]
    fn rejects_inventory_with_extra_arguments() {
        // Parse an inventory command that should not accept any extra values.
        let error = parse_command(&[String::from("inventory"), String::from("unexpected")])
            .expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap reports the unexpected extra argument.
        assert!(rendered_error.contains("unexpected argument"));
    }
}
