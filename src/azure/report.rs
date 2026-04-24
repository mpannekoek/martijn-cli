// Import the Azure data model types used by the renderer.
use crate::azure::model::{
    AzureAccount, AzureInventoryGroup, AzureResourceGroupReportItem, AzureResourceReportItem,
};
// Import the shared config-path helper so inventory and config live under the same base folder.
use crate::config::resolve_cli_directory;
// Import the shared application result type used by path helpers and rendering.
use crate::AppResult;
// Import `Serialize` so typed view models can be passed to Tera as template context.
use serde::Serialize;
// Import ordered maps and sets so summaries and region lists can be deterministic.
use std::collections::{BTreeMap, BTreeSet};
// Import `PathBuf` so resolved filesystem locations stay platform-aware.
use std::path::PathBuf;
// Import `Path` for the test-only helper that assembles inventory directories from a home path.
#[cfg(test)]
use std::path::Path;
// Import Tera helpers so the Markdown output can be rendered from a template file.
use tera::{Context, Tera};
// Import time helpers so the report can store readable timestamps.
use time::OffsetDateTime;
// Import the custom filename and timestamp formatter macro.
use time::macros::format_description;
// Import `Uuid` so we can generate a short unique filename suffix.
use uuid::Uuid;

// Keep the inventory template name in one constant so loading and rendering use the same key.
const INVENTORY_TEMPLATE_NAME: &str = "inventory.md.tera";
// Load the Markdown template at compile time so runtime lookup remains simple.
const INVENTORY_TEMPLATE_SOURCE: &str = include_str!("templates/inventory.md.tera");
// Keep the default Azure region in one place for non-standard region detection.
const DEFAULT_REGION: &str = "westeurope";

// Store one `label + count` row that can be rendered in summary bullet lists.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct SummaryCountRow {
    // Store the rendered label text.
    label: String,
    // Store how many resources belong to the label.
    count: usize,
}

// Store one rendered resource row for the Markdown table.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct InventoryResourceView {
    // Store the escaped resource name cell value.
    name: String,
    // Store the escaped resource type cell value.
    resource_type: String,
    // Store the escaped resource location cell value.
    location: String,
    // Store the escaped SKU cell value.
    sku: String,
    // Store the escaped tags cell value.
    tags: String,
}

// Store one rendered resource-group section in the final report.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct InventoryGroupView {
    // Store the escaped resource-group name for section headings.
    name: String,
    // Store the escaped resource-group location for section headings.
    location: String,
    // Store how many resources the group contains.
    total_resources: usize,
    // Store the group's rendered resource rows.
    resources: Vec<InventoryResourceView>,
}

// Store the full report data that the Tera template needs.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct InventoryTemplateView {
    // Store the human-readable UTC generation timestamp.
    generated_at: String,
    // Store the escaped friendly subscription name.
    subscription_name: String,
    // Store the escaped subscription identifier.
    subscription_id: String,
    // Store the escaped Azure user identifier.
    azure_user: String,
    // Store how many resource groups were processed.
    resource_group_count: usize,
    // Store how many resources were processed in total.
    total_resources: usize,
    // Store grouped counts by resource type.
    resources_per_type: Vec<SummaryCountRow>,
    // Store grouped counts by resource region.
    resources_per_region: Vec<SummaryCountRow>,
    // Store the names of empty resource groups.
    empty_resource_groups: Vec<String>,
    // Store how many resources have no tags.
    resources_without_tags: usize,
    // Store region names that differ from the default region.
    non_standard_regions: Vec<String>,
    // Store rendered resource-group sections.
    groups: Vec<InventoryGroupView>,
}

// Count all resources across every resource group in the report.
pub(crate) fn count_total_resources(inventory_groups: &[AzureInventoryGroup]) -> usize {
    // Start the running total at zero.
    let mut total_resource_count = 0usize;

    // Add the resource count from each group to the total.
    for inventory_group in inventory_groups {
        total_resource_count += inventory_group.resources.len();
    }

    // Return the final total.
    total_resource_count
}

// Sort resource groups alphabetically without making upper/lower case matter.
pub(crate) fn sort_resource_groups(resource_groups: &mut [AzureResourceGroupReportItem]) {
    // Compare lowercase versions so names sort the same regardless of original casing.
    resource_groups.sort_by_key(|resource_group| resource_group.name.to_lowercase());
}

