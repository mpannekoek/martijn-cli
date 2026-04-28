// Import the Azure data model types shared by commands and report layers.
use crate::azure::model::{
    AzureAccount, AzureInventoryGroup, AzureResourceGroupReportItem, AzureResourceReportItem,
};
// Import the Azure report helpers used to sort, render and locate output files.
use crate::azure::report::{
    resolve_inventory_output_directory, sort_resource_groups, sort_resources,
};
// Import the Azure snapshot helpers used to normalize, render and locate JSON snapshots.
use crate::azure::snapshot::{
    build_group_snapshot_envelope, build_named_snapshot_file_name, build_snapshot_envelope,
    resolve_group_snapshot_output_directory, resolve_resource_snapshot_output_directory,
    resolve_snapshot_output_directory,
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

// Store the fields needed for the subscription-wide `inventory resources list` command.
#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct AzureSubscriptionResourceListItem {
    // Store the resource name that users recognize in the Azure portal.
    name: String,
    // Rename the JSON field `type` because `type` is a Rust keyword.
    #[serde(rename = "type")]
    resource_type: String,
    // Rename Azure's camelCase group field to a Rust-style field name.
    #[serde(default, rename = "resourceGroup")]
    resource_group: String,
    // Use an empty string when Azure does not send a location for this resource.
    #[serde(default)]
    location: String,
}

// Describe one saved Azure inventory Markdown report on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InventoryReportFile {
    // Store the plain file name so command output can stay compact.
    pub(crate) file_name: String,
    // Store the full path so callers can still locate the file exactly.
    pub(crate) path: PathBuf,
    // Store the optional modified time because some filesystems may not provide it.
    pub(crate) modified_at: Option<SystemTime>,
    // Store the file size in bytes so users can quickly compare reports.
    pub(crate) size_bytes: u64,
}

// Describe the inventory report shape that decides where a saved report belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryReportKind {
    // Store reports created by `inventory resources list`.
    ResourcesList,
    // Store reports created by `inventory resources tree`.
    ResourcesTree,
    // Store reports created by `inventory groups list`.
    GroupsList,
}

// Describe the snapshot shape that decides where a saved snapshot belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapshotKind {
    // Store snapshots created from `az resource list`.
    Resources,
    // Store snapshots created from `az group list`.
    Groups,
}

// Describe one saved Azure snapshot JSON file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotFile {
    // Store the snapshot kind so list output can show which subdirectory was used.
    pub(crate) kind: SnapshotKind,
    // Store the plain file name so command output can stay compact.
    pub(crate) file_name: String,
    // Store the full path so callers can still locate the file exactly.
    pub(crate) path: PathBuf,
    // Store the optional modified time because some filesystems may not provide it.
    pub(crate) modified_at: Option<SystemTime>,
    // Store the file size in bytes so users can quickly compare snapshots.
    pub(crate) size_bytes: u64,
}

// Describe the result of trying to resolve a user-supplied artifact name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArtifactMatch<T> {
    // Store the single exact match that can be acted on safely.
    One(T),
    // Store multiple matching names so callers can explain why the action stopped.
    Many(Vec<T>),
    // Store the absence of matches explicitly instead of using a nullable path.
    None,
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

// Build a terminal-friendly resources list from the active subscription.
pub(crate) async fn render_inventory_resources_list() -> AppResult<String> {
    // Ask Azure CLI for all resources in one subscription-wide call.
    let mut resources = fetch_subscription_resources().await?;
    // Sort the resources so repeated runs stay easy to compare.
    sort_subscription_resources(&mut resources);
    // Render the sorted resources as readable terminal text.
    Ok(render_resources_list_text(&resources))
}

// Build a terminal-friendly resources tree from the active subscription.
pub(crate) async fn render_inventory_resources_tree() -> AppResult<String> {
    // Ask Azure CLI for all resource groups that belong to the active subscription.
    let resource_groups = fetch_resource_groups().await?;
    // Build the grouped inventory by fetching resources for each resource group.
    let inventory_groups = build_inventory_groups(resource_groups).await?;
    // Render the grouped data as simple ASCII tree output.
    Ok(render_resources_tree_text(&inventory_groups))
}

// Build a terminal-friendly resource-group list from the active subscription.
pub(crate) async fn render_inventory_groups_list() -> AppResult<String> {
    // Ask Azure CLI for all resource groups that belong to the active subscription.
    let resource_groups = fetch_resource_groups().await?;
    // Render the sorted groups as readable terminal text.
    Ok(render_groups_list_text(&resource_groups))
}

