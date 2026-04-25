// Import the Azure data model types shared by the shell and report layers.
use crate::azure::model::{
    AzureAccount, AzureInventoryGroup, AzureResourceGroupReportItem, AzureResourceReportItem,
};
// Import the Azure report helpers used to sort, render and locate output files.
use crate::azure::report::{
    build_inventory_file_name, count_total_resources, render_inventory_markdown,
    resolve_inventory_output_directory, sort_resource_groups, sort_resources,
};
// Import the Azure snapshot helpers used to normalize, render and locate JSON snapshots.
use crate::azure::snapshot::{
    build_snapshot_envelope, build_snapshot_file_name, resolve_snapshot_output_directory,
};
// Import the shared application result type.
use crate::AppResult;
// Import filesystem helpers so we can create directories and write files.
use std::fs;
// Import `Path` and `PathBuf` because this service reads and writes report files.
use std::path::{Path, PathBuf};
// Import `Stdio` so we can control how spawned processes use standard streams.
use std::process::Stdio;
// Import `SystemTime` so inventory listings can expose filesystem modification times.
use std::time::SystemTime;
// Import Tokio's async `Command` so Azure CLI calls work inside async code.
use tokio::process::Command;

// Describe one saved Azure inventory Markdown report on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InventoryReportFile {
    // Store the plain file name so shell output can stay compact.
    pub(crate) file_name: String,
    // Store the full path so callers can still locate the file exactly.
    pub(crate) path: PathBuf,
    // Store the optional modified time because some filesystems may not provide it.
    pub(crate) modified_at: Option<SystemTime>,
    // Store the file size in bytes so users can quickly compare reports.
    pub(crate) size_bytes: u64,
}

// Ask Azure CLI for the active account and convert the output into structured data.
pub(crate) async fn fetch_azure_account() -> AppResult<Option<AzureAccount>> {
    // Build and run `az account show` with a query that only returns the fields we need.
    let output = azure_cli_command()
        .args([
            "account",
            "show",
            "--query",
            "[name, id, user.name]",
            "--output",
            "tsv",
        ])
        // Capture standard output because we need to parse the command result.
        .stdout(Stdio::piped())
        // Hide standard error because a failing status is enough to mean "not logged in".
        .stderr(Stdio::null())
        // Spawn the child process and wait for it to finish.
        .output()
        .await
        // Convert process startup errors into a readable application error.
        .map_err(|error| format!("`az` is not available: {error}"))?;

    // Treat a non-zero exit status as "no active account" instead of a hard failure.
    if !output.status.success() {
        return Ok(None);
    }

    // Decode the captured bytes into text, tolerating invalid UTF-8 if necessary.
    let raw_output = String::from_utf8_lossy(&output.stdout);
    // Parse the text into a strongly typed `AzureAccount`, if the format looks valid.
    let account = parse_account_from_tsv(raw_output.as_ref());

    // Return the parsed account, or `None` when the output was incomplete.
    Ok(account)
}

// Run an Azure CLI command while attaching it to the current terminal session.
pub(crate) async fn run_az_interactive_command(args: &[&str]) -> AppResult<bool> {
    // Start from the platform-correct Azure CLI executable name.
    let status = azure_cli_command()
        // Pass through the command-specific arguments such as `login` or `logout`.
        .args(args)
        // Reuse the current terminal input so interactive login can ask questions.
        .stdin(Stdio::inherit())
        // Reuse the current terminal output so the user sees Azure CLI output live.
        .stdout(Stdio::inherit())
        // Reuse the current terminal error stream for visible failures.
        .stderr(Stdio::inherit())
        // Run the process and wait for it to finish.
        .status()
        .await
        // Convert process startup errors into a readable application error.
        .map_err(|error| format!("`az` is not available: {error}"))?;

    // Return `true` when the command exited successfully.
    Ok(status.success())
}