// Sort resources by type first and by name second, both case-insensitively.
pub(crate) fn sort_resources(resources: &mut [AzureResourceReportItem]) {
    // Compare lowercased type values first and use lowercased name values as tie-breaker.
    resources.sort_by(|left, right| {
        left.resource_type
            .to_lowercase()
            .cmp(&right.resource_type.to_lowercase())
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
}

// Render the full Markdown document by feeding a typed view model into a Tera template.
pub(crate) fn render_inventory_markdown(
    account: &AzureAccount,
    inventory_groups: &[AzureInventoryGroup],
    total_resource_count: usize,
) -> AppResult<String> {
    // Capture the current UTC time once so the metadata and body stay in sync.
    let generated_at = OffsetDateTime::now_utc();
    // Describe the human-friendly timestamp format required by the report layout.
    let generated_at_format = format_description!("[year]-[month]-[day] [hour]:[minute] UTC");
    // Format the timestamp and return a clear error when formatting fails.
    let generated_at_display = generated_at
        .format(&generated_at_format)
        .map_err(|error| format!("unable to format the inventory generation timestamp: {error}"))?;

    // Build one typed view model that contains all values needed by the template.
    let template_view = build_inventory_template_view(
        account,
        inventory_groups,
        total_resource_count,
        generated_at_display,
    );
    // Convert the typed view model into a Tera context object.
    let template_context = Context::from_serialize(&template_view)
        .map_err(|error| format!("unable to build the inventory template context: {error}"))?;

    // Create a fresh in-memory Tera registry.
    let mut tera = Tera::default();
    // Register the embedded template under its fixed logical name.
    tera.add_raw_template(INVENTORY_TEMPLATE_NAME, INVENTORY_TEMPLATE_SOURCE)
        .map_err(|error| format!("unable to load the inventory Markdown template: {error}"))?;

    // Render the final Markdown text from the template and typed context.
    let markdown = tera
        .render(INVENTORY_TEMPLATE_NAME, &template_context)
        .map_err(|error| format!("unable to render the inventory Markdown template: {error}"))?;

    // Return the completed Markdown document.
    Ok(markdown)
}

// Build the full template view model from account metadata and grouped inventory data.
fn build_inventory_template_view(
    account: &AzureAccount,
    inventory_groups: &[AzureInventoryGroup],
    total_resource_count: usize,
    generated_at_display: String,
) -> InventoryTemplateView {
    // Build summary rows for `Resources per type`.
    let resources_per_type = build_resources_per_type_summary(inventory_groups);
    // Build summary rows for `Resources per region` using resource locations only.
    let resources_per_region = build_resources_per_region_summary(inventory_groups);
    // Collect and sort the names of resource groups that contain zero resources.
    let empty_resource_groups = collect_empty_resource_group_names(inventory_groups);
    // Count how many resources do not have any tags.
    let resources_without_tags = count_resources_without_tags(inventory_groups);
    // Collect all non-default regions from both resource-group and resource locations.
    let non_standard_regions = collect_non_standard_regions(inventory_groups);
    // Build rendered sections for every resource group.
    let groups = build_group_views(inventory_groups);

    // Return one fully prepared template view.
    InventoryTemplateView {
        generated_at: generated_at_display,
        subscription_name: escape_markdown_text(&account.name),
        subscription_id: escape_markdown_text(&account.subscription_id),
        azure_user: escape_markdown_text(&account.user),
        resource_group_count: inventory_groups.len(),
        total_resources: total_resource_count,
        resources_per_type,
        resources_per_region,
        empty_resource_groups,
        resources_without_tags,
        non_standard_regions,
        groups,
    }
}

// Count resources per type and return rows sorted by count descending and name ascending.
fn build_resources_per_type_summary(
    inventory_groups: &[AzureInventoryGroup],
) -> Vec<SummaryCountRow> {
    // Keep one counter bucket per lowercased type key while preserving a display label.
    let mut counters: BTreeMap<String, (String, usize)> = BTreeMap::new();

    // Walk through all resources in all groups to accumulate totals.
    for inventory_group in inventory_groups {
        for resource in &inventory_group.resources {
            // Normalize empty type values to a clear placeholder.
            let display_label = normalize_non_empty_value(&resource.resource_type, "-");
            // Use lowercase keys so type counting is case-insensitive.
            let normalized_key = display_label.to_lowercase();

            // Increment an existing bucket or create a new bucket for first encounter.
            match counters.get_mut(&normalized_key) {
                Some((_, count)) => {
                    *count += 1;
                }
                None => {
                    counters.insert(normalized_key, (display_label, 1));
                }
            }
        }
    }

    // Convert all counter buckets into serializable summary rows.
    let mut summary_rows: Vec<SummaryCountRow> = counters
        .into_values()
        .map(|(label, count)| SummaryCountRow {
            label: escape_markdown_text(&label),
            count,
        })
        .collect();

    // Sort rows by count descending, then by label ascending for deterministic output.
    sort_summary_rows(&mut summary_rows);

    // Return the sorted summary rows.
    summary_rows
}

// Count resources per region and return rows sorted by count descending and name ascending.
fn build_resources_per_region_summary(
    inventory_groups: &[AzureInventoryGroup],
) -> Vec<SummaryCountRow> {
    // Keep one counter bucket per normalized location key.
    let mut counters: BTreeMap<String, usize> = BTreeMap::new();

    // Walk through all resources in all groups to accumulate regional totals.
    for inventory_group in inventory_groups {
        for resource in &inventory_group.resources {
            // Normalize the location and fall back to `-` when Azure did not provide one.
            let normalized_region =
                normalize_region_value(&resource.location).unwrap_or_else(|| String::from("-"));

            // Increment the counter for the normalized region label.
            *counters.entry(normalized_region).or_insert(0usize) += 1;
        }
    }

    // Convert counters into serializable summary rows.
    let mut summary_rows: Vec<SummaryCountRow> = counters
        .into_iter()
        .map(|(label, count)| SummaryCountRow {
            label: escape_markdown_text(&label),
            count,
        })
        .collect();

    // Sort rows by count descending, then by label ascending for deterministic output.
    sort_summary_rows(&mut summary_rows);

    // Return the sorted summary rows.
    summary_rows
}

// Sort summary rows by count descending and label ascending (case-insensitive).
fn sort_summary_rows(summary_rows: &mut [SummaryCountRow]) {
    // Use `right.count.cmp(&left.count)` so larger counts appear first.
    summary_rows.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
    });
}