// Save one rendered inventory report body into the correct inventory subdirectory.
pub(crate) fn save_inventory_report_text(
    report_kind: InventoryReportKind,
    requested_name: Option<&str>,
    report_body: &str,
) -> AppResult<PathBuf> {
    // Resolve the target directory from the report kind before touching the filesystem.
    let output_directory = inventory_report_kind_directory(report_kind)?;

    // Create the directory tree when it does not exist yet.
    fs::create_dir_all(&output_directory).map_err(|error| {
        format!(
            "unable to create the inventory report directory `{}`: {error}",
            output_directory.display()
        )
    })?;

    // Build either a generated filename or a user-provided safe filename.
    let file_name = build_inventory_report_file_name(report_kind, requested_name);
    // Join the directory and filename into the final full path.
    let output_file_path = output_directory.join(file_name);

    // Write the generated Markdown to the target file.
    fs::write(&output_file_path, report_body).map_err(|error| {
        format!(
            "unable to write the inventory report `{}`: {error}",
            output_file_path.display()
        )
    })?;

    // Return the full path to the newly created report.
    Ok(output_file_path)
}

// Return the directory where Azure inventory reports are stored.
pub(crate) fn inventory_reports_directory() -> AppResult<PathBuf> {
    // Reuse the same report helper as generation so list and generate stay in sync.
    resolve_inventory_output_directory()
}

// List saved Azure inventory reports from the standard inventory directory.
pub(crate) fn list_inventory_reports() -> AppResult<Vec<InventoryReportFile>> {
    // Resolve the base directory once so every report subtype is searched below it.
    let output_directory = inventory_reports_directory()?;
    // Delegate to the path-based helper so tests can use temporary directories.
    list_inventory_reports_in_directory(&output_directory)
}

// Resolve one saved inventory report by user-visible file name or stem.
pub(crate) fn find_inventory_report(name: &str) -> AppResult<ArtifactMatch<InventoryReportFile>> {
    // List all reports first so matching behavior stays consistent with `report list`.
    let reports = list_inventory_reports()?;
    // Match by exact file name, file stem, or extension-added file name.
    Ok(match_inventory_report_name(name, reports))
}

// Read a saved inventory report selected by name.
pub(crate) fn read_inventory_report(name: &str) -> AppResult<ArtifactMatch<String>> {
    // Resolve the requested report before reading any file.
    let matched_report = find_inventory_report(name)?;

    // Read only when exactly one report matched.
    match matched_report {
        ArtifactMatch::One(report) => {
            // Read the full Markdown body as UTF-8 text.
            let report_body = fs::read_to_string(&report.path).map_err(|error| {
                format!(
                    "unable to read the inventory report `{}`: {error}",
                    report.path.display()
                )
            })?;

            // Return the report body wrapped in the same match shape.
            Ok(ArtifactMatch::One(report_body))
        }
        ArtifactMatch::Many(reports) => {
            // Keep the ambiguous matches so command handlers can print useful choices.
            let file_names = reports
                .into_iter()
                .map(|report| report.file_name)
                .collect::<Vec<String>>();

            // Return the ambiguous names instead of reading an arbitrary file.
            Ok(ArtifactMatch::Many(file_names))
        }
        ArtifactMatch::None => {
            // Report a clean no-match result.
            Ok(ArtifactMatch::None)
        }
    }
}

