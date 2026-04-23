// Import the Azure data model types used by the renderer.
use crate::azure::model::{
    AzureAccount, AzureInventoryGroup, AzureResourceGroupReportItem, AzureResourceReportItem,
};
// Import the shared config-path helper so inventory and config live under the same base folder.
use crate::config::resolve_cli_directory;
// Import the shared application result type used by path helpers.
use crate::AppResult;
// Import `PathBuf` so resolved filesystem locations stay platform-aware.
use std::path::PathBuf;
// Import `Path` for the test-only helper that assembles inventory directories from a home path.
#[cfg(test)]
use std::path::Path;
// Import time formatting helpers so the report can store readable timestamps.
use time::OffsetDateTime;
// Import the RFC 3339 formatter for YAML metadata timestamps.
use time::format_description::well_known::Rfc3339;
// Import the custom filename formatter macro for compact timestamp-based filenames.
use time::macros::format_description;
// Import `Uuid` so we can generate a short unique filename suffix.
use uuid::Uuid;

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

// Sort resources alphabetically without making upper/lower case matter.
pub(crate) fn sort_resources(resources: &mut [AzureResourceReportItem]) {
    // Compare lowercase versions so names sort the same regardless of original casing.
    resources.sort_by_key(|resource| resource.name.to_lowercase());
}