// Collect empty resource-group names and sort them alphabetically without case sensitivity.
fn collect_empty_resource_group_names(inventory_groups: &[AzureInventoryGroup]) -> Vec<String> {
    // Gather only groups where no resources were returned.
    let mut empty_group_names: Vec<String> = inventory_groups
        .iter()
        .filter(|inventory_group| inventory_group.resources.is_empty())
        .map(|inventory_group| escape_markdown_text(&inventory_group.resource_group.name))
        .collect();

    // Keep output stable and easy to scan.
    empty_group_names.sort_by_key(|name| name.to_lowercase());

    // Return the sorted names.
    empty_group_names
}

// Count resources that do not contain any usable tag key/value pairs.
fn count_resources_without_tags(inventory_groups: &[AzureInventoryGroup]) -> usize {
    // Start the running total at zero.
    let mut resources_without_tags = 0usize;

    // Walk through every resource and increment when tags are missing.
    for inventory_group in inventory_groups {
        for resource in &inventory_group.resources {
            if !resource_has_tags(resource) {
                resources_without_tags += 1;
            }
        }
    }

    // Return the final count.
    resources_without_tags
}

// Return `true` when a resource has at least one non-empty tag key.
fn resource_has_tags(resource: &AzureResourceReportItem) -> bool {
    // Stop early when Azure did not return a tags object.
    let Some(tags) = resource.tags.as_ref() else {
        return false;
    };

    // A single non-empty key is enough to treat the resource as tagged.
    for key in tags.keys() {
        if !key.trim().is_empty() {
            return true;
        }
    }

    // No non-empty keys were found.
    false
}

// Collect non-default regions from both resource-group and resource locations.
fn collect_non_standard_regions(inventory_groups: &[AzureInventoryGroup]) -> Vec<String> {
    // Normalize the configured default region once so comparisons stay case-insensitive.
    let normalized_default_region = DEFAULT_REGION.to_lowercase();
    // Use an ordered set so values remain unique and deterministic.
    let mut detected_regions: BTreeSet<String> = BTreeSet::new();

    // Inspect every resource group and every resource location.
    for inventory_group in inventory_groups {
        // Normalize the current resource-group location and insert when non-default.
        if let Some(group_region) = normalize_region_value(&inventory_group.resource_group.location)
        {
            if group_region != normalized_default_region {
                detected_regions.insert(group_region);
            }
        }

        // Normalize every resource location and insert when non-default.
        for resource in &inventory_group.resources {
            if let Some(resource_region) = normalize_region_value(&resource.location) {
                if resource_region != normalized_default_region {
                    detected_regions.insert(resource_region);
                }
            }
        }
    }

    // Convert the set into a serializable list with Markdown-safe values.
    detected_regions
        .into_iter()
        .map(|region| escape_markdown_text(&region))
        .collect()
}

