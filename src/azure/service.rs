// Import the Azure data model types shared by the shell and report layers.
use crate::azure::model::{
    AzureAccount, AzureInventoryGroup, AzureResourceGroupReportItem, AzureResourceReportItem,
};
// Import the Azure report helpers used to sort, render and locate output files.
use crate::azure::report::{
    build_inventory_file_name, count_total_resources, render_inventory_markdown,
    resolve_inventory_output_directory, sort_resource_groups, sort_resources,
};
// Import the shared application result type.
use crate::AppResult;
// Import filesystem helpers so we can create directories and write files.
use std::fs;
// Import `PathBuf` because this service returns the written file path.
use std::path::PathBuf;
// Import `Stdio` so we can control how spawned processes use standard streams.
use std::process::Stdio;
// Import Tokio's async `Command` so Azure CLI calls work inside async code.
use tokio::process::Command;

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
    // Create the full Markdown text, including the YAML front matter.
    let markdown = render_inventory_markdown(account, &inventory_groups, total_resource_count);
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

    // Decode the JSON bytes into a Rust `String`.
    let raw_json = String::from_utf8(output.stdout)
        .map_err(|error| format!("Azure CLI output was not valid UTF-8: {error}"))?;

    // Return the decoded JSON text.
    Ok(raw_json)
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
    use super::{build_service_principal_login_arguments, parse_account_from_tsv};

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
}