// Run `az login` with service-principal credentials that were already resolved and validated.
pub(crate) async fn run_az_service_principal_login(
    tenant: &str,
    client_id: &str,
    client_secret: &str,
) -> AppResult<bool> {
    // Build the exact Azure CLI arguments once so the same sequence is reused consistently.
    let arguments = build_service_principal_login_arguments(tenant, client_id, client_secret);
    // Start from the platform-correct Azure CLI executable name.
    let status = azure_cli_command()
        // Pass the service-principal login arguments to Azure CLI.
        .args(arguments)
        // Reuse the current terminal output so Azure CLI messages remain visible.
        .stdout(Stdio::inherit())
        // Reuse the current terminal error stream for visible failures.
        .stderr(Stdio::inherit())
        // Run the process and wait for it to finish.
        .status()
        .await
        // Convert process startup errors into a readable application error.
        .map_err(|error| format!("`az` is not available: {error}"))?;

    // Return `true` when the command exited successfully.
    Ok(status.success())
}

// Build the Azure inventory report, write it to disk and return the final path.
pub(crate) async fn generate_inventory_report(account: &AzureAccount) -> AppResult<PathBuf> {
    // Ask Azure CLI for all resource groups that belong to the active subscription.
    let resource_groups = fetch_resource_groups().await?;
    // Build the grouped inventory by fetching resources for each resource group.
    let inventory_groups = build_inventory_groups(resource_groups).await?;
    // Count the total number of resources so the metadata stays accurate.
    let total_resource_count = count_total_resources(&inventory_groups);
    // Create the full Markdown text from the report template.
    let markdown = render_inventory_markdown(account, &inventory_groups, total_resource_count)?;
    // Resolve the target directory inside the user's home directory.
    let output_directory = resolve_inventory_output_directory()?;

    // Create the directory tree when it does not exist yet.
    fs::create_dir_all(&output_directory).map_err(|error| {
        format!(
            "unable to create the inventory output directory `{}`: {error}",
            output_directory.display()
        )
    })?;

    // Generate a unique filename so every run creates a new Markdown document.
    let file_name = build_inventory_file_name();
    // Join the directory and filename into the final full path.
    let output_file_path = output_directory.join(file_name);

    // Write the generated Markdown to the target file.
    fs::write(&output_file_path, markdown).map_err(|error| {
        format!(
            "unable to write the inventory file `{}`: {error}",
            output_file_path.display()
        )
    })?;

    // Return the full path to the newly created inventory report.
    Ok(output_file_path)
}

// Return the directory where Azure inventory reports are stored.
pub(crate) fn inventory_reports_directory() -> AppResult<PathBuf> {
    // Reuse the same report helper as generation so list and generate stay in sync.
    resolve_inventory_output_directory()
}

// List saved Azure inventory reports from the standard inventory directory.
pub(crate) fn list_inventory_reports() -> AppResult<Vec<InventoryReportFile>> {
    // Resolve the directory once so errors mention the same location users know from generation.
    let output_directory = inventory_reports_directory()?;
    // Delegate to the path-based helper so tests can use temporary directories.
    list_inventory_reports_in_directory(&output_directory)
}

// Build the Azure resource snapshot, write it to disk and return the final path.
pub(crate) async fn generate_resource_snapshot(account: &AzureAccount) -> AppResult<PathBuf> {
    // Ask Azure CLI for every resource in the active subscription.
    let raw_resources = fetch_raw_subscription_resources().await?;
    // Convert the raw resources into the snapshot envelope with normalized fields and hashes.
    let snapshot = build_snapshot_envelope(account, raw_resources)?;
    // Serialize the snapshot as pretty JSON so humans can inspect it easily.
    let snapshot_json = serde_json::to_string_pretty(&snapshot)
        .map_err(|error| format!("unable to serialize the Azure snapshot JSON: {error}"))?;
    // Resolve the target directory below the user's home directory.
    let output_directory = resolve_snapshot_output_directory()?;

    // Create the directory tree when it does not exist yet.
    fs::create_dir_all(&output_directory).map_err(|error| {
        format!(
            "unable to create the snapshot output directory `{}`: {error}",
            output_directory.display()
        )
    })?;

    // Generate a unique filename so every run creates a new JSON document.
    let file_name = build_snapshot_file_name();
    // Join the directory and filename into the final full path.
    let output_file_path = output_directory.join(file_name);

    // Write the generated JSON to the target file.
    fs::write(&output_file_path, snapshot_json).map_err(|error| {
        format!(
            "unable to write the snapshot file `{}`: {error}",
            output_file_path.display()
        )
    })?;

    // Return the full path to the newly created snapshot file.
    Ok(output_file_path)
}