// Build rendered group sections, including one table row per resource.
fn build_group_views(inventory_groups: &[AzureInventoryGroup]) -> Vec<InventoryGroupView> {
    // Prepare an output vector sized to the number of incoming groups.
    let mut group_views: Vec<InventoryGroupView> = Vec::with_capacity(inventory_groups.len());

    // Render each input group into one output view.
    for inventory_group in inventory_groups {
        // Build rendered resource rows for the current group.
        let mut resource_views: Vec<InventoryResourceView> =
            Vec::with_capacity(inventory_group.resources.len());

        // Render every resource into a Markdown-safe table row.
        for resource in &inventory_group.resources {
            // Normalize missing locations to a clear placeholder.
            let location = normalize_non_empty_value(&resource.location, "-");
            // Build the final SKU value using the agreed fallback chain.
            let sku = resolve_resource_sku(resource);
            // Build the final tags value as a readable `k=v` comma-separated string.
            let tags = format_resource_tags(resource);

            // Store one rendered table row.
            resource_views.push(InventoryResourceView {
                name: escape_markdown_table_cell(&resource.name),
                resource_type: escape_markdown_table_cell(&resource.resource_type),
                location: escape_markdown_table_cell(&location),
                sku: escape_markdown_table_cell(&sku),
                tags: escape_markdown_table_cell(&tags),
            });
        }

        // Store one rendered section for the current resource group.
        group_views.push(InventoryGroupView {
            name: escape_markdown_heading(&inventory_group.resource_group.name),
            location: escape_markdown_heading(&normalize_non_empty_value(
                &inventory_group.resource_group.location,
                "-",
            )),
            total_resources: inventory_group.resources.len(),
            resources: resource_views,
        });
    }

    // Return all rendered group sections.
    group_views
}

// Resolve the final SKU string using `sku.name -> kind -> -` fallback order.
fn resolve_resource_sku(resource: &AzureResourceReportItem) -> String {
    // Prefer the explicit SKU name when Azure provided one.
    if let Some(sku_name) = resource
        .sku
        .as_ref()
        .and_then(|sku| sku.name.as_ref())
        .map(|sku_name| sku_name.trim())
    {
        if !sku_name.is_empty() {
            return sku_name.to_owned();
        }
    }

    // Fall back to `kind` when `sku.name` is missing or empty.
    if let Some(kind) = resource.kind.as_ref().map(|kind| kind.trim()) {
        if !kind.is_empty() {
            return kind.to_owned();
        }
    }

    // Use a dash when neither SKU nor kind can provide a useful value.
    String::from("-")
}

// Build a readable comma-separated `key=value` list from Azure tags.
fn format_resource_tags(resource: &AzureResourceReportItem) -> String {
    // Stop early when Azure did not return a tags object.
    let Some(tags) = resource.tags.as_ref() else {
        return String::from("-");
    };

    // Collect `key=value` entries into a vector so we can sort them predictably.
    let mut tag_entries: Vec<String> = Vec::new();

    // Render each tag pair in readable `key=value` form.
    for (key, value) in tags {
        // Normalize the key once so empty-key and prefix checks use the same value.
        let normalized_key = key.trim();
        // Ignore empty keys because they are not meaningful to readers.
        if normalized_key.is_empty() {
            continue;
        }

        // Hide internal metadata tags that start with `hidden-` (case-insensitive).
        if normalized_key.to_lowercase().starts_with("hidden-") {
            continue;
        }

        // Store one normalized entry.
        tag_entries.push(format!("{}={}", normalized_key, value.trim()));
    }

    // Keep output stable by sorting tags alphabetically without case sensitivity.
    tag_entries.sort_by_key(|entry| entry.to_lowercase());

    // Return a dash when no usable tags were found.
    if tag_entries.is_empty() {
        return String::from("-");
    }

    // Join entries with comma+space so the value stays human-readable.
    tag_entries.join(", ")
}

// Normalize a string and return a fallback value when the normalized text is empty.
fn normalize_non_empty_value(value: &str, fallback: &str) -> String {
    // Remove leading and trailing whitespace first.
    let trimmed = value.trim();

    // Return the fallback when nothing meaningful remains.
    if trimmed.is_empty() {
        return String::from(fallback);
    }

    // Return the trimmed value as an owned string.
    trimmed.to_owned()
}

// Normalize a region string to lowercase and return `None` when empty.
fn normalize_region_value(value: &str) -> Option<String> {
    // Remove leading and trailing whitespace first.
    let trimmed = value.trim();

    // Skip empty region values so they do not pollute region summaries.
    if trimmed.is_empty() {
        return None;
    }

    // Return lowercase region names so comparisons become case-insensitive.
    Some(trimmed.to_lowercase())
}

