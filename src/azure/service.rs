// Import the Azure data model types shared by commands and report layers.
use crate::azure::model::{
    AzureAccount, AzureGroupSnapshotEnvelope, AzureInventoryGroup, AzureResourceGroupReportItem,
    AzureResourceReportItem, AzureResourceSkuReportItem, AzureSnapshotEnvelope,
    AzureSnapshotNormalizedResource,
};
// Import the Azure report helpers used to sort, render and locate output files.
use crate::azure::report::{
    count_total_resources, render_inventory_markdown, resolve_inventory_output_directory,
    sort_resource_groups, sort_resources,
};
// Import the Azure snapshot helpers used to normalize, render and locate JSON snapshots.
use crate::azure::snapshot::{
    build_group_snapshot_envelope, build_snapshot_envelope, build_snapshot_file_name,
    resolve_group_snapshot_output_directory, resolve_resource_snapshot_output_directory,
    resolve_snapshot_output_directory,
};
// Import the shared application result type.
use crate::AppResult;
// Import `Serialize` so tree report metadata can be passed into a Tera template.
use serde::Serialize;
// Import `BTreeMap` so snapshot resources can be grouped into stable tree output.
use std::collections::BTreeMap;
// Import filesystem helpers so we can create directories and write files.
use std::fs;
// Import `Path` and `PathBuf` because this service reads and writes report files.
use std::path::{Path, PathBuf};
// Import `Stdio` so we can control how spawned processes use standard streams.
use std::process::Stdio;
// Import `SystemTime` so inventory listings can expose filesystem modification times.
use std::time::SystemTime;
// Import Tera helpers so saved tree reports can be rendered from a Markdown template.
use tera::{Context, Tera};
// Import time helpers so saved reports can show when they were generated.
use time::OffsetDateTime;
// Import the custom timestamp formatter macro used for human-readable report metadata.
use time::macros::format_description;
// Import Tokio's async `Command` so Azure CLI calls work inside async code.
use tokio::process::Command;

// Keep the resources-tree template name in one constant so registration and rendering stay aligned.
const INVENTORY_RESOURCES_TREE_TEMPLATE_NAME: &str = "inventory.resource.tree.md.tera";
// Embed the saved tree Markdown template at compile time so runtime file lookup is unnecessary.
const INVENTORY_RESOURCES_TREE_TEMPLATE_SOURCE: &str =
    include_str!("templates/inventory.resource.tree.md.tera");

// Store the fields needed for the subscription-wide `inventory resource list` command.
#[cfg(test)]
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

// Store the prepared data for one resource-tree render.
#[derive(Debug, PartialEq, Eq)]
struct InventoryResourcesTreeView {
    // Store the subscription name that becomes the root of the plain text tree.
    subscription_name: String,
    // Store the subscription ID so saved Markdown can identify the source snapshot.
    subscription_id: String,
    // Store the Azure user from the snapshot metadata for saved Markdown reports.
    azure_user: String,
    // Store how many resource-group buckets are visible in the rendered tree.
    resource_group_count: usize,
    // Store how many resources were read from the snapshot.
    total_resources: usize,
    // Store resources grouped by resource group in deterministic display order.
    grouped_resources: BTreeMap<String, Vec<String>>,
    // Store the final plain text tree so stdout and saved Markdown can share it.
    tree_body: String,
}

// Store the exact fields used by the saved resources-tree Markdown template.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct InventoryResourcesTreeTemplateView {
    // Store the human-readable UTC time when the saved report was rendered.
    generated_at: String,
    // Store the subscription name shown in the report metadata.
    subscription_name: String,
    // Store the subscription ID shown in the report metadata.
    subscription_id: String,
    // Store the Azure user shown in the report metadata.
    azure_user: String,
    // Store how many resource groups are visible in the tree.
    resource_group_count: usize,
    // Store how many resources are visible in the tree.
    total_resources: usize,
    // Store the plain text tree that will be placed in one Markdown code block.
    tree_body: String,
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
    // Store reports created by `inventory resource list`.
    ResourcesList,
    // Store reports created by `inventory resource tree`.
    ResourcesTree,
    // Store reports created by `inventory group list`.
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
    // Store the snapshot kind so command output can show whether this is a resource or group snapshot.
    pub(crate) kind: SnapshotKind,
    // Store the plain file name so command output can stay compact.
    pub(crate) file_name: String,
    // Store the full path so callers can still locate the file exactly.
    pub(crate) path: PathBuf,
    // Store the optional modified time because some filesystems may not provide it.
    pub(crate) modified_at: Option<SystemTime>,
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

// Build the resources-list Markdown document from a saved resources snapshot.
pub(crate) fn render_inventory_resources_list(snapshot_name: Option<&str>) -> AppResult<String> {
    // Read the selected resource snapshot from local disk.
    let snapshot = read_resource_snapshot(snapshot_name)?;
    // Render the loaded snapshot through the shared Markdown path used by stdout and `--save`.
    render_inventory_resources_list_from_snapshot(snapshot)
}