// List saved Azure inventory reports from one concrete directory.
fn list_inventory_reports_in_directory(directory: &Path) -> AppResult<Vec<InventoryReportFile>> {
    // Try to open the directory and handle a missing directory as an empty list.
    let directory_entries = match fs::read_dir(directory) {
        Ok(directory_entries) => directory_entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Return no reports when the inventory directory has not been created yet.
            return Ok(Vec::new());
        }
        Err(error) => {
            // Return a readable error for permission problems or other filesystem failures.
            return Err(format!(
                "unable to read the inventory directory `{}`: {error}",
                directory.display()
            )
            .into());
        }
    };

    // Store only files that match the inventory report naming convention.
    let mut reports: Vec<InventoryReportFile> = Vec::new();

    // Walk through the directory entries one by one so every fallible step is explicit.
    for directory_entry_result in directory_entries {
        // Convert a failed directory entry read into a readable application error.
        let directory_entry = directory_entry_result.map_err(|error| {
            format!(
                "unable to read an entry in the inventory directory `{}`: {error}",
                directory.display()
            )
        })?;
        // Keep the full path so metadata and shell output can refer to the same file.
        let path = directory_entry.path();
        // Read metadata before accepting the entry so directories are ignored.
        let metadata = directory_entry.metadata().map_err(|error| {
            format!(
                "unable to read metadata for inventory path `{}`: {error}",
                path.display()
            )
        })?;

        // Skip directories and special files because only Markdown files are report entries.
        if !metadata.is_file() {
            continue;
        }

        // Convert the OS file name into UTF-8, ignoring names that cannot be displayed safely.
        let Some(file_name) = directory_entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };

        // Skip files that do not match the report prefix and Markdown extension.
        if !is_inventory_report_file_name(&file_name) {
            continue;
        }

        // Add the accepted report with the metadata needed for table output.
        reports.push(InventoryReportFile {
            file_name,
            path,
            modified_at: metadata.modified().ok(),
            size_bytes: metadata.len(),
        });
    }

    // Sort newest reports first, and use file name order to keep ties deterministic.
    reports.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| right.file_name.cmp(&left.file_name))
    });

    // Return the filtered and sorted list.
    Ok(reports)
}

// Check whether a file name belongs to an Azure inventory Markdown report.
fn is_inventory_report_file_name(file_name: &str) -> bool {
    // Require the generator's stable prefix so unrelated Markdown files stay hidden.
    file_name.starts_with("azure-inventory-")
        // Require the Markdown extension because generated reports are Markdown documents.
        && file_name.ends_with(".md")
}

// Ask Azure CLI for all resource groups in the current subscription.
async fn fetch_resource_groups() -> AppResult<Vec<AzureResourceGroupReportItem>> {
    // Capture JSON output because structured data is easier to parse safely than table text.
    let raw_json = run_az_json_command(&["group", "list"]).await?;
    // Parse the JSON text into typed Rust values.
    let mut resource_groups: Vec<AzureResourceGroupReportItem> = serde_json::from_str(&raw_json)
        .map_err(|error| format!("Azure CLI returned invalid resource group JSON: {error}"))?;

    // Sort the groups so the Markdown output stays predictable and easy to scan.
    sort_resource_groups(&mut resource_groups);

    // Return the ready-to-use list.
    Ok(resource_groups)
}