// Resolve the final inventory output directory inside the user's home folder.
pub(crate) fn resolve_inventory_output_directory() -> AppResult<PathBuf> {
    // Resolve the shared CLI directory so inventory and config stay grouped together.
    let cli_directory = resolve_cli_directory()?;
    // Append the fixed inventory folder below the shared CLI directory.
    Ok(cli_directory.join("inventory"))
}

// Append the fixed `.martijn/cli/inventory` directory under a known home directory.
#[cfg(test)]
pub(crate) fn build_inventory_output_directory_from_home(home_directory: &Path) -> PathBuf {
    // Join path segments one by one so separators stay correct on every platform.
    home_directory
        .join(".martijn")
        .join("cli")
        .join("inventory")
}

// Build a unique report filename that includes a timestamp and a short UUID fragment.
pub(crate) fn build_inventory_file_name() -> String {
    // Capture the current UTC time for the timestamp part of the filename.
    let now = OffsetDateTime::now_utc();
    // Describe the compact timestamp format used in the filename.
    let file_name_format = format_description!("[year][month][day]-[hour][minute][second]");
    // Format the current time using the compact filename-safe representation.
    let timestamp = now
        .format(&file_name_format)
        .expect("filename timestamp formatting should succeed");
    // Generate a random UUID so repeated runs in the same second still stay unique.
    let unique_id = Uuid::new_v4().simple().to_string();
    // Keep only the first eight characters so the filename stays compact.
    let short_unique_id = &unique_id[..8];

    // Return the final filename with the required prefix and `.md` extension.
    format!("azure-inventory-{timestamp}-{short_unique_id}.md")
}

// Escape characters that could break Markdown headings or bullet content.
fn escape_markdown_text(value: &str) -> String {
    // Escape a small set of formatting characters that matter most in this report.
    value
        .replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('`', "\\`")
}

// Escape text used in section headings.
fn escape_markdown_heading(value: &str) -> String {
    // Reuse the standard Markdown text escaping for headings.
    escape_markdown_text(value)
}

// Escape values that are rendered inside Markdown table cells.
fn escape_markdown_table_cell(value: &str) -> String {
    // Escape table separators, formatting markers and multiline values.
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('`', "\\`")
        .replace('\r', "")
        .replace('\n', "<br>")
}

#[cfg(test)]
mod tests {
    // Import the Azure helpers that the tests verify.
    use super::{
        AzureAccount, AzureInventoryGroup, AzureResourceGroupReportItem, AzureResourceReportItem,
        build_inventory_file_name, build_inventory_output_directory_from_home,
        render_inventory_markdown, sort_resource_groups, sort_resources,
    };
    // Import the SKU helper model for test resource construction.
    use crate::azure::model::AzureResourceSkuReportItem;
    // Import `BTreeMap` so tests can build deterministic tag maps.
    use std::collections::BTreeMap;
    // Import `PathBuf` so path-building helpers can be tested with platform-aware values.
    use std::path::PathBuf;

    #[test]
    fn sorts_resource_groups_case_insensitively() {
        // Create an intentionally mixed-case list of resource groups.
        let mut resource_groups = vec![
            AzureResourceGroupReportItem {
                name: String::from("zeta-group"),
                location: String::from("westeurope"),
            },
            AzureResourceGroupReportItem {
                name: String::from("Alpha-group"),
                location: String::from("northeurope"),
            },
            AzureResourceGroupReportItem {
                name: String::from("beta-group"),
                location: String::from("swedencentral"),
            },
        ];

        // Sort the resource groups using the helper under test.
        sort_resource_groups(&mut resource_groups);

        // Confirm that alphabetical order ignores casing differences.
        assert_eq!(resource_groups[0].name, "Alpha-group");
        assert_eq!(resource_groups[1].name, "beta-group");
        assert_eq!(resource_groups[2].name, "zeta-group");
    }

