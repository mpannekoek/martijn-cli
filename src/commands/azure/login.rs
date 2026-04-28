// Import Azure CLI helpers used by login and logout flows.
use crate::azure::service::{run_az_interactive_command, run_az_service_principal_login};
// Import the shared config loader so login can use defaults from `config.toml`.
use crate::config::load_app_config;
// Import `Uuid` so we can try parse the tenant identifier as a UUID for better error messages.
use uuid::Uuid;

// Import typed login arguments parsed by the CLI shape module.
use super::cli::LoginArguments;
// Import state helpers so login commands can refresh visible account status.
use super::state::{SessionState, refresh_and_print_status};

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
        // Track whether the CLI auto-selected this mode from config so we can explain it clearly.
        auto_detected: bool,
    },
}

// Run `az login`, report the outcome and refresh the visible state afterwards.
pub(super) async fn handle_login(state: &mut SessionState, arguments: &LoginArguments) {
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

    // Refresh and print the cached status so the CLI reflects the newest state.
    refresh_and_print_status(state).await;
}

// Run `az logout`, report the outcome and refresh the visible state afterwards.
pub(super) async fn handle_logout(state: &mut SessionState) {
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

    // Refresh and print the cached status so the CLI reflects the newest state.
    refresh_and_print_status(state).await;
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

    // Import the login argument type so tests can build parsed command inputs.
    use super::super::cli::LoginArguments;
    // Import the resolver helpers so the tests can validate login behavior.
    use super::{ResolvedLogin, resolve_login};

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

        // Confirm that the CLI falls back to normal interactive login with the shared tenant.
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
        // Use an empty config so the test only covers the explicit secret value.
        let config = AppConfig::default();

        // Try to resolve the login and capture the expected validation error.
        let error = resolve_login(&arguments, &config).expect_err("login should fail");

        // Confirm that the error explains the blank client secret clearly.
        assert!(error.contains("requires a client secret"));
    }
}