// Delete a saved inventory report selected by name.
pub(crate) fn delete_inventory_report(name: &str) -> AppResult<ArtifactMatch<PathBuf>> {
    // Resolve the requested report before deleting any file.
    let matched_report = find_inventory_report(name)?;

    // Delete only when exactly one report matched.
    match matched_report {
        ArtifactMatch::One(report) => {
            // Remove the selected file from disk.
            fs::remove_file(&report.path).map_err(|error| {
                format!(
                    "unable to delete the inventory report `{}`: {error}",
                    report.path.display()
                )
            })?;

            // Return the deleted path for user-facing confirmation.
            Ok(ArtifactMatch::One(report.path))
        }
        ArtifactMatch::Many(reports) => {
            // Keep the ambiguous matches so command handlers can print useful choices.
            let file_names = reports
                .into_iter()
                .map(|report| report.path)
                .collect::<Vec<PathBuf>>();

            // Return the ambiguous paths instead of deleting an arbitrary file.
            Ok(ArtifactMatch::Many(file_names))
        }
        ArtifactMatch::None => {
            // Report a clean no-match result.
            Ok(ArtifactMatch::None)
        }
    }
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
    let output_directory = resolve_resource_snapshot_output_directory()?;

    // Create the directory tree when it does not exist yet.
    fs::create_dir_all(&output_directory).map_err(|error| {
        format!(
            "unable to create the snapshot output directory `{}`: {error}",
            output_directory.display()
        )
    })?;

    // Generate a unique filename so every run creates a new JSON document.
    let file_name = build_named_snapshot_file_name("resources");
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

// Build the Azure resource-group snapshot, write it to disk and return the final path.
pub(crate) async fn generate_group_snapshot(account: &AzureAccount) -> AppResult<PathBuf> {
    // Ask Azure CLI for every resource group in the active subscription as raw JSON.
    let raw_groups = fetch_raw_resource_groups().await?;
    // Convert the raw groups into the snapshot envelope with normalized fields and hashes.
    let snapshot = build_group_snapshot_envelope(account, raw_groups)?;
    // Serialize the snapshot as pretty JSON so humans can inspect it easily.
    let snapshot_json = serde_json::to_string_pretty(&snapshot)
        .map_err(|error| format!("unable to serialize the Azure group snapshot JSON: {error}"))?;
    // Resolve the target directory below the user's home directory.
    let output_directory = resolve_group_snapshot_output_directory()?;

    // Create the directory tree when it does not exist yet.
    fs::create_dir_all(&output_directory).map_err(|error| {
        format!(
            "unable to create the snapshot output directory `{}`: {error}",
            output_directory.display()
        )
    })?;

    // Generate a unique filename so every run creates a new JSON document.
    let file_name = build_named_snapshot_file_name("groups");
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

// List saved Azure snapshots from every snapshot subdirectory.
pub(crate) fn list_snapshots() -> AppResult<Vec<SnapshotFile>> {
    // Resolve the base directory once so every snapshot subtype is searched below it.
    let output_directory = resolve_snapshot_output_directory()?;
    // Delegate to the path-based helper so tests can use temporary directories.
    list_snapshots_in_directory(&output_directory)
}

// Delete a saved snapshot selected by name.
pub(crate) fn delete_snapshot(name: &str) -> AppResult<ArtifactMatch<PathBuf>> {
    // List all snapshots first so matching behavior stays consistent with `snapshot list`.
    let snapshots = list_snapshots()?;
    // Match by exact file name, file stem, or extension-added file name.
    let matched_snapshot = match_snapshot_name(name, snapshots);

    // Delete only when exactly one snapshot matched.
    match matched_snapshot {
        ArtifactMatch::One(snapshot) => {
            // Remove the selected file from disk.
            fs::remove_file(&snapshot.path).map_err(|error| {
                format!(
                    "unable to delete the snapshot `{}`: {error}",
                    snapshot.path.display()
                )
            })?;

            // Return the deleted path for user-facing confirmation.
            Ok(ArtifactMatch::One(snapshot.path))
        }
        ArtifactMatch::Many(snapshots) => {
            // Keep the ambiguous matches so command handlers can print useful choices.
            let file_names = snapshots
                .into_iter()
                .map(|snapshot| snapshot.path)
                .collect::<Vec<PathBuf>>();

            // Return the ambiguous paths instead of deleting an arbitrary file.
            Ok(ArtifactMatch::Many(file_names))
        }
        ArtifactMatch::None => {
            // Report a clean no-match result.
            Ok(ArtifactMatch::None)
        }
    }
}

// List saved Azure inventory reports from one concrete base directory.
fn list_inventory_reports_in_directory(directory: &Path) -> AppResult<Vec<InventoryReportFile>> {
    // Store every report found below the inventory base directory.
    let mut reports: Vec<InventoryReportFile> = Vec::new();

    // Visit each known report-kind subdirectory in a deterministic order.
    for report_kind in [
        InventoryReportKind::ResourcesList,
        InventoryReportKind::ResourcesTree,
        InventoryReportKind::GroupsList,
    ] {
        // Build the relative path for this report kind.
        let relative_directory = inventory_report_kind_relative_directory(report_kind);
        // Join the base directory with the relative subtype directory.
        let report_directory = directory.join(relative_directory);
        // Read the reports inside this one concrete directory.
        let mut kind_reports = list_inventory_reports_in_single_directory(&report_directory)?;
        // Append the current kind's reports to the full result list.
        reports.append(&mut kind_reports);
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

// List saved Azure inventory reports from one concrete leaf directory.
fn list_inventory_reports_in_single_directory(
    directory: &Path,
) -> AppResult<Vec<InventoryReportFile>> {
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
        // Keep the full path so metadata and command output can refer to the same file.
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

    // Return the filtered and sorted list.
    Ok(reports)
}

// Check whether a file name belongs to an Azure inventory Markdown report.
fn is_inventory_report_file_name(file_name: &str) -> bool {
    // Require the Markdown extension because generated reports are Markdown documents.
    file_name.ends_with(".md")
}

// List saved Azure snapshots from one concrete base directory.
fn list_snapshots_in_directory(directory: &Path) -> AppResult<Vec<SnapshotFile>> {
    // Store every snapshot found below the snapshot base directory.
    let mut snapshots: Vec<SnapshotFile> = Vec::new();

    // Visit each known snapshot-kind subdirectory in a deterministic order.
    for snapshot_kind in [SnapshotKind::Resources, SnapshotKind::Groups] {
        // Build the relative path for this snapshot kind.
        let relative_directory = snapshot_kind_directory_name(snapshot_kind);
        // Join the base directory with the relative subtype directory.
        let snapshot_directory = directory.join(relative_directory);
        // Read the snapshots inside this one concrete directory.
        let mut kind_snapshots =
            list_snapshots_in_single_directory(snapshot_kind, &snapshot_directory)?;
        // Append the current kind's snapshots to the full result list.
        snapshots.append(&mut kind_snapshots);
    }

    // Sort newest snapshots first, and use file name order to keep ties deterministic.
    snapshots.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| right.file_name.cmp(&left.file_name))
    });

    // Return the filtered and sorted list.
    Ok(snapshots)
}

// List saved Azure snapshots from one concrete leaf directory.
fn list_snapshots_in_single_directory(
    snapshot_kind: SnapshotKind,
    directory: &Path,
) -> AppResult<Vec<SnapshotFile>> {
    // Try to open the directory and handle a missing directory as an empty list.
    let directory_entries = match fs::read_dir(directory) {
        Ok(directory_entries) => directory_entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Return no snapshots when the directory has not been created yet.
            return Ok(Vec::new());
        }
        Err(error) => {
            // Return a readable error for permission problems or other filesystem failures.
            return Err(format!(
                "unable to read the snapshot directory `{}`: {error}",
                directory.display()
            )
            .into());
        }
    };

    // Store only files that match the snapshot naming convention.
    let mut snapshots: Vec<SnapshotFile> = Vec::new();

    // Walk through the directory entries one by one so every fallible step is explicit.
    for directory_entry_result in directory_entries {
        // Convert a failed directory entry read into a readable application error.
        let directory_entry = directory_entry_result.map_err(|error| {
            format!(
                "unable to read an entry in the snapshot directory `{}`: {error}",
                directory.display()
            )
        })?;
        // Keep the full path so metadata and command output can refer to the same file.
        let path = directory_entry.path();
        // Read metadata before accepting the entry so directories are ignored.
        let metadata = directory_entry.metadata().map_err(|error| {
            format!(
                "unable to read metadata for snapshot path `{}`: {error}",
                path.display()
            )
        })?;

        // Skip directories and special files because only JSON files are snapshot entries.
        if !metadata.is_file() {
            continue;
        }

        // Convert the OS file name into UTF-8, ignoring names that cannot be displayed safely.
        let Some(file_name) = directory_entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };

        // Skip files that do not match the snapshot extension.
        if !is_snapshot_file_name(&file_name) {
            continue;
        }

        // Add the accepted snapshot with the metadata needed for table output.
        snapshots.push(SnapshotFile {
            kind: snapshot_kind,
            file_name,
            path,
            modified_at: metadata.modified().ok(),
            size_bytes: metadata.len(),
        });
    }

    // Return the filtered list for this one directory.
    Ok(snapshots)
}

