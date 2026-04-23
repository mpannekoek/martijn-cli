// Import `Deserialize` so TOML text can become typed Rust structs.
use serde::Deserialize;
// Import filesystem helpers so we can read the optional config file.
use std::fs;
// Import `Path` and `PathBuf` so paths stay platform-aware on every operating system.
use std::path::{Path, PathBuf};

// Import the shared application result type used across the CLI.
use crate::AppResult;

// Hold the full CLI configuration file in one typed root struct.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct AppConfig {
    // Store the optional Azure-specific settings under the `[azure]` table.
    #[serde(default)]
    pub(crate) azure: Option<AzureConfig>,
}

// Hold the settings that belong to the top-level `[azure]` table.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct AzureConfig {
    // Store the shared default tenant that both login modes can reuse.
    #[serde(default)]
    pub(crate) tenant: Option<String>,
    // Store the nested service-principal settings from `[azure.service_principal]`.
    #[serde(default)]
    pub(crate) service_principal: Option<AzureServicePrincipalConfig>,
}

// Hold the values that belong to `[azure.service_principal]`.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct AzureServicePrincipalConfig {
    // Store the default application identifier for service-principal login.
    #[serde(default)]
    pub(crate) client_id: Option<String>,
    // Store the default client secret for service-principal login.
    #[serde(default)]
    pub(crate) client_secret: Option<String>,
}

impl AppConfig {
    // Return the configured Azure tenant when it exists and is not empty.
    pub(crate) fn azure_tenant(&self) -> Option<&str> {
        // Walk into the optional Azure section and ignore blank strings.
        self.azure
            .as_ref()
            .and_then(|azure| non_empty_value(azure.tenant.as_deref()))
    }

    // Return the configured service-principal client ID when it exists and is not empty.
    pub(crate) fn azure_service_principal_client_id(&self) -> Option<&str> {
        // Walk into the nested Azure service-principal section and ignore blank strings.
        self.azure
            .as_ref()
            .and_then(|azure| azure.service_principal.as_ref())
            .and_then(|service_principal| non_empty_value(service_principal.client_id.as_deref()))
    }

    // Return the configured service-principal client secret when it exists and is not empty.
    pub(crate) fn azure_service_principal_client_secret(&self) -> Option<&str> {
        // Walk into the nested Azure service-principal section and ignore blank strings.
        self.azure
            .as_ref()
            .and_then(|azure| azure.service_principal.as_ref())
            .and_then(|service_principal| {
                non_empty_value(service_principal.client_secret.as_deref())
            })
    }

    // Report whether the config contains a complete service-principal default set.
    pub(crate) fn has_complete_service_principal_defaults(&self) -> bool {
        // Require the shared tenant, the client ID and the client secret to all be present.
        self.azure_tenant().is_some()
            && self.azure_service_principal_client_id().is_some()
            && self.azure_service_principal_client_secret().is_some()
    }
}

// Load the optional CLI config file from the standard location in the user's home directory.
pub(crate) fn load_app_config() -> AppResult<AppConfig> {
    // Resolve the final config path before reading anything from disk.
    let config_path = resolve_cli_config_path()?;
    // Delegate to the path-based helper so tests can reuse the same logic.
    load_app_config_from_path(&config_path)
}

// Load the CLI config from one concrete file path.
pub(crate) fn load_app_config_from_path(config_path: &Path) -> AppResult<AppConfig> {
    // Treat a missing file as "no config yet" so first-time setup stays smooth.
    if !config_path.exists() {
        return Ok(AppConfig::default());
    }

    // Read the whole file into memory because the config is expected to stay small.
    let raw_config = fs::read_to_string(config_path).map_err(|error| {
        format!(
            "unable to read the config file `{}`: {error}",
            config_path.display()
        )
    })?;

    // Parse the TOML text into the typed config struct.
    let parsed_config: AppConfig = toml::from_str(&raw_config).map_err(|error| {
        format!(
            "unable to parse the config file `{}` as TOML: {error}",
            config_path.display()
        )
    })?;

    // Return the successfully parsed config.
    Ok(parsed_config)
}