// Ask Azure CLI for all resources inside one specific resource group.
async fn fetch_resources_for_group(
    resource_group_name: &str,
) -> AppResult<Vec<AzureResourceReportItem>> {
    // Pass the group name explicitly so Azure returns only the matching resources.
    let raw_json =
        run_az_json_command(&["resource", "list", "--resource-group", resource_group_name]).await?;
    // Parse the JSON text into typed Rust values.
    let mut resources: Vec<AzureResourceReportItem> = serde_json::from_str(&raw_json)
        .map_err(|error| format!("Azure CLI returned invalid resource JSON: {error}"))?;

    // Sort the resources so each group section stays stable and readable.
    sort_resources(&mut resources);

    // Return the ready-to-use list.
    Ok(resources)
}

// Ask Azure CLI for all resources in the active subscription as raw JSON values.
async fn fetch_raw_subscription_resources() -> AppResult<Vec<serde_json::Value>> {
    // Capture JSON output because the snapshot must preserve Azure's raw resource objects.
    let raw_json = run_az_json_command(&["resource", "list"]).await?;
    // Parse the JSON text into flexible `Value` objects so unknown fields are preserved.
    let raw_resources: Vec<serde_json::Value> = serde_json::from_str(&raw_json)
        .map_err(|error| format!("Azure CLI returned invalid resource snapshot JSON: {error}"))?;

    // Return the raw resources exactly as parsed from Azure CLI output.
    Ok(raw_resources)
}

// Build the full inventory by pairing every resource group with its resources.
async fn build_inventory_groups(
    resource_groups: Vec<AzureResourceGroupReportItem>,
) -> AppResult<Vec<AzureInventoryGroup>> {
    // Allocate the output vector up front because we know the final group count already.
    let mut inventory_groups: Vec<AzureInventoryGroup> = Vec::with_capacity(resource_groups.len());

    // Walk through each resource group one by one so errors stay easy to attribute.
    for resource_group in resource_groups {
        // Fetch all resources that belong to the current group.
        let resources = fetch_resources_for_group(&resource_group.name).await?;
        // Store the combined group section for later Markdown rendering.
        inventory_groups.push(AzureInventoryGroup {
            resource_group,
            resources,
        });
    }

    // Return the full grouped inventory.
    Ok(inventory_groups)
}

// Run one Azure CLI command and capture successful JSON output as text.
async fn run_az_json_command(args: &[&str]) -> AppResult<String> {
    // Start from the platform-correct Azure CLI executable name.
    let output = azure_cli_command()
        // Pass the Azure CLI command-specific arguments.
        .args(args)
        // Force JSON output so parsing stays structured and predictable.
        .args(["--output", "json"])
        // Capture standard output because we need to parse the JSON text.
        .stdout(Stdio::piped())
        // Capture standard error so we can show Azure CLI failures to the user.
        .stderr(Stdio::piped())
        // Spawn the child process and wait for it to finish.
        .output()
        .await
        // Convert process startup errors into a readable application error.
        .map_err(|error| format!("`az` is not available: {error}"))?;

    // Return a detailed error when Azure CLI reported a non-zero exit status.
    if !output.status.success() {
        // Decode the error stream into readable text.
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Remove surrounding whitespace so the message prints neatly.
        let trimmed_stderr = stderr.trim();

        // Prefer Azure CLI's own error text when it provided one.
        if !trimmed_stderr.is_empty() {
            return Err(format!("Azure CLI error: {trimmed_stderr}").into());
        }

        // Fall back to the process exit status when no stderr text was available.
        return Err(format!("Azure CLI exited with status {}", output.status).into());
    }

    // Decode the JSON bytes into text before we hand them to `serde_json`.
    // We first try strict UTF-8 because JSON is expected to use UTF-8 encoding.
    // This keeps the common case simple and preserves the original text unchanged.
    let raw_json = decode_azure_cli_json_output(&output.stdout);

    // Return the decoded JSON text.
    Ok(raw_json)
}