// Check whether a file name belongs to an Azure snapshot JSON file.
fn is_snapshot_file_name(file_name: &str) -> bool {
    // Require the JSON extension because generated snapshots are JSON documents.
    file_name.ends_with(".json")
}

// Build the leaf directory for one inventory report kind.
fn inventory_report_kind_directory(report_kind: InventoryReportKind) -> AppResult<PathBuf> {
    // Resolve the base inventory directory once.
    let inventory_directory = inventory_reports_directory()?;
    // Append the kind-specific relative directory.
    Ok(inventory_directory.join(inventory_report_kind_relative_directory(report_kind)))
}

// Return the relative directory below `inventory` for one report kind.
fn inventory_report_kind_relative_directory(report_kind: InventoryReportKind) -> PathBuf {
    // Match every report kind to the requested folder shape.
    match report_kind {
        InventoryReportKind::ResourcesList => PathBuf::from("resources").join("list"),
        InventoryReportKind::ResourcesTree => PathBuf::from("resources").join("tree"),
        InventoryReportKind::GroupsList => PathBuf::from("groups").join("list"),
    }
}

// Return the directory name below `snapshot` for one snapshot kind.
fn snapshot_kind_directory_name(snapshot_kind: SnapshotKind) -> &'static str {
    // Match every snapshot kind to the requested folder shape.
    match snapshot_kind {
        SnapshotKind::Resources => "resources",
        SnapshotKind::Groups => "groups",
    }
}