    #[test]
    fn sorts_resources_by_type_then_name_case_insensitively() {
        // Create resources where type and name ordering can both be verified.
        let mut resources = vec![
            AzureResourceReportItem {
                name: String::from("vm-b"),
                resource_type: String::from("Microsoft.Compute/virtualMachines"),
                location: String::from("westeurope"),
                kind: None,
                tags: None,
                sku: None,
            },
            AzureResourceReportItem {
                name: String::from("vm-a"),
                resource_type: String::from("microsoft.compute/virtualMachines"),
                location: String::from("westeurope"),
                kind: None,
                tags: None,
                sku: None,
            },
            AzureResourceReportItem {
                name: String::from("app-z"),
                resource_type: String::from("Microsoft.Web/sites"),
                location: String::from("westeurope"),
                kind: None,
                tags: None,
                sku: None,
            },
            AzureResourceReportItem {
                name: String::from("app-a"),
                resource_type: String::from("Microsoft.Web/sites"),
                location: String::from("westeurope"),
                kind: None,
                tags: None,
                sku: None,
            },
        ];

        // Sort resources with the helper under test.
        sort_resources(&mut resources);

        // Confirm that compute resources appear before web resources.
        assert_eq!(
            resources[0].resource_type,
            "microsoft.compute/virtualMachines"
        );
        assert_eq!(
            resources[1].resource_type,
            "Microsoft.Compute/virtualMachines"
        );
        // Confirm name ordering within the same type.
        assert_eq!(resources[0].name, "vm-a");
        assert_eq!(resources[1].name, "vm-b");
        assert_eq!(resources[2].name, "app-a");
        assert_eq!(resources[3].name, "app-z");
    }

    #[test]
    fn inventory_file_name_uses_expected_prefix_and_extension() {
        // Build one generated filename.
        let file_name = build_inventory_file_name();

        // Confirm that the filename uses the expected prefix.
        assert!(file_name.starts_with("azure-inventory-"));
        // Confirm that the filename ends with the Markdown extension.
        assert!(file_name.ends_with(".md"));
    }

    #[test]
    fn render_inventory_markdown_uses_new_layout_sections() {
        // Create a sample account for metadata rendering.
        let account = AzureAccount {
            name: String::from("Prod"),
            subscription_id: String::from("sub-123"),
            user: String::from("user-123"),
        };

        // Build one tagged resource so table rendering can be verified.
        let mut tagged_resource_tags = BTreeMap::new();
        tagged_resource_tags.insert(String::from("env"), String::from("prod"));

        // Create two resource groups, one with resources and one empty.
        let inventory_groups = vec![
            AzureInventoryGroup {
                resource_group: AzureResourceGroupReportItem {
                    name: String::from("rg-app-prod"),
                    location: String::from("westeurope"),
                },
                resources: vec![
                    AzureResourceReportItem {
                        name: String::from("app-api"),
                        resource_type: String::from("Microsoft.Web/sites"),
                        location: String::from("westeurope"),
                        kind: None,
                        tags: Some(tagged_resource_tags),
                        sku: Some(AzureResourceSkuReportItem {
                            name: Some(String::from("P1v3")),
                        }),
                    },
                    AzureResourceReportItem {
                        name: String::from("storage"),
                        resource_type: String::from("Microsoft.Storage/storageAccounts"),
                        location: String::from("northeurope"),
                        kind: Some(String::from("StorageV2")),
                        tags: None,
                        sku: None,
                    },
                ],
            },
            AzureInventoryGroup {
                resource_group: AzureResourceGroupReportItem {
                    name: String::from("rg-empty"),
                    location: String::from("westeurope"),
                },
                resources: Vec::new(),
            },
        ];

        // Render the Markdown inventory for the sample data.
        let markdown = render_inventory_markdown(&account, &inventory_groups, 2)
            .expect("inventory markdown should render successfully");

        // Confirm that the new layout starts with the expected heading.
        assert!(markdown.starts_with("# Azure Inventory\n\n"));
        // Confirm that YAML front matter is no longer present.
        assert!(!markdown.starts_with("---\n"));
        // Confirm that metadata bullets are rendered.
        assert!(markdown.contains("- Generated: "));
        assert!(markdown.contains("- Subscription: Prod (sub-123)"));
        // Confirm that summary sections are present.
        assert!(markdown.contains("## Summary"));
        assert!(markdown.contains("### Resources per type"));
        assert!(markdown.contains("### Resources per region"));
        assert!(markdown.contains("### Empty resource groups"));
        assert!(markdown.contains("### Signals"));
        // Confirm that group sections and table headers are present.
        assert!(markdown.contains("## rg-app-prod (westeurope)"));
        assert!(markdown.contains("| Name | Type | Location | SKU | Tags |"));
        // Confirm that the first row is rendered and that no blank line splits it from separator.
        assert!(
            markdown.contains("| app-api | Microsoft.Web/sites | westeurope | P1v3 | env=prod |")
        );
        assert!(
            !markdown.contains(
                "| Name | Type | Location | SKU | Tags |\n|------|------|----------|-----|------|\n\n| app-api | Microsoft.Web/sites | westeurope | P1v3 | env=prod |"
            )
        );
    }