// Resolve the standard CLI config path `~/.martijn/cli/config.toml`.
pub(crate) fn resolve_cli_config_path() -> AppResult<PathBuf> {
    // Resolve the shared CLI directory first so all features use the same base path.
    let cli_directory = resolve_cli_directory()?;
    // Append the fixed config filename in a platform-aware way.
    Ok(cli_directory.join("config.toml"))
}

// Resolve the shared CLI directory `~/.martijn/cli`.
pub(crate) fn resolve_cli_directory() -> AppResult<PathBuf> {
    // Resolve the home directory before joining the fixed application-specific segments.
    let home_directory = resolve_home_directory()?;
    // Build the final directory in a platform-aware way.
    Ok(build_cli_directory_from_home(&home_directory))
}

// Append the fixed `.martijn/cli` directory under a known home directory.
pub(crate) fn build_cli_directory_from_home(home_directory: &Path) -> PathBuf {
    // Join path segments one by one so separators stay correct on every platform.
    home_directory.join(".martijn").join("cli")
}

// Resolve the user's home directory in a way that works on Unix-like systems and Windows.
fn resolve_home_directory() -> AppResult<PathBuf> {
    // Check the Unix-style `HOME` variable first because it is the standard on Unix-like systems.
    if let Some(home) = std::env::var_os("HOME") {
        // Convert the environment value into a platform-aware path.
        let home_directory = PathBuf::from(home);
        // Ignore empty values because they are not useful filesystem paths.
        if !home_directory.as_os_str().is_empty() {
            return Ok(home_directory);
        }
    }

    // Check the Windows-style `USERPROFILE` variable next.
    if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        // Convert the environment value into a platform-aware path.
        let home_directory = PathBuf::from(user_profile);
        // Ignore empty values because they are not useful filesystem paths.
        if !home_directory.as_os_str().is_empty() {
            return Ok(home_directory);
        }
    }

    // Report a clear error when neither home-directory variable is available.
    Err("could not determine the user's home directory from HOME or USERPROFILE".into())
}

// Return only non-empty string values so blank config entries behave like missing values.
fn non_empty_value(value: Option<&str>) -> Option<&str> {
    // Remove surrounding whitespace before deciding whether the value is usable.
    value.and_then(|value| {
        // Keep the trimmed value only when it still contains visible text.
        let trimmed = value.trim();
        // Return `None` for blank strings so callers can treat them as missing.
        if trimmed.is_empty() {
            None
        } else {
            // Return the trimmed string slice so later validation sees the clean value.
            Some(trimmed)
        }
    })
}

#[cfg(test)]
mod tests {
    // Import the helpers under test from the parent module.
    use super::{
        AppConfig, build_cli_directory_from_home, load_app_config_from_path,
        resolve_cli_config_path,
    };
    // Import standard filesystem and path helpers used in the tests.
    use std::fs;
    use std::path::PathBuf;
    // Import `Uuid` so each temporary test path stays unique.
    use uuid::Uuid;

    #[test]
    fn builds_cli_directory_under_home() {
        // Build a fake Unix-style home path for the path helper.
        let home_directory = PathBuf::from("/home/martijn");
        // Build the CLI directory from that home path.
        let cli_directory = build_cli_directory_from_home(&home_directory);

        // Confirm that the helper appends `.martijn/cli` in order.
        assert_eq!(cli_directory, home_directory.join(".martijn").join("cli"));
    }

    #[test]
    fn missing_config_file_returns_default_config() {
        // Build a unique path that does not exist on disk.
        let missing_path = std::env::temp_dir()
            .join(format!("martijn-config-missing-{}", Uuid::new_v4()))
            .join("config.toml");
        // Load the config from that missing location.
        let config = load_app_config_from_path(&missing_path).expect("config should load");

        // Confirm that a missing file behaves like an empty config.
        assert_eq!(config.azure_tenant(), None);
        // Confirm that the missing file does not produce service-principal defaults either.
        assert!(!config.has_complete_service_principal_defaults());
    }