// Return the user-facing label for one snapshot kind.
pub(crate) fn snapshot_kind_label(snapshot_kind: SnapshotKind) -> &'static str {
    // Match every snapshot kind to a compact terminal label.
    match snapshot_kind {
        SnapshotKind::Resources => "resources",
        SnapshotKind::Groups => "groups",
    }
}

// Build the Markdown filename for an inventory report.
fn build_inventory_report_file_name(
    report_kind: InventoryReportKind,
    requested_name: Option<&str>,
) -> String {
    // Use a user-provided slug when the save option contains a visible name.
    if let Some(name) = requested_name.and_then(non_empty_text) {
        // Normalize the requested name so it is safe as a file name.
        let slug = slugify_file_stem(name);
        // Add the Markdown extension when the user did not type it.
        return ensure_file_extension(&slug, "md");
    }

    // Use the report kind as the generated filename prefix.
    let prefix = inventory_report_kind_file_prefix(report_kind);
    // Delegate to the timestamp helper so generated names stay unique.
    build_generated_markdown_file_name(prefix)
}

// Return the generated filename prefix for one report kind.
fn inventory_report_kind_file_prefix(report_kind: InventoryReportKind) -> &'static str {
    // Match every report kind to a short, descriptive file prefix.
    match report_kind {
        InventoryReportKind::ResourcesList => "azure-inventory-resources-list",
        InventoryReportKind::ResourcesTree => "azure-inventory-resources-tree",
        InventoryReportKind::GroupsList => "azure-inventory-groups-list",
    }
}

// Build a unique Markdown file name with a prefix, timestamp and short UUID.
fn build_generated_markdown_file_name(prefix: &str) -> String {
    // Capture the current UTC time for the timestamp part of the filename.
    let now = time::OffsetDateTime::now_utc();
    // Describe the compact timestamp format used in the filename.
    let file_name_format =
        time::macros::format_description!("[year][month][day]-[hour][minute][second]");
    // Format the current time using the compact filename-safe representation.
    let timestamp = now
        .format(&file_name_format)
        .expect("filename timestamp formatting should succeed");
    // Generate a random UUID so repeated runs in the same second still stay unique.
    let unique_id = uuid::Uuid::new_v4().simple().to_string();
    // Keep only the first eight characters so the filename stays compact.
    let short_unique_id = &unique_id[..8];

    // Return the final filename with the required `.md` extension.
    format!("{prefix}-{timestamp}-{short_unique_id}.md")
}

// Convert one user-supplied name into a simple filesystem-safe file stem.
pub(crate) fn slugify_file_stem(name: &str) -> String {
    // Store the final slug as owned text because we build it character by character.
    let mut slug = String::new();
    // Track whether the last emitted character was a dash to avoid repeated separators.
    let mut previous_was_dash = false;

    // Walk through the input one Unicode scalar value at a time.
    for character in name.trim().chars() {
        // Lowercase ASCII letters and keep ASCII digits because they are safe everywhere.
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_was_dash = false;
            continue;
        }

        // Preserve dots only when they are inside the stem and not repeated as separators.
        if character == '.' {
            if !slug.is_empty() && !previous_was_dash {
                slug.push('.');
            }
            previous_was_dash = false;
            continue;
        }

        // Treat all other visible separators as one dash.
        if !slug.is_empty() && !previous_was_dash {
            slug.push('-');
            previous_was_dash = true;
        }
    }

    // Remove trailing separators so generated names stay tidy.
    while slug.ends_with('-') || slug.ends_with('.') {
        slug.pop();
    }

    // Fall back to a plain name when the input had no safe characters.
    if slug.is_empty() {
        return String::from("report");
    }

    // Return the safe file stem.
    slug
}