// Convert captured Azure CLI JSON bytes into a Rust `String`.
fn decode_azure_cli_json_output(stdout_bytes: &[u8]) -> String {
    // Try the strict UTF-8 path first because valid JSON should already decode cleanly.
    match String::from_utf8(stdout_bytes.to_vec()) {
        Ok(valid_utf8_text) => {
            // Return the original text unchanged when the bytes were valid UTF-8.
            valid_utf8_text
        }
        Err(_) => {
            // Fall back to a lossy decode when Azure CLI emits one or more invalid bytes.
            // This replaces only the broken byte sequences with the Unicode replacement character.
            // We prefer this over aborting the full inventory command, because the surrounding JSON
            // structure is often still intact enough for `serde_json` to parse successfully.
            String::from_utf8_lossy(stdout_bytes).into_owned()
        }
    }
}

// Build a `Command` with the correct Azure CLI executable name for this platform.
fn azure_cli_command() -> Command {
    #[cfg(windows)]
    {
        // Azure CLI commonly installs `az` as `az.cmd` on Windows.
        // Rust does not automatically resolve `.cmd` when the extension is omitted.
        Command::new("az.cmd")
    }

    #[cfg(not(windows))]
    {
        // Unix-like systems usually expose Azure CLI directly as `az`.
        Command::new("az")
    }
}

// Build the exact `az login --service-principal` argument list in one place.
fn build_service_principal_login_arguments<'a>(
    tenant: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
) -> [&'a str; 8] {
    // Return the argument list in the same order a user would type it manually.
    [
        "login",
        "--service-principal",
        "--tenant",
        tenant,
        "--username",
        client_id,
        "--password",
        client_secret,
    ]
}

// Parse the three lines returned by our TSV query into an `AzureAccount`.
fn parse_account_from_tsv(raw_output: &str) -> Option<AzureAccount> {
    // Collect the non-empty trimmed lines into a vector we can inspect safely.
    let mut fields: Vec<&str> = Vec::new();

    // Walk over each line because the Azure CLI query returns one value per line.
    for line in raw_output.lines() {
        // Trim surrounding whitespace so accidental spacing does not break parsing.
        let trimmed = line.trim();
        // Skip empty lines because they do not hold useful field data.
        if trimmed.is_empty() {
            continue;
        }
        // Store the cleaned field for later validation.
        fields.push(trimmed);
    }

    // Require exactly three fields: subscription name, subscription id and user.
    if fields.len() != 3 {
        return None;
    }

    // Clone the first field into an owned `String` for the struct.
    let name = fields[0].to_owned();
    // Clone the second field into an owned `String` for the struct.
    let subscription_id = fields[1].to_owned();
    // Clone the third field into an owned `String` for the struct.
    let user = fields[2].to_owned();

    // Return the fully parsed account data.
    Some(AzureAccount {
        name,
        subscription_id,
        user,
    })
}

#[cfg(test)]
mod tests {
    // Import the helpers under test from the parent module.
    use super::{
        build_service_principal_login_arguments, decode_azure_cli_json_output,
        list_inventory_reports_in_directory, parse_account_from_tsv,
    };
    // Import filesystem helpers so tests can create isolated report directories.
    use std::fs;
    // Import environment helpers so tests can write below the system temporary directory.
    use std::env;
    // Import `Uuid` so every test directory gets a unique name.
    use uuid::Uuid;

    #[test]
    fn parses_account_from_expected_tsv_output() {
        // Build one small TSV snippet that matches the Azure CLI query output shape.
        let raw_output =
            "My Subscription\n00000000-0000-0000-0000-000000000000\nalice@example.com\n";
        // Parse the TSV text into the typed account model.
        let account = parse_account_from_tsv(raw_output).expect("account should parse");

        // Confirm that the subscription name was parsed from the first line.
        assert_eq!(account.name, "My Subscription");
        // Confirm that the subscription ID was parsed from the second line.
        assert_eq!(
            account.subscription_id,
            "00000000-0000-0000-0000-000000000000"
        );
        // Confirm that the Azure user was parsed from the third line.
        assert_eq!(account.user, "alice@example.com");
    }