    #[test]
    fn parses_valid_azure_config() {
        // Create a unique temporary directory for this test's config file.
        let temporary_directory =
            std::env::temp_dir().join(format!("martijn-config-{}", Uuid::new_v4()));
        // Create the directory tree before writing the config file.
        fs::create_dir_all(&temporary_directory).expect("temporary directory should be created");
        // Build the full config file path inside the temporary directory.
        let config_path = temporary_directory.join("config.toml");
        // Write a small but valid Azure config file to disk.
        fs::write(
            &config_path,
            "[azure]\ntenant = \"tenant-id\"\n\n[azure.service_principal]\nclient_id = \"client-id\"\nclient_secret = \"secret-value\"\n",
        )
        .expect("config file should be written");

        // Load the config from the file we just created.
        let config = load_app_config_from_path(&config_path).expect("config should parse");

        // Confirm that the top-level Azure tenant was parsed correctly.
        assert_eq!(config.azure_tenant(), Some("tenant-id"));
        // Confirm that the nested service-principal client ID was parsed correctly.
        assert_eq!(
            config.azure_service_principal_client_id(),
            Some("client-id")
        );
        // Confirm that the nested service-principal client secret was parsed correctly.
        assert_eq!(
            config.azure_service_principal_client_secret(),
            Some("secret-value")
        );
        // Confirm that the config now counts as a complete SP default set.
        assert!(config.has_complete_service_principal_defaults());

        // Remove the temporary directory tree so the test leaves no clutter behind.
        fs::remove_dir_all(&temporary_directory).expect("temporary directory should be removed");
    }

    #[test]
    fn rejects_invalid_toml_config() {
        // Create a unique temporary directory for this test's malformed config file.
        let temporary_directory =
            std::env::temp_dir().join(format!("martijn-config-invalid-{}", Uuid::new_v4()));
        // Create the directory tree before writing the malformed config file.
        fs::create_dir_all(&temporary_directory).expect("temporary directory should be created");
        // Build the full config file path inside the temporary directory.
        let config_path = temporary_directory.join("config.toml");
        // Write malformed TOML so the parser has something concrete to reject.
        fs::write(&config_path, "[azure\ntenant = \"broken\"").expect("config file should exist");

        // Try to load the malformed config and capture the resulting error.
        let error =
            load_app_config_from_path(&config_path).expect_err("config parsing should fail");
        // Render the error so we can inspect the human-readable message.
        let rendered_error = error.to_string();

        // Confirm that the error message points to TOML parsing clearly.
        assert!(rendered_error.contains("unable to parse the config file"));

        // Remove the temporary directory tree so the test leaves no clutter behind.
        fs::remove_dir_all(&temporary_directory).expect("temporary directory should be removed");
    }

    #[test]
    fn default_config_reports_no_values() {
        // Create the all-empty default config value.
        let config = AppConfig::default();

        // Confirm that no Azure tenant exists by default.
        assert_eq!(config.azure_tenant(), None);
        // Confirm that no complete service-principal defaults exist by default.
        assert!(!config.has_complete_service_principal_defaults());
    }

    #[test]
    fn resolves_cli_config_path_under_home_variables() {
        // Store the original `HOME` variable so the test can restore it afterwards.
        let original_home = std::env::var_os("HOME");
        // Store the original `USERPROFILE` variable so the test can restore it afterwards.
        let original_user_profile = std::env::var_os("USERPROFILE");
        // Set `HOME` to a predictable fake path for this test.
        unsafe { std::env::set_var("HOME", "/tmp/martijn-home") };
        // Remove `USERPROFILE` so the helper definitely prefers `HOME`.
        unsafe { std::env::remove_var("USERPROFILE") };

        // Resolve the config path with the temporary environment in place.
        let config_path = resolve_cli_config_path().expect("config path should resolve");

        // Confirm that the helper appends `.martijn/cli/config.toml` under `HOME`.
        assert_eq!(
            config_path,
            PathBuf::from("/tmp/martijn-home")
                .join(".martijn")
                .join("cli")
                .join("config.toml")
        );

        // Restore the original `HOME` variable after the assertion.
        match original_home {
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            None => unsafe { std::env::remove_var("HOME") },
        }

        // Restore the original `USERPROFILE` variable after the assertion.
        match original_user_profile {
            Some(value) => unsafe { std::env::set_var("USERPROFILE", value) },
            None => unsafe { std::env::remove_var("USERPROFILE") },
        }
    }
}