    #[test]
    fn render_inventory_markdown_formats_tags_with_fallback_dash() {
        // Create a sample account for metadata rendering.
        let account = AzureAccount {
            name: String::from("Prod"),
            subscription_id: String::from("sub-123"),
            user: String::from("user-123"),
        };

        // Create deterministic tags so ordering can be asserted.
        let mut tags = BTreeMap::new();
        tags.insert(String::from("owner"), String::from("team-a"));
        tags.insert(String::from("env"), String::from("prod"));
        tags.insert(
            String::from("hidden-link:/subscriptions/test"),
            String::from("Resource"),
        );

        // Create one group with one tagged and one untagged resource.
        let inventory_groups = vec![AzureInventoryGroup {
            resource_group: AzureResourceGroupReportItem {
                name: String::from("rg-demo"),
                location: String::from("westeurope"),
            },
            resources: vec![
                AzureResourceReportItem {
                    name: String::from("tagged"),
                    resource_type: String::from("Microsoft.Web/sites"),
                    location: String::from("westeurope"),
                    kind: None,
                    tags: Some(tags),
                    sku: None,
                },
                AzureResourceReportItem {
                    name: String::from("untagged"),
                    resource_type: String::from("Microsoft.Storage/storageAccounts"),
                    location: String::from("westeurope"),
                    kind: None,
                    tags: None,
                    sku: None,
                },
            ],
        }];

        // Render the Markdown inventory for the sample data.
        let markdown = render_inventory_markdown(&account, &inventory_groups, 2)
            .expect("inventory markdown should render successfully");

        // Confirm that tags render as readable `k=v` values.
        assert!(markdown.contains("env=prod, owner=team-a"));
        // Confirm that `hidden-*` tags are removed from rendered output.
        assert!(!markdown.contains("hidden-link:/subscriptions/test=Resource"));
        // Confirm that missing tags are rendered as a dash.
        assert!(
            markdown
                .contains("| untagged | Microsoft.Storage/storageAccounts | westeurope | - | - |")
        );
    }

    #[test]
    fn render_inventory_markdown_keeps_resource_rows_on_separate_table_lines() {
        // Create a sample account for metadata rendering.
        let account = AzureAccount {
            name: String::from("Prod"),
            subscription_id: String::from("sub-123"),
            user: String::from("user-123"),
        };

        // Create one group with two resources that should render as two table rows.
        let inventory_groups = vec![AzureInventoryGroup {
            resource_group: AzureResourceGroupReportItem {
                name: String::from("rg-demo"),
                location: String::from("westeurope"),
            },
            resources: vec![
                AzureResourceReportItem {
                    name: String::from("resource-a"),
                    resource_type: String::from("Microsoft.Web/sites"),
                    location: String::from("westeurope"),
                    kind: None,
                    tags: None,
                    sku: None,
                },
                AzureResourceReportItem {
                    name: String::from("resource-b"),
                    resource_type: String::from("Microsoft.Web/sites"),
                    location: String::from("westeurope"),
                    kind: None,
                    tags: None,
                    sku: None,
                },
            ],
        }];

        // Render the Markdown inventory for the sample data.
        let markdown = render_inventory_markdown(&account, &inventory_groups, 2)
            .expect("inventory markdown should render successfully");

        // Confirm that each resource appears as its own markdown table row.
        assert!(markdown.contains("| resource-a | Microsoft.Web/sites | westeurope | - | - |"));
        assert!(markdown.contains("| resource-b | Microsoft.Web/sites | westeurope | - | - |"));
        // Confirm that no blank line appears between adjacent resource rows.
        assert!(
            !markdown.contains(
                "| resource-a | Microsoft.Web/sites | westeurope | - | - |\n\n| resource-b | Microsoft.Web/sites | westeurope | - | - |"
            )
        );
    }