// Add a file extension when the safe stem does not already contain it.
fn ensure_file_extension(file_stem: &str, extension: &str) -> String {
    // Build the expected suffix once.
    let expected_suffix = format!(".{extension}");
    // Keep the name unchanged when it already ends with the expected extension.
    if file_stem.ends_with(&expected_suffix) {
        return file_stem.to_owned();
    }

    // Append the extension in the normal case.
    format!("{file_stem}.{extension}")
}

// Return trimmed text only when it contains visible characters.
fn non_empty_text(value: &str) -> Option<&str> {
    // Remove surrounding whitespace before deciding whether the value is usable.
    let trimmed = value.trim();

    // Treat an empty value as missing.
    if trimmed.is_empty() {
        return None;
    }

    // Return the trimmed text slice.
    Some(trimmed)
}

// Match saved inventory reports by exact filename, stem, or implicit `.md` filename.
fn match_inventory_report_name(
    name: &str,
    reports: Vec<InventoryReportFile>,
) -> ArtifactMatch<InventoryReportFile> {
    // Delegate to the generic matcher with the Markdown extension.
    match_artifact_name(name, reports, "md", |report| &report.file_name)
}

// Match saved snapshots by exact filename, stem, or implicit `.json` filename.
fn match_snapshot_name(name: &str, snapshots: Vec<SnapshotFile>) -> ArtifactMatch<SnapshotFile> {
    // Delegate to the generic matcher with the JSON extension.
    match_artifact_name(name, snapshots, "json", |snapshot| &snapshot.file_name)
}

// Match a user name against a list of artifact records.
fn match_artifact_name<T, NameGetter>(
    name: &str,
    artifacts: Vec<T>,
    extension: &str,
    get_file_name: NameGetter,
) -> ArtifactMatch<T>
where
    // Accept any function that can borrow the file name from one artifact.
    NameGetter: Fn(&T) -> &str,
{
    // Trim the requested name so accidental surrounding spaces do not matter.
    let requested_name = name.trim();
    // Add the expected extension so users may omit it.
    let requested_name_with_extension = ensure_file_extension(requested_name, extension);
    // Store every matching artifact so ambiguity can be reported safely.
    let mut matches: Vec<T> = Vec::new();

    // Walk through all artifacts and keep every name match.
    for artifact in artifacts {
        // Borrow the file name before deciding whether to move the artifact into matches.
        let file_name = get_file_name(&artifact);
        // Build the file stem by trimming only the final expected extension.
        let file_stem = file_name
            .strip_suffix(&format!(".{extension}"))
            .unwrap_or(file_name);

        // Accept exact file names, extension-added names and bare stems.
        if file_name == requested_name
            || file_name == requested_name_with_extension
            || file_stem == requested_name
        {
            matches.push(artifact);
        }
    }

    // Convert the collected matches into an explicit match result.
    match matches.len() {
        0 => ArtifactMatch::None,
        1 => ArtifactMatch::One(matches.remove(0)),
        _ => ArtifactMatch::Many(matches),
    }
}

// Render resources as compact terminal rows.
fn render_resources_list_text(resources: &[AzureSubscriptionResourceListItem]) -> String {
    // Start with a clear heading so pasted output remains understandable.
    let mut output = String::from("Azure resources\n");
    // Add a simple header row instead of depending on a table-rendering crate.
    output.push_str("Name | Type | Group | Location\n");
    // Add a separator row so the columns are easy to scan.
    output.push_str("---- | ---- | ----- | --------\n");

    // Handle empty subscriptions with an explicit row.
    if resources.is_empty() {
        output.push_str("- | - | - | -\n");
        return output;
    }

    // Render one resource per line.
    for resource in resources {
        // Normalize missing values to dashes so columns stay readable.
        let name = display_value(&resource.name);
        // Normalize missing values to dashes so columns stay readable.
        let resource_type = display_value(&resource.resource_type);
        // Normalize missing values to dashes so columns stay readable.
        let resource_group = display_value(&resource.resource_group);
        // Normalize missing values to dashes so columns stay readable.
        let location = display_value(&resource.location);

        // Append the final row to the output buffer.
        output.push_str(&format!(
            "{name} | {resource_type} | {resource_group} | {location}\n"
        ));
    }

    // Return the complete terminal output.
    output
}