// Render the full Markdown document, including YAML front matter and grouped sections.
pub(crate) fn render_inventory_markdown(
    account: &AzureAccount,
    inventory_groups: &[AzureInventoryGroup],
    total_resource_count: usize,
) -> String {
    // Capture the current UTC time once so the metadata and body stay in sync.
    let generated_at = OffsetDateTime::now_utc();
    // Format the timestamp as RFC 3339 because that format is widely understood by tooling.
    let generated_at_rfc3339 = generated_at
        .format(&Rfc3339)
        .expect("RFC 3339 formatting should succeed");

    // Start with an empty `String` that we fill step by step for readability.
    let mut markdown = String::new();

    // Start the YAML front matter block.
    markdown.push_str("---\n");
    // Add the document title metadata.
    markdown.push_str("title: \"Azure Resource Inventory\"\n");
    // Store the generation timestamp in a machine-friendly format.
    markdown.push_str(&format!(
        "generated_at: \"{}\"\n",
        escape_yaml_double_quoted_string(&generated_at_rfc3339)
    ));
    // Store the CLI command that produced this file.
    markdown.push_str("generator: \"martijn azure inventory\"\n");
    // Store the friendly subscription name.
    markdown.push_str(&format!(
        "subscription_name: \"{}\"\n",
        escape_yaml_double_quoted_string(&account.name)
    ));
    // Store the active subscription identifier.
    markdown.push_str(&format!(
        "subscription_id: \"{}\"\n",
        escape_yaml_double_quoted_string(&account.subscription_id)
    ));
    // Store the active Azure user or principal.
    markdown.push_str(&format!(
        "azure_user: \"{}\"\n",
        escape_yaml_double_quoted_string(&account.user)
    ));
    // Store how many resource groups were included in this report.
    markdown.push_str(&format!(
        "resource_group_count: {}\n",
        inventory_groups.len()
    ));
    // Store how many resources were included in total.
    markdown.push_str(&format!("resource_count: {}\n", total_resource_count));
    // Store the sort order so consumers know how the report was organized.
    markdown.push_str("sort_order: \"resource_group:name,resource:name\"\n");
    // Store a simple format version for future compatibility.
    markdown.push_str("format_version: 1\n");
    // Close the YAML front matter block.
    markdown.push_str("---\n\n");

    // Add the top-level document title.
    markdown.push_str("# Azure Resource Inventory\n\n");
    // Add a short summary line with the generation timestamp.
    markdown.push_str(&format!(
        "Generated on `{}` for subscription **{}** (`{}`).\n\n",
        generated_at_rfc3339,
        escape_markdown_text(&account.name),
        escape_markdown_code_span(&account.subscription_id)
    ));

    // Render every resource group as its own Markdown section.
    for inventory_group in inventory_groups {
        // Add the resource group heading.
        markdown.push_str(&format!(
            "## {}\n\n",
            escape_markdown_heading(&inventory_group.resource_group.name)
        ));
        // Add one summary line for the group's location and resource count.
        markdown.push_str(&format!(
            "Location: `{}`  \nResources: {}\n\n",
            escape_markdown_code_span(&inventory_group.resource_group.location),
            inventory_group.resources.len()
        ));

        // Show a clear placeholder when the group does not contain resources.
        if inventory_group.resources.is_empty() {
            markdown.push_str("_No resources found._\n\n");
            continue;
        }

        // Render every resource as a bullet point.
        for resource in &inventory_group.resources {
            // Start the bullet with the bold resource name.
            markdown.push_str(&format!("- **{}**", escape_markdown_text(&resource.name)));
            // Add the resource type after the name.
            markdown.push_str(&format!(
                " - type: `{}`",
                escape_markdown_code_span(&resource.resource_type)
            ));
            // Add the resource location after the type.
            markdown.push_str(&format!(
                ", location: `{}`",
                escape_markdown_code_span(&resource.location)
            ));

            // Add the optional kind only when Azure returned one.
            if let Some(kind) = &resource.kind {
                // Skip empty or whitespace-only kinds because they do not add useful information.
                if !kind.trim().is_empty() {
                    markdown.push_str(&format!(", kind: `{}`", escape_markdown_code_span(kind)));
                }
            }

            // Finish the bullet line.
            markdown.push('\n');
        }

        // Separate resource-group sections with one blank line.
        markdown.push('\n');
    }

    // Return the completed Markdown document.
    markdown
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

// Escape a value so it stays safe inside a YAML double-quoted string.
fn escape_yaml_double_quoted_string(value: &str) -> String {
    // Replace backslashes first so later escapes do not get reinterpreted.
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
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

// Escape text that is rendered inside one inline code span.
fn escape_markdown_code_span(value: &str) -> String {
    // Replace backticks so they do not accidentally close the code span early.
    value.replace('`', "\\`")
}

// Reuse the plain Markdown text escaping for headings as well.
fn escape_markdown_heading(value: &str) -> String {
    // Forward to the general Markdown text escaper to keep behavior consistent.
    escape_markdown_text(value)
}

#[cfg(test)]
mod tests {
    // Import the Azure helpers that the tests verify.
    use super::{
        AzureAccount, AzureInventoryGroup, AzureResourceGroupReportItem, AzureResourceReportItem,
        build_inventory_file_name, build_inventory_output_directory_from_home,
        render_inventory_markdown, sort_resource_groups, sort_resources,
    };
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
    fn sorts_resources_case_insensitively() {
        // Create an intentionally mixed-case list of resources.
        let mut resources = vec![
            AzureResourceReportItem {
                name: String::from("vm-two"),
                resource_type: String::from("Microsoft.Compute/virtualMachines"),
                location: String::from("westeurope"),
                kind: None,
            },
            AzureResourceReportItem {
                name: String::from("App-One"),
                resource_type: String::from("Microsoft.Web/sites"),
                location: String::from("northeurope"),
                kind: Some(String::from("app")),
            },
            AzureResourceReportItem {
                name: String::from("storage-three"),
                resource_type: String::from("Microsoft.Storage/storageAccounts"),
                location: String::from("westeurope"),
                kind: None,
            },
        ];

        // Sort the resources using the helper under test.
        sort_resources(&mut resources);

        // Confirm that alphabetical order ignores casing differences.
        assert_eq!(resources[0].name, "App-One");
        assert_eq!(resources[1].name, "storage-three");
        assert_eq!(resources[2].name, "vm-two");
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
    fn render_inventory_markdown_includes_front_matter_and_sections() {
        // Create a sample account for the metadata block.
        let account = AzureAccount {
            name: String::from("Demo Subscription"),
            subscription_id: String::from("sub-123"),
            user: String::from("martijn@example.com"),
        };
        // Create one sample inventory section with one resource.
        let inventory_groups = vec![AzureInventoryGroup {
            resource_group: AzureResourceGroupReportItem {
                name: String::from("rg-demo"),
                location: String::from("westeurope"),
            },
            resources: vec![AzureResourceReportItem {
                name: String::from("app-demo"),
                resource_type: String::from("Microsoft.Web/sites"),
                location: String::from("westeurope"),
                kind: Some(String::from("app")),
            }],
        }];

        // Render the Markdown inventory for the sample data.
        let markdown = render_inventory_markdown(&account, &inventory_groups, 1);

        // Confirm that the document starts with YAML front matter.
        assert!(markdown.starts_with("---\n"));
        // Confirm that the title metadata is present.
        assert!(markdown.contains("title: \"Azure Resource Inventory\""));
        // Confirm that the body contains the expected top-level heading.
        assert!(markdown.contains("# Azure Resource Inventory"));
        // Confirm that the resource-group section heading is present.
        assert!(markdown.contains("## rg-demo"));
        // Confirm that the rendered resource entry includes the optional kind.
        assert!(markdown.contains("kind: `app`"));
    }

    #[test]
    fn render_inventory_markdown_omits_kind_when_missing() {
        // Create a sample account for the metadata block.
        let account = AzureAccount {
            name: String::from("Demo Subscription"),
            subscription_id: String::from("sub-123"),
            user: String::from("martijn@example.com"),
        };
        // Create one sample inventory section with one resource that has no kind.
        let inventory_groups = vec![AzureInventoryGroup {
            resource_group: AzureResourceGroupReportItem {
                name: String::from("rg-demo"),
                location: String::from("westeurope"),
            },
            resources: vec![AzureResourceReportItem {
                name: String::from("storage-demo"),
                resource_type: String::from("Microsoft.Storage/storageAccounts"),
                location: String::from("westeurope"),
                kind: None,
            }],
        }];

        // Render the Markdown inventory for the sample data.
        let markdown = render_inventory_markdown(&account, &inventory_groups, 1);

        // Confirm that no empty `kind` fragment was rendered.
        assert!(!markdown.contains("kind:"));
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