    #[test]
    fn render_inventory_markdown_uses_dash_when_only_hidden_tags_exist() {
        // Create a sample account for metadata rendering.
        let account = AzureAccount {
            name: String::from("Prod"),
            subscription_id: String::from("sub-123"),
            user: String::from("user-123"),
        };

        // Create tags that should all be filtered out by the `hidden-*` rule.
        let mut hidden_only_tags = BTreeMap::new();
        hidden_only_tags.insert(
            String::from("hidden-link:/subscriptions/test"),
            String::from("Resource"),
        );
        hidden_only_tags.insert(String::from("HIDDEN-TRACE-ID"), String::from("trace-123"));

        // Create one group with one resource that has only hidden tags.
        let inventory_groups = vec![AzureInventoryGroup {
            resource_group: AzureResourceGroupReportItem {
                name: String::from("rg-demo"),
                location: String::from("westeurope"),
            },
            resources: vec![AzureResourceReportItem {
                name: String::from("resource-hidden-only"),
                resource_type: String::from("Microsoft.Web/sites"),
                location: String::from("westeurope"),
                kind: None,
                tags: Some(hidden_only_tags),
                sku: None,
            }],
        }];

        // Render the Markdown inventory for the sample data.
        let markdown = render_inventory_markdown(&account, &inventory_groups, 1)
            .expect("inventory markdown should render successfully");

        // Confirm that the tags column falls back to a dash after filtering hidden tags.
        assert!(
            markdown
                .contains("| resource-hidden-only | Microsoft.Web/sites | westeurope | - | - |")
        );
    }

    #[test]
    fn render_inventory_markdown_lists_non_standard_regions_from_groups_and_resources() {
        // Create a sample account for metadata rendering.
        let account = AzureAccount {
            name: String::from("Prod"),
            subscription_id: String::from("sub-123"),
            user: String::from("user-123"),
        };

        // Create groups where one non-standard region exists on group level and one on resource level.
        let inventory_groups = vec![
            AzureInventoryGroup {
                resource_group: AzureResourceGroupReportItem {
                    name: String::from("rg-one"),
                    location: String::from("westeurope"),
                },
                resources: vec![AzureResourceReportItem {
                    name: String::from("resource-one"),
                    resource_type: String::from("Microsoft.Web/sites"),
                    location: String::from("northeurope"),
                    kind: None,
                    tags: None,
                    sku: None,
                }],
            },
            AzureInventoryGroup {
                resource_group: AzureResourceGroupReportItem {
                    name: String::from("rg-two"),
                    location: String::from("swedencentral"),
                },
                resources: Vec::new(),
            },
        ];

        // Render the Markdown inventory for the sample data.
        let markdown = render_inventory_markdown(&account, &inventory_groups, 1)
            .expect("inventory markdown should render successfully");

        // Confirm that both non-standard regions are listed and default region is excluded.
        assert!(markdown.contains("Non-standard regions detected: northeurope, swedencentral"));
    }

    #[test]
    fn render_inventory_markdown_shows_total_resources_per_group() {
        // Create a sample account for metadata rendering.
        let account = AzureAccount {
            name: String::from("Prod"),
            subscription_id: String::from("sub-123"),
            user: String::from("user-123"),
        };

        // Create one group with exactly two resources.
        let inventory_groups = vec![AzureInventoryGroup {
            resource_group: AzureResourceGroupReportItem {
                name: String::from("rg-demo"),
                location: String::from("westeurope"),
            },
            resources: vec![
                AzureResourceReportItem {
                    name: String::from("resource-a"),
                    resource_type: String::from("Microsoft.Web/sites"),
                    location: String::from("westeurope"),
                    kind: None,
                    tags: None,
                    sku: None,
                },
                AzureResourceReportItem {
                    name: String::from("resource-b"),
                    resource_type: String::from("Microsoft.Storage/storageAccounts"),
                    location: String::from("westeurope"),
                    kind: None,
                    tags: None,
                    sku: None,
                },
            ],
        }];

        // Render the Markdown inventory for the sample data.
        let markdown = render_inventory_markdown(&account, &inventory_groups, 2)
            .expect("inventory markdown should render successfully");

        // Confirm that the group section shows the total resource count.
        assert!(markdown.contains("- Total resources: 2"));
    }

    #[test]
    fn output_directory_appends_inventory_segments_under_home() {
        // Create a Unix-like sample home directory.
        let home_directory = PathBuf::from("/home/martijn");

        // Build the final inventory directory from the sample home path.
        let output_directory = build_inventory_output_directory_from_home(&home_directory);

        // Confirm that the fixed inventory path is appended correctly.
        assert_eq!(
            output_directory,
            PathBuf::from("/home/martijn")
                .join(".martijn")
                .join("cli")
                .join("inventory")
        );
    }

    #[test]
    fn output_directory_keeps_windows_style_home_paths_pathbuf_safe() {
        // Create a Windows-style sample home directory.
        let home_directory = PathBuf::from(r"C:\Users\Martijn");

        // Build the final inventory directory from the sample home path.
        let output_directory = build_inventory_output_directory_from_home(&home_directory);

        // Confirm that the helper appends the same logical path segments.
        assert_eq!(
            output_directory,
            PathBuf::from(r"C:\Users\Martijn")
                .join(".martijn")
                .join("cli")
                .join("inventory")
        );
    }
}