// Build the resources-tree Markdown document from a saved resources snapshot.
pub(crate) fn render_inventory_resources_tree(snapshot_name: Option<&str>) -> AppResult<String> {
    // Read the selected resource snapshot from local disk.
    let snapshot = read_resource_snapshot(snapshot_name)?;
    // Render the loaded snapshot through the shared Markdown path used by stdout and `--save`.
    render_inventory_resources_tree_from_snapshot(snapshot)
}

// Render resources-list Markdown from an already loaded snapshot.
fn render_inventory_resources_list_from_snapshot(
    snapshot: AzureSnapshotEnvelope,
) -> AppResult<String> {
    // Delegate to the rich resources-list template so there is only one report body shape.
    render_saved_resources_list_markdown_from_snapshot(snapshot)
}

// Render a saved resources-list Markdown report from an already loaded snapshot.
fn render_saved_resources_list_markdown_from_snapshot(
    snapshot: AzureSnapshotEnvelope,
) -> AppResult<String> {
    // Convert snapshot metadata into the account model expected by the template renderer.
    let account = account_from_resource_snapshot(&snapshot);
    // Convert the flat snapshot resources into resource-group sections for the report template.
    let inventory_groups = inventory_groups_from_resource_snapshot(snapshot);
    // Count resources through the shared report helper so the template receives consistent totals.
    let total_resource_count = count_total_resources(&inventory_groups);

    // Render the final Markdown with `inventory.resource.list.md.tera`.
    render_inventory_markdown(&account, &inventory_groups, total_resource_count)
}

// Render resources-tree Markdown from an already loaded snapshot.
fn render_inventory_resources_tree_from_snapshot(
    snapshot: AzureSnapshotEnvelope,
) -> AppResult<String> {
    // Build the tree view once so metadata and tree body stay internally consistent.
    let tree_view = resource_tree_view_from_snapshot(snapshot);
    // Render the final Markdown from that prepared view.
    render_saved_resources_tree_markdown_from_view(tree_view)
}

// Render saved resources-tree Markdown from an already prepared tree view.
fn render_saved_resources_tree_markdown_from_view(
    tree_view: InventoryResourcesTreeView,
) -> AppResult<String> {
    // Capture the current UTC time once for the saved report metadata.
    let generated_at = OffsetDateTime::now_utc();
    // Describe the compact timestamp format used by the other inventory Markdown reports.
    let generated_at_format = format_description!("[year]-[month]-[day] [hour]:[minute] UTC");
    // Convert the timestamp into text and surface a clear error if formatting fails.
    let generated_at_display = generated_at.format(&generated_at_format).map_err(|error| {
        format!("unable to format the resources tree report generation timestamp: {error}")
    })?;

    // Double-check during development that the cached count matches the grouped tree data.
    debug_assert_eq!(
        tree_view.resource_group_count,
        tree_view.grouped_resources.len()
    );

    // Build the serializable view that exactly matches the Tera template fields.
    let template_view = InventoryResourcesTreeTemplateView {
        generated_at: generated_at_display,
        subscription_name: tree_view.subscription_name,
        subscription_id: tree_view.subscription_id,
        azure_user: tree_view.azure_user,
        resource_group_count: tree_view.resource_group_count,
        total_resources: tree_view.total_resources,
        tree_body: ensure_trailing_newline(&tree_view.tree_body),
    };
    // Convert the strongly typed view into a Tera context for template rendering.
    let template_context = Context::from_serialize(&template_view)
        .map_err(|error| format!("unable to build the resources tree template context: {error}"))?;

    // Create a fresh in-memory Tera registry for this single embedded template.
    let mut tera = Tera::default();
    // Register the Markdown template under a stable logical name.
    tera.add_raw_template(
        INVENTORY_RESOURCES_TREE_TEMPLATE_NAME,
        INVENTORY_RESOURCES_TREE_TEMPLATE_SOURCE,
    )
    .map_err(|error| format!("unable to load the resources tree Markdown template: {error}"))?;

    // Render the saved Markdown report from the prepared context.
    let markdown = tera
        .render(INVENTORY_RESOURCES_TREE_TEMPLATE_NAME, &template_context)
        .map_err(|error| {
            format!("unable to render the resources tree Markdown template: {error}")
        })?;

    // Return the completed Markdown document.
    Ok(markdown)
}