// Render resources grouped under resource groups as simple ASCII tree output.
pub(crate) fn render_resources_tree_text(inventory_groups: &[AzureInventoryGroup]) -> String {
    // Start with a clear heading so pasted output remains understandable.
    let mut output = String::from("Azure resource tree\n");

    // Handle empty subscriptions with an explicit line.
    if inventory_groups.is_empty() {
        output.push_str("-\n");
        return output;
    }

    // Render every group as one parent node.
    for inventory_group in inventory_groups {
        // Normalize the group location before rendering.
        let group_location = display_value(&inventory_group.resource_group.location);
        // Append the resource group line.
        output.push_str(&format!(
            "{} ({})\n",
            display_value(&inventory_group.resource_group.name),
            group_location
        ));

        // Render an explicit placeholder for empty groups.
        if inventory_group.resources.is_empty() {
            output.push_str("  - -\n");
            continue;
        }

        // Render every resource below the group.
        for resource in &inventory_group.resources {
            // Append the resource child line with the resource type for quick scanning.
            output.push_str(&format!(
                "  - {} [{}]\n",
                display_value(&resource.name),
                display_value(&resource.resource_type)
            ));
        }
    }

    // Return the complete terminal output.
    output
}

// Render resource groups as compact terminal rows.
pub(crate) fn render_groups_list_text(resource_groups: &[AzureResourceGroupReportItem]) -> String {
    // Start with a clear heading so pasted output remains understandable.
    let mut output = String::from("Azure resource groups\n");
    // Add a simple header row instead of depending on a table-rendering crate.
    output.push_str("Name | Location\n");
    // Add a separator row so the columns are easy to scan.
    output.push_str("---- | --------\n");

    // Handle empty subscriptions with an explicit row.
    if resource_groups.is_empty() {
        output.push_str("- | -\n");
        return output;
    }

    // Render one resource group per line.
    for resource_group in resource_groups {
        // Normalize missing values to dashes so columns stay readable.
        let name = display_value(&resource_group.name);
        // Normalize missing values to dashes so columns stay readable.
        let location = display_value(&resource_group.location);
        // Append the final row to the output buffer.
        output.push_str(&format!("{name} | {location}\n"));
    }

    // Return the complete terminal output.
    output
}

// Wrap terminal text in a small Markdown report.
pub(crate) fn render_saved_inventory_markdown(title: &str, body: &str) -> String {
    // Start with a Markdown heading for the report title.
    let mut markdown = format!("# {title}\n\n");
    // Explain that the body is terminal-shaped output.
    markdown.push_str("```text\n");
    // Include the exact terminal body.
    markdown.push_str(body);
    // Ensure the fenced block starts its closing marker on a new line.
    if !body.ends_with('\n') {
        markdown.push('\n');
    }
    // Close the Markdown fenced code block.
    markdown.push_str("```\n");

    // Return the complete report.
    markdown
}