    #[test]
    fn builds_service_principal_login_arguments_in_expected_order() {
        // Build the Azure CLI argument list for one fake service principal.
        let arguments =
            build_service_principal_login_arguments("tenant-id", "client-id", "secret-value");

        // Confirm that the generated argument order matches the documented Azure CLI syntax.
        assert_eq!(
            arguments,
            [
                "login",
                "--service-principal",
                "--tenant",
                "tenant-id",
                "--username",
                "client-id",
                "--password",
                "secret-value",
            ]
        );
    }

    #[test]
    fn keeps_valid_utf8_json_output_unchanged() {
        // Build one small JSON example that already uses valid UTF-8 text.
        let stdout_bytes = br#"{"name":"storage-account"}"#;

        // Decode the bytes with the Azure CLI JSON helper.
        let decoded_output = decode_azure_cli_json_output(stdout_bytes);

        // Confirm that clean UTF-8 output is preserved exactly.
        assert_eq!(decoded_output, r#"{"name":"storage-account"}"#);
    }

    #[test]
    fn replaces_invalid_utf8_bytes_when_decoding_json_output() {
        // Build one byte sequence that contains invalid UTF-8 inside a JSON string value.
        let stdout_bytes = b"{\"name\":\"stor\xffage-account\"}";

        // Decode the bytes with the Azure CLI JSON helper.
        let decoded_output = decode_azure_cli_json_output(stdout_bytes);

        // Confirm that the invalid byte becomes the Unicode replacement character.
        assert_eq!(decoded_output, "{\"name\":\"stor\u{FFFD}age-account\"}");
    }

    #[test]
    fn lists_inventory_reports_newest_first_and_ignores_unrelated_files() {
        // Build an isolated temporary directory for this filesystem test.
        let directory = env::temp_dir().join(format!(
            "martijn-cli-inventory-test-{}",
            Uuid::new_v4().simple()
        ));
        // Create the temporary directory before writing report files into it.
        fs::create_dir_all(&directory).expect("temporary directory should be created");

        // Create an older-looking inventory report that should sort after the newer one.
        let older_report = directory.join("azure-inventory-20260423-080734-11111111.md");
        // Write small Markdown content so the file has metadata and a non-zero size.
        fs::write(&older_report, "# Older report\n").expect("older report should be written");
        // Create a newer-looking inventory report that should sort first.
        let newer_report = directory.join("azure-inventory-20260424-072348-22222222.md");
        // Write small Markdown content so the file has metadata and a non-zero size.
        fs::write(&newer_report, "# Newer report\n").expect("newer report should be written");
        // Create an unrelated Markdown file that should not appear in the listing.
        fs::write(directory.join("notes.md"), "# Notes\n")
            .expect("unrelated file should be written");

        // Ask the listing helper to read, filter, and sort the temporary directory.
        let reports =
            list_inventory_reports_in_directory(&directory).expect("reports should be listed");

        // Confirm that only files matching the inventory naming convention are returned.
        assert_eq!(reports.len(), 2);
        // Confirm that the newer report appears first in the listing.
        assert_eq!(
            reports[0].file_name,
            "azure-inventory-20260424-072348-22222222.md"
        );
        // Confirm that the older report appears after the newer report.
        assert_eq!(
            reports[1].file_name,
            "azure-inventory-20260423-080734-11111111.md"
        );

        // Clean up the temporary directory after the assertions complete.
        fs::remove_dir_all(&directory).expect("temporary directory should be removed");
    }

    #[test]
    fn missing_inventory_directory_returns_an_empty_list() {
        // Build a path that should not exist because it includes a fresh UUID.
        let missing_directory = env::temp_dir().join(format!(
            "martijn-cli-missing-inventory-test-{}",
            Uuid::new_v4().simple()
        ));

        // Ask the listing helper to read the missing directory.
        let reports = list_inventory_reports_in_directory(&missing_directory)
            .expect("missing directory should not be an error");

        // Confirm that a missing directory behaves like an empty inventory history.
        assert!(reports.is_empty());
    }
}