// Build a terminal-friendly resource-group list from a saved groups snapshot.
pub(crate) fn render_inventory_groups_list(snapshot_name: Option<&str>) -> AppResult<String> {
    // Read the selected group snapshot from local disk.
    let snapshot = read_group_snapshot(snapshot_name)?;
    // Convert snapshot entries into the smaller shape used by the group renderer.
    let mut resource_groups = group_list_items_from_snapshot(snapshot);
    // Sort the groups so repeated runs stay easy to compare.
    sort_resource_groups(&mut resource_groups);
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

// Read a resource snapshot selected by name or by newest matching file.
fn read_resource_snapshot(snapshot_name: Option<&str>) -> AppResult<AzureSnapshotEnvelope> {
    // Resolve the concrete snapshot file before reading JSON from disk.
    let snapshot_file = select_snapshot_file(SnapshotKind::Resources, snapshot_name)?;
    // Read the whole JSON document as text so serde can parse it safely.
    let snapshot_json = fs::read_to_string(&snapshot_file.path).map_err(|error| {
        format!(
            "unable to read the resource snapshot `{}`: {error}",
            snapshot_file.path.display()
        )
    })?;
    // Deserialize the snapshot into the typed envelope used by snapshot generation.
    serde_json::from_str(&snapshot_json).map_err(|error| {
        format!(
            "unable to parse the resource snapshot `{}`: {error}",
            snapshot_file.path.display()
        )
        .into()
    })
}

// Read a resource-group snapshot selected by name or by newest matching file.
fn read_group_snapshot(snapshot_name: Option<&str>) -> AppResult<AzureGroupSnapshotEnvelope> {
    // Resolve the concrete snapshot file before reading JSON from disk.
    let snapshot_file = select_snapshot_file(SnapshotKind::Groups, snapshot_name)?;
    // Read the whole JSON document as text so serde can parse it safely.
    let snapshot_json = fs::read_to_string(&snapshot_file.path).map_err(|error| {
        format!(
            "unable to read the group snapshot `{}`: {error}",
            snapshot_file.path.display()
        )
    })?;
    // Deserialize the snapshot into the typed envelope used by snapshot generation.
    serde_json::from_str(&snapshot_json).map_err(|error| {
        format!(
            "unable to parse the group snapshot `{}`: {error}",
            snapshot_file.path.display()
        )
        .into()
    })
}

// Select one saved snapshot file by explicit name or newest file for the requested type.
fn select_snapshot_file(
    snapshot_kind: SnapshotKind,
    snapshot_name: Option<&str>,
) -> AppResult<SnapshotFile> {
    // Resolve the base snapshot directory used by normal CLI commands.
    let snapshot_directory = resolve_snapshot_output_directory()?;
    // Delegate to the directory-based helper so tests can use isolated temporary folders.
    select_snapshot_file_in_directory(snapshot_kind, snapshot_name, &snapshot_directory)
}

// Select one saved snapshot file from a concrete base directory.
fn select_snapshot_file_in_directory(
    snapshot_kind: SnapshotKind,
    snapshot_name: Option<&str>,
    directory: &Path,
) -> AppResult<SnapshotFile> {
    // List all snapshots through the same path and sorting logic used by `snapshot list`.
    let snapshots = list_snapshots_in_directory(directory)?;
    // Keep only snapshots of the type required by the current inventory command.
    let matching_kind_snapshots = snapshots
        .into_iter()
        .filter(|snapshot| snapshot.kind == snapshot_kind)
        .collect::<Vec<SnapshotFile>>();

    // Use explicit name matching when the user provided `--snapshot`.
    if let Some(requested_name) = snapshot_name.and_then(non_empty_text) {
        // Reuse the same exact-name, stem and extension matching as snapshot deletion.
        return match match_snapshot_name(requested_name, matching_kind_snapshots) {
            ArtifactMatch::One(snapshot) => Ok(snapshot),
            ArtifactMatch::Many(snapshots) => {
                // Join names into one readable line because this is returned as an error string.
                let file_names = snapshots
                    .into_iter()
                    .map(|snapshot| snapshot.file_name)
                    .collect::<Vec<String>>()
                    .join(", ");
                // Stop instead of choosing an arbitrary ambiguous snapshot.
                Err(format!(
                    "multiple {} snapshots match `{requested_name}`: {file_names}",
                    snapshot_kind_label(snapshot_kind)
                )
                .into())
            }
            ArtifactMatch::None => Err(format!(
                "no {} snapshot found for `{requested_name}`. Run `martijn azure snapshot create {}` first.",
                snapshot_kind_label(snapshot_kind),
                snapshot_kind_label(snapshot_kind)
            )
            .into()),
        };
    }

    // Use the newest matching snapshot because `list_snapshots` already sorted newest first.
    matching_kind_snapshots.into_iter().next().ok_or_else(|| {
        format!(
            "no {} snapshots found. Run `martijn azure snapshot create {}` first.",
            snapshot_kind_label(snapshot_kind),
            snapshot_kind_label(snapshot_kind)
        )
        .into()
    })
}

// Convert a resource snapshot into rows for `inventory resource list`.
#[cfg(test)]
fn resource_list_items_from_snapshot(
    snapshot: AzureSnapshotEnvelope,
) -> Vec<AzureSubscriptionResourceListItem> {
    // Store every converted resource in a vector sized to the snapshot entry count.
    let mut resources: Vec<AzureSubscriptionResourceListItem> =
        Vec::with_capacity(snapshot.resources.len());

    // Move each snapshot resource out of the envelope so no unnecessary clones are needed.
    for snapshot_resource in snapshot.resources {
        // Move the normalized resource into a shorter local name for readable field access.
        let normalized = snapshot_resource.normalized;
        // Keep only the fields that the list renderer prints.
        resources.push(AzureSubscriptionResourceListItem {
            name: normalized.name,
            resource_type: normalized.resource_type,
            resource_group: normalized.resource_group,
            location: normalized.location,
        });
    }

    // Return the converted list rows.
    resources
}

// Convert a group snapshot into rows for `inventory group list`.
fn group_list_items_from_snapshot(
    snapshot: AzureGroupSnapshotEnvelope,
) -> Vec<AzureResourceGroupReportItem> {
    // Store every converted group in a vector sized to the snapshot entry count.
    let mut groups: Vec<AzureResourceGroupReportItem> = Vec::with_capacity(snapshot.groups.len());

    // Move each snapshot group out of the envelope so no unnecessary clones are needed.
    for snapshot_group in snapshot.groups {
        // Move the normalized group into a shorter local name for readable field access.
        let normalized = snapshot_group.normalized;
        // Keep only the fields that the group list renderer prints.
        groups.push(AzureResourceGroupReportItem {
            name: normalized.name,
            location: normalized.location,
        });
    }

    // Return the converted group rows.
    groups
}

// Build the account shape required by the Markdown report from snapshot metadata.
fn account_from_resource_snapshot(snapshot: &AzureSnapshotEnvelope) -> AzureAccount {
    // Clone the subscription metadata because the returned account owns its strings.
    AzureAccount {
        name: snapshot.subscription.name.clone(),
        subscription_id: snapshot.subscription.id.clone(),
        user: snapshot.subscription.user.clone(),
    }
}

// Convert a flat resource snapshot into grouped inventory data for the list report.
fn inventory_groups_from_resource_snapshot(
    snapshot: AzureSnapshotEnvelope,
) -> Vec<AzureInventoryGroup> {
    // Use a BTreeMap so resource-group sections are deterministic before final sorting.
    let mut groups_by_name: BTreeMap<String, AzureInventoryGroup> = BTreeMap::new();

    // Move every resource out of the snapshot so conversion does not need extra cloning.
    for snapshot_resource in snapshot.resources {
        // Move the normalized resource into a local variable for clear ownership.
        let normalized = snapshot_resource.normalized;
        // Normalize the resource-group name because an empty heading would be hard to read.
        let resource_group_name = display_value(&normalized.resource_group);
        // Convert the normalized snapshot resource into the report resource row shape.
        let report_resource = report_resource_from_snapshot_resource(normalized);

        // Insert the group on first use, then reuse it for all later resources in that group.
        let inventory_group = groups_by_name
            .entry(resource_group_name.clone())
            .or_insert_with(|| AzureInventoryGroup {
                resource_group: AzureResourceGroupReportItem {
                    name: resource_group_name,
                    // Leave group location empty because resource snapshots do not contain group metadata.
                    location: String::new(),
                },
                resources: Vec::new(),
            });

        // Store the converted resource inside its owning group.
        inventory_group.resources.push(report_resource);
    }

    // Move the grouped data into a vector for the existing report renderer.
    let mut inventory_groups: Vec<AzureInventoryGroup> = groups_by_name.into_values().collect();

    // Sort each group's resources by type and name, matching the old report behavior.
    for inventory_group in &mut inventory_groups {
        sort_resources(&mut inventory_group.resources);
    }

    // Sort the resource-group sections case-insensitively for stable report output.
    inventory_groups
        .sort_by_key(|inventory_group| inventory_group.resource_group.name.to_lowercase());

    // Return the grouped inventory model.
    inventory_groups
}

// Convert one normalized snapshot resource into the richer report resource row shape.
fn report_resource_from_snapshot_resource(
    normalized: AzureSnapshotNormalizedResource,
) -> AzureResourceReportItem {
    // Convert the flexible JSON `kind` field into an optional string for SKU fallback rendering.
    let kind = optional_string_from_json_value(normalized.kind);
    // Convert the flexible JSON `sku` field into the small SKU shape used by the report.
    let sku = report_sku_from_json_value(normalized.sku);
    // Convert the flexible JSON `tags` object into a deterministic string map.
    let tags = report_tags_from_json_value(normalized.tags);

    // Return the report row with only fields that the Markdown template needs.
    AzureResourceReportItem {
        name: normalized.name,
        resource_type: normalized.resource_type,
        location: normalized.location,
        kind,
        tags,
        sku,
    }
}

// Read an optional string from a JSON value.
fn optional_string_from_json_value(value: serde_json::Value) -> Option<String> {
    // Keep real JSON strings as Rust strings.
    match value {
        serde_json::Value::String(text) => Some(text),
        _ => None,
    }
}

// Convert the snapshot SKU JSON into the report SKU helper.
fn report_sku_from_json_value(value: serde_json::Value) -> Option<AzureResourceSkuReportItem> {
    // Continue only when Azure stored SKU as a JSON object.
    let serde_json::Value::Object(object) = value else {
        return None;
    };

    // Read the common `sku.name` field when it is present as a string.
    let name = object
        .get("name")
        .and_then(|name_value| name_value.as_str())
        .map(|name| name.to_owned());

    // Return `None` when the object did not contain any useful SKU name.
    if name.is_none() {
        return None;
    }

    // Store the SKU name in the small report model.
    Some(AzureResourceSkuReportItem { name })
}

// Convert snapshot tag JSON into the report tag map.
fn report_tags_from_json_value(value: serde_json::Value) -> Option<BTreeMap<String, String>> {
    // Continue only when Azure stored tags as a JSON object.
    let serde_json::Value::Object(object) = value else {
        return None;
    };

    // Store tags in a BTreeMap so rendered tag order is deterministic.
    let mut tags = BTreeMap::new();

    // Convert every JSON tag value into a readable string.
    for (key, value) in object {
        // Prefer plain JSON strings because Azure tags are normally strings.
        let tag_value = match value {
            serde_json::Value::String(text) => text,
            other_value => other_value.to_string(),
        };

        // Insert the tag exactly under its Azure-provided key.
        tags.insert(key, tag_value);
    }

    // Return `None` for empty tag objects so missing tags render as `-`.
    if tags.is_empty() {
        return None;
    }

    // Return the prepared tag map.
    Some(tags)
}

// Build the prepared tree view from one resource snapshot.
fn resource_tree_view_from_snapshot(snapshot: AzureSnapshotEnvelope) -> InventoryResourcesTreeView {
    // Normalize the subscription name before it becomes the root line of the tree.
    let subscription_name = display_value(&snapshot.subscription.name);
    // Normalize the subscription ID so saved Markdown has a visible fallback.
    let subscription_id = display_value(&snapshot.subscription.id);
    // Normalize the Azure user so saved Markdown has a visible fallback.
    let azure_user = display_value(&snapshot.subscription.user);
    // Count resources before moving them out of the snapshot envelope.
    let total_resources = snapshot.resources.len();
    // Convert all snapshot resources into deterministic resource-group buckets.
    let grouped_resources = resource_tree_items_from_snapshot(snapshot);
    // Count only the groups that are actually visible in this resource snapshot.
    let resource_group_count = grouped_resources.len();
    // Render the final plain text tree once so stdout and saved Markdown can share the body.
    let tree_body = render_resources_tree_text(&subscription_name, &grouped_resources);

    // Return one prepared view with both metadata and rendered output.
    InventoryResourcesTreeView {
        subscription_name,
        subscription_id,
        azure_user,
        resource_group_count,
        total_resources,
        grouped_resources,
        tree_body,
    }
}

// Group resource names from one resource snapshot by resource group name.
fn resource_tree_items_from_snapshot(
    snapshot: AzureSnapshotEnvelope,
) -> BTreeMap<String, Vec<String>> {
    // Use a BTreeMap so group names render in deterministic alphabetical order.
    let mut grouped_resources: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // Walk through every resource from the snapshot.
    for snapshot_resource in snapshot.resources {
        // Move the normalized resource into a shorter local name for readable field access.
        let normalized: AzureSnapshotNormalizedResource = snapshot_resource.normalized;
        // Normalize an empty resource group to a dash so the tree stays visible.
        let resource_group = display_value(&normalized.resource_group);
        // Normalize an empty resource name to a dash for the same reason.
        let resource_name = display_value(&normalized.name);
        // Insert the resource name below its group, creating the group bucket when needed.
        grouped_resources
            .entry(resource_group)
            .or_default()
            .push(resource_name);
    }

    // Sort resource names inside each group so the tree is stable and easy to scan.
    for resource_names in grouped_resources.values_mut() {
        // Compare lowercase forms so display order does not depend on Azure casing.
        resource_names.sort_by_key(|resource_name| resource_name.to_lowercase());
    }

    // Return the grouped tree data.
    grouped_resources
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

// Check whether a file name belongs to an inventory Markdown report.
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

        // Add the accepted snapshot with the metadata needed for sorting and deletion.
        snapshots.push(SnapshotFile {
            kind: snapshot_kind,
            file_name,
            path,
            modified_at: metadata.modified().ok(),
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
    // Match every snapshot kind to the singular subcommand word that users type.
    match snapshot_kind {
        SnapshotKind::Resources => "resource",
        SnapshotKind::Groups => "group",
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
#[cfg(test)]
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

// Render a compact resource tree as plain text for terminal output.
pub(crate) fn render_resources_tree_text(
    subscription_name: &str,
    grouped_resources: &BTreeMap<String, Vec<String>>,
) -> String {
    // Start with the subscription as the root node of the tree.
    let mut output = format!("{subscription_name}\n");

    // Stop after the root line when the snapshot contains no resource groups.
    if grouped_resources.is_empty() {
        return output;
    }

    // Store the number of groups so we can choose the final connector correctly.
    let group_count = grouped_resources.len();
    // Render each resource group as a child of the subscription root.
    for (group_index, (resource_group_name, resource_names)) in grouped_resources.iter().enumerate()
    {
        // Check whether this group is the final child of the subscription root.
        let group_is_last = group_index + 1 == group_count;
        // Pick the connector that visually marks either a middle child or the last child.
        let group_connector = if group_is_last {
            " └── "
        } else {
            " ├── "
        };

        // Append the resource group line below the subscription root.
        output.push_str(group_connector);
        // Print only the group name because this view intentionally stays compact.
        output.push_str(resource_group_name);
        // End the resource group line before rendering child resources.
        output.push('\n');

        // Store the number of resources so the final resource can get the closing connector.
        let resource_count = resource_names.len();
        // Render every resource name directly below its resource group.
        for (resource_index, resource_name) in resource_names.iter().enumerate() {
            // Check whether this resource is the final child of its group.
            let resource_is_last = resource_index + 1 == resource_count;
            // Pick the connector for a middle resource or the final resource.
            let resource_connector = if resource_is_last {
                "      └── "
            } else {
                "      ├── "
            };

            // Append the resource line using the fixed indentation requested for this command.
            output.push_str(resource_connector);
            // Print only the resource name because types and locations are intentionally omitted.
            output.push_str(resource_name);
            // End the resource line.
            output.push('\n');
        }
    }

    // Return the complete plain text tree.
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

// Return text with exactly the newline needed before a Markdown fence closes.
fn ensure_trailing_newline(text: &str) -> String {
    // Clone the input into an owned string because templates need owned serializable values.
    let mut output = text.to_owned();

    // Add a newline only when the caller supplied text without one.
    if !output.ends_with('\n') {
        output.push('\n');
    }

    // Return the normalized text.
    output
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
        AzureSubscriptionResourceListItem, InventoryReportKind, SnapshotKind,
        build_inventory_report_file_name, build_service_principal_login_arguments,
        decode_azure_cli_json_output, group_list_items_from_snapshot,
        list_inventory_reports_in_directory, parse_account_from_tsv, render_groups_list_text,
        render_inventory_resources_list_from_snapshot,
        render_inventory_resources_tree_from_snapshot, render_resources_list_text,
        render_resources_tree_text, render_saved_resources_list_markdown_from_snapshot,
        render_saved_resources_tree_markdown_from_view, resource_list_items_from_snapshot,
        resource_tree_items_from_snapshot, resource_tree_view_from_snapshot,
        select_snapshot_file_in_directory, slugify_file_stem,
    };
    // Import model types so rendering tests can build representative Azure data.
    use crate::azure::model::{
        AzureGroupSnapshotEnvelope, AzureResourceGroupReportItem, AzureSnapshotEnvelope,
        AzureSnapshotGroup, AzureSnapshotNormalizedGroup, AzureSnapshotNormalizedResource,
        AzureSnapshotResource, AzureSnapshotSubscription,
    };
    // Import `Value` so tests can fill flexible snapshot fields without custom structs.
    use serde_json::Value;
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

        // Build the report leaf directory used by `inventory resource list`.
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
    fn newest_snapshot_selection_filters_by_requested_type() {
        // Build an isolated temporary snapshot directory for this filesystem test.
        let directory = env::temp_dir().join(format!(
            "martijn-cli-snapshot-select-test-{}",
            Uuid::new_v4().simple()
        ));
        // Create the resource snapshot directory.
        let resource_directory = directory.join("resources");
        // Create the group snapshot directory.
        let group_directory = directory.join("groups");
        // Create both directories before writing files.
        fs::create_dir_all(&resource_directory).expect("resource directory should be created");
        fs::create_dir_all(&group_directory).expect("group directory should be created");

        // Write an older resource snapshot.
        fs::write(
            resource_directory.join("20260423-080000-11111111.json"),
            "{}",
        )
        .expect("older resource snapshot should be written");
        // Sleep briefly so filesystems with coarse timestamps can still observe ordering.
        std::thread::sleep(std::time::Duration::from_millis(10));
        // Write a newer resource snapshot.
        fs::write(
            resource_directory.join("20260424-080000-22222222.json"),
            "{}",
        )
        .expect("newer resource snapshot should be written");
        // Write a group snapshot that must not be selected for resource inventory.
        fs::write(group_directory.join("20260425-080000-33333333.json"), "{}")
            .expect("group snapshot should be written");

        // Select the newest resource snapshot without providing an explicit name.
        let selected_snapshot =
            select_snapshot_file_in_directory(SnapshotKind::Resources, None, &directory)
                .expect("resource snapshot should be selected");

        // Confirm that selection stayed within the resources directory.
        assert_eq!(selected_snapshot.file_name, "20260424-080000-22222222.json");

        // Clean up the temporary directory after the assertions complete.
        fs::remove_dir_all(&directory).expect("temporary directory should be removed");
    }

    #[test]
    fn named_snapshot_selection_accepts_file_stems() {
        // Build an isolated temporary snapshot directory for this filesystem test.
        let directory = env::temp_dir().join(format!(
            "martijn-cli-snapshot-name-test-{}",
            Uuid::new_v4().simple()
        ));
        // Create the group snapshot directory before writing files.
        let group_directory = directory.join("groups");
        // Create the directory before writing a group snapshot.
        fs::create_dir_all(&group_directory).expect("group directory should be created");
        // Write one group snapshot that can be selected by stem.
        fs::write(group_directory.join("daily-groups.json"), "{}")
            .expect("group snapshot should be written");

        // Select the snapshot by stem instead of full file name.
        let selected_snapshot = select_snapshot_file_in_directory(
            SnapshotKind::Groups,
            Some("daily-groups"),
            &directory,
        )
        .expect("named group snapshot should be selected");

        // Confirm that stem matching resolved the JSON file.
        assert_eq!(selected_snapshot.file_name, "daily-groups.json");

        // Clean up the temporary directory after the assertions complete.
        fs::remove_dir_all(&directory).expect("temporary directory should be removed");
    }

    #[test]
    fn missing_snapshot_selection_returns_actionable_error() {
        // Build a path that should not exist because it includes a fresh UUID.
        let missing_directory = env::temp_dir().join(format!(
            "martijn-cli-missing-snapshot-test-{}",
            Uuid::new_v4().simple()
        ));

        // Try to select a resource snapshot from the missing directory.
        let error =
            select_snapshot_file_in_directory(SnapshotKind::Resources, None, &missing_directory)
                .expect_err("missing resource snapshot should be an error");
        // Convert the application error into text so the message can be inspected.
        let message = error.to_string();

        // Confirm that the message tells the user which snapshot command to run.
        assert!(message.contains("snapshot create resource"));
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
    fn resource_snapshot_converts_to_resource_list_rows() {
        // Build a small resource snapshot envelope for conversion.
        let snapshot = sample_resource_snapshot();

        // Convert the snapshot into list-renderer rows.
        let resources = resource_list_items_from_snapshot(snapshot);
        // Render the resources into terminal text.
        let output = render_resources_list_text(&resources);

        // Confirm that the converted resource row appears in list output.
        assert!(output.contains("app-api | Microsoft.Web/sites | rg-app | westeurope"));
    }

    #[test]
    fn saved_resources_list_markdown_uses_inventory_list_template() {
        // Build a small resource snapshot envelope for Markdown rendering.
        let snapshot = sample_resource_snapshot();

        // Render the snapshot through the saved resources-list template path.
        let markdown = render_saved_resources_list_markdown_from_snapshot(snapshot)
            .expect("resources-list Markdown should render");

        // Confirm that the Tera template title is used instead of the generic wrapper title.
        assert!(markdown.starts_with("# Azure resource inventory\n\n"));
        // Confirm that the generic terminal-output code fence is not used for this report.
        assert!(!markdown.contains("```text"));
        // Confirm that subscription metadata from the snapshot is included.
        assert!(markdown.contains("- Subscription: subscription (sub)"));
        // Confirm that the resource-group section reconstructed from the snapshot is present.
        assert!(markdown.contains("### rg-app"));
        // Confirm that the HTML details layout is no longer rendered.
        assert!(!markdown.contains("<details>"));
        assert!(!markdown.contains("<summary>"));
        // Confirm that resource details are rendered as a compact Markdown bullet.
        assert!(
            markdown
                .contains("- app-api\n  - Location: westeurope\n  - SKU: P1v3\n  - Tags: env=prod")
        );
    }

    #[test]
    fn inventory_resources_list_uses_the_same_markdown_as_saved_reports() {
        // Build the Markdown through the snapshot-based helper that backs saved reports.
        let saved_markdown =
            render_saved_resources_list_markdown_from_snapshot(sample_resource_snapshot())
                .expect("saved resources-list Markdown should render");
        // Build the Markdown through the normal inventory path from the same snapshot data.
        let command_markdown =
            render_inventory_resources_list_from_snapshot(sample_resource_snapshot())
                .expect("command resources-list Markdown should render");

        // Confirm the saved Markdown remains the rich report format.
        assert!(saved_markdown.starts_with("# Azure resource inventory\n\n"));
        // Confirm stdout and `--save` use the exact same Markdown body.
        assert_eq!(command_markdown, saved_markdown);
    }

    #[test]
    fn group_snapshot_converts_to_group_list_rows() {
        // Build a small group snapshot envelope for conversion.
        let snapshot = sample_group_snapshot();

        // Convert the snapshot into group-renderer rows.
        let groups = group_list_items_from_snapshot(snapshot);
        // Render the groups into terminal text.
        let output = render_groups_list_text(&groups);

        // Confirm that the converted group row appears in list output.
        assert!(output.contains("rg-app | westeurope"));
    }

    #[test]
    fn resource_snapshot_tree_renders_only_group_and_resource_names() {
        // Build a small resource snapshot envelope for tree conversion.
        let snapshot = sample_resource_snapshot();

        // Group resource names by resource group.
        let grouped_resources = resource_tree_items_from_snapshot(snapshot);
        // Render the tree as plain text rooted at the snapshot subscription.
        let output = render_resources_tree_text("subscription", &grouped_resources);

        // Confirm that stdout is exactly the compact plain text tree shape.
        assert_eq!(output, "subscription\n └── rg-app\n      └── app-api\n");
        // Confirm that stdout no longer contains Markdown fences.
        assert!(!output.contains("```"));
        // Confirm that intentionally omitted metadata does not leak into the tree.
        assert!(!output.contains("Microsoft.Web/sites"));
        // Confirm that intentionally omitted locations do not leak into the tree.
        assert!(!output.contains("westeurope"));
    }

    #[test]
    fn saved_tree_markdown_includes_metadata_and_one_code_block() {
        // Build a prepared tree view from the sample snapshot metadata.
        let tree_view = resource_tree_view_from_snapshot(sample_resource_snapshot());
        // Render the tree in the saved Markdown report template.
        let markdown = render_saved_resources_tree_markdown_from_view(tree_view)
            .expect("saved tree Markdown should render");

        // Confirm that the report starts with the template title.
        assert!(markdown.starts_with("# Azure resources Inventory tree\n\n"));
        // Confirm that subscription metadata from the snapshot is included.
        assert!(markdown.contains("- Subscription: subscription (sub)"));
        // Confirm that Azure user metadata from the snapshot is included.
        assert!(markdown.contains("- Azure user: user@example.com"));
        // Confirm that resource counts from the snapshot are included.
        assert!(markdown.contains("- Resource groups: 1"));
        // Confirm that total resource counts from the snapshot are included.
        assert!(markdown.contains("- Total resources: 1"));
        // Confirm that the plain tree body is included unchanged inside the Markdown report.
        assert!(markdown.contains("subscription\n └── rg-app\n      └── app-api\n"));
        // Confirm that exactly one text fence wraps the plain tree body.
        assert_eq!(markdown.matches("```text").count(), 1);
    }

    #[test]
    fn inventory_resources_tree_uses_the_saved_markdown_template_shape() {
        // Render the command Markdown from a sample snapshot.
        let command_markdown =
            render_inventory_resources_tree_from_snapshot(sample_resource_snapshot())
                .expect("command tree Markdown should render");
        // Build a prepared tree view from the same sample snapshot metadata.
        let tree_view = resource_tree_view_from_snapshot(sample_resource_snapshot());
        // Render the same Markdown shape used by both stdout and `--save`.
        let saved_markdown = render_saved_resources_tree_markdown_from_view(tree_view)
            .expect("tree Markdown should render");

        // Confirm that tree inventory output is now a Markdown document, not only the raw tree.
        assert!(command_markdown.starts_with("# Azure resources Inventory tree\n\n"));
        // Confirm the Markdown still carries the compact tree as a fenced text block.
        assert!(
            command_markdown.contains("```text\nsubscription\n └── rg-app\n      └── app-api\n")
        );
        // Confirm stdout and `--save` use the exact same Markdown body.
        assert_eq!(command_markdown, saved_markdown);
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

    // Build one small resource snapshot envelope for conversion tests.
    fn sample_resource_snapshot() -> AzureSnapshotEnvelope {
        // Return a complete envelope with the minimum fields needed by inventory conversion.
        AzureSnapshotEnvelope {
            generated_at: String::from("2026-04-28T17:15:46Z"),
            subscription: sample_snapshot_subscription(),
            resources: vec![AzureSnapshotResource {
                normalized: AzureSnapshotNormalizedResource {
                    id: String::from(
                        "/subscriptions/sub/resourceGroups/rg-app/providers/web/app-api",
                    ),
                    name: String::from("app-api"),
                    resource_type: String::from("Microsoft.Web/sites"),
                    resource_group: String::from("rg-app"),
                    location: String::from("westeurope"),
                    kind: Value::Null,
                    sku: serde_json::json!({ "name": "P1v3" }),
                    tags: serde_json::json!({ "env": "prod" }),
                },
                fingerprint: String::from("resource-fingerprint"),
                raw: Value::Object(Default::default()),
            }],
        }
    }

    // Build one small group snapshot envelope for conversion tests.
    fn sample_group_snapshot() -> AzureGroupSnapshotEnvelope {
        // Return a complete envelope with the minimum fields needed by inventory conversion.
        AzureGroupSnapshotEnvelope {
            generated_at: String::from("2026-04-28T17:15:55Z"),
            subscription: sample_snapshot_subscription(),
            groups: vec![AzureSnapshotGroup {
                normalized: AzureSnapshotNormalizedGroup {
                    id: String::from("/subscriptions/sub/resourceGroups/rg-app"),
                    name: String::from("rg-app"),
                    location: String::from("westeurope"),
                    tags: Value::Object(Default::default()),
                    managed_by: Value::Null,
                },
                fingerprint: String::from("group-fingerprint"),
                raw: Value::Object(Default::default()),
            }],
        }
    }

    // Build shared subscription metadata for sample snapshot envelopes.
    fn sample_snapshot_subscription() -> AzureSnapshotSubscription {
        // Return stable subscription metadata because conversion tests do not inspect it.
        AzureSnapshotSubscription {
            id: String::from("sub"),
            name: String::from("subscription"),
            user: String::from("user@example.com"),
        }
    }
}