// Normalize one display value to a dash when Azure returned empty text.
fn display_value(value: &str) -> String {
    // Remove surrounding whitespace before checking whether the value is meaningful.
    let trimmed = value.trim();

    // Use a dash for missing values.
    if trimmed.is_empty() {
        return String::from("-");
    }

    // Return the visible value unchanged otherwise.
    trimmed.to_owned()
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

// Ask Azure CLI for all resources in the current subscription.
async fn fetch_subscription_resources() -> AppResult<Vec<AzureSubscriptionResourceListItem>> {
    // Capture JSON output because structured data is easier to parse safely than table text.
    let raw_json = run_az_json_command(&["resource", "list"]).await?;
    // Parse the JSON text into typed Rust values.
    let mut resources: Vec<AzureSubscriptionResourceListItem> = serde_json::from_str(&raw_json)
        .map_err(|error| format!("Azure CLI returned invalid resource JSON: {error}"))?;

    // Sort the resources so the terminal output stays predictable and easy to scan.
    sort_subscription_resources(&mut resources);

    // Return the ready-to-use list.
    Ok(resources)
}

// Sort subscription resources by type, group and name case-insensitively.
fn sort_subscription_resources(resources: &mut [AzureSubscriptionResourceListItem]) {
    // Compare lowercased values so display order does not depend on Azure casing.
    resources.sort_by(|left, right| {
        left.resource_type
            .to_lowercase()
            .cmp(&right.resource_type.to_lowercase())
            .then_with(|| {
                left.resource_group
                    .to_lowercase()
                    .cmp(&right.resource_group.to_lowercase())
            })
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
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

// Ask Azure CLI for all resource groups in the active subscription as raw JSON values.
async fn fetch_raw_resource_groups() -> AppResult<Vec<serde_json::Value>> {
    // Capture JSON output because the snapshot must preserve Azure's raw group objects.
    let raw_json = run_az_json_command(&["group", "list"]).await?;
    // Parse the JSON text into flexible `Value` objects so unknown fields are preserved.
    let raw_groups: Vec<serde_json::Value> = serde_json::from_str(&raw_json)
        .map_err(|error| format!("Azure CLI returned invalid group snapshot JSON: {error}"))?;

    // Return the raw groups exactly as parsed from Azure CLI output.
    Ok(raw_groups)
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
        AzureSubscriptionResourceListItem, InventoryReportKind, build_inventory_report_file_name,
        build_service_principal_login_arguments, decode_azure_cli_json_output,
        list_inventory_reports_in_directory, parse_account_from_tsv, render_groups_list_text,
        render_resources_list_text, render_resources_tree_text, slugify_file_stem,
    };
    // Import model types so rendering tests can build representative Azure data.
    use crate::azure::model::{
        AzureInventoryGroup, AzureResourceGroupReportItem, AzureResourceReportItem,
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

        // Build the report leaf directory used by `inventory resources list`.
        let report_directory = directory.join("resources").join("list");
        // Create the temporary report directory before writing report files into it.
        fs::create_dir_all(&report_directory).expect("report directory should be created");

        // Create an older-looking inventory report that should sort after the newer one.
        let older_report = report_directory.join("azure-inventory-20260423-080734-11111111.md");
        // Write small Markdown content so the file has metadata and a non-zero size.
        fs::write(&older_report, "# Older report\n").expect("older report should be written");
        // Create a newer-looking inventory report that should sort first.
        let newer_report = report_directory.join("azure-inventory-20260424-072348-22222222.md");
        // Write small Markdown content so the file has metadata and a non-zero size.
        fs::write(&newer_report, "# Newer report\n").expect("newer report should be written");
        // Create an unrelated Markdown file that should not appear in the listing.
        fs::write(report_directory.join("notes.txt"), "# Notes\n")
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

    #[test]
    fn slugify_file_stem_normalizes_custom_names() {
        // Normalize a name with spaces and punctuation.
        let slug = slugify_file_stem(" Daily Report! ");

        // Confirm that the slug is lowercase and separator-safe.
        assert_eq!(slug, "daily-report");
    }

    #[test]
    fn inventory_report_file_name_adds_markdown_extension_to_custom_name() {
        // Build a custom report filename from a user-provided name.
        let file_name =
            build_inventory_report_file_name(InventoryReportKind::ResourcesList, Some("Daily Run"));

        // Confirm that the custom name is slugged and gets the Markdown extension.
        assert_eq!(file_name, "daily-run.md");
    }

    #[test]
    fn render_resources_list_text_prints_expected_columns() {
        // Build one representative Azure resource.
        let resources = vec![AzureSubscriptionResourceListItem {
            name: String::from("app-api"),
            resource_type: String::from("Microsoft.Web/sites"),
            resource_group: String::from("rg-app"),
            location: String::from("westeurope"),
        }];

        // Render the resources into terminal text.
        let output = render_resources_list_text(&resources);

        // Confirm that the output includes the expected resource row.
        assert!(output.contains("app-api | Microsoft.Web/sites | rg-app | westeurope"));
    }

    #[test]
    fn render_resources_tree_text_groups_resources_under_groups() {
        // Build one grouped inventory entry.
        let inventory_groups = vec![AzureInventoryGroup {
            resource_group: AzureResourceGroupReportItem {
                name: String::from("rg-app"),
                location: String::from("westeurope"),
            },
            resources: vec![AzureResourceReportItem {
                name: String::from("app-api"),
                resource_type: String::from("Microsoft.Web/sites"),
                location: String::from("westeurope"),
                kind: None,
                tags: None,
                sku: None,
            }],
        }];

        // Render the grouped resources into terminal text.
        let output = render_resources_tree_text(&inventory_groups);

        // Confirm that the group appears as a parent row.
        assert!(output.contains("rg-app (westeurope)"));
        // Confirm that the resource appears as a child row.
        assert!(output.contains("  - app-api [Microsoft.Web/sites]"));
    }

    #[test]
    fn render_groups_list_text_prints_expected_columns() {
        // Build one representative Azure resource group.
        let groups = vec![AzureResourceGroupReportItem {
            name: String::from("rg-app"),
            location: String::from("westeurope"),
        }];

        // Render the resource groups into terminal text.
        let output = render_groups_list_text(&groups);

        // Confirm that the output includes the expected group row.
        assert!(output.contains("rg-app | westeurope"));
    }
}
