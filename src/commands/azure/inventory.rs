// Import the Azure service layer so inventory commands can delegate real Azure work.
use crate::azure::service::{
    ArtifactMatch, InventoryReportKind, delete_inventory_report, inventory_reports_directory,
    list_inventory_reports, read_inventory_report, render_inventory_groups_list,
    render_inventory_resources_list, render_inventory_resources_tree,
    render_saved_inventory_markdown, render_saved_inventory_resources_list_markdown,
    render_saved_inventory_resources_tree_markdown, save_inventory_report_text,
};

// Import the parsed save option so handlers can decide whether to write a report.
use super::cli::SaveArguments;

// Import the Azure session state type so the runner can keep one consistent handler signature.
use super::state::SessionState;

// Print Azure resources and optionally save the output as Markdown.
pub(super) async fn handle_inventory_resources_list(
    _state: &mut SessionState,
    arguments: &SaveArguments,
) {
    // Ask the service layer to build output from the selected local snapshot.
    match render_inventory_resources_list(arguments.snapshot.as_deref()) {
        Ok(output) => {
            // Print the default stdout output for this inventory command.
            print!("{output}");
            // Save the rich Markdown report when the user provided `--save`.
            save_inventory_resources_list_output_if_requested(arguments);
        }
        Err(error) => {
            // Explain clearly why the inventory command could not complete.
            println!("Unable to list Azure resources: {error}");
        }
    }
}

// Print Azure resources as a tree and optionally save the output as Markdown.
pub(super) async fn handle_inventory_resources_tree(
    _state: &mut SessionState,
    arguments: &SaveArguments,
) {
    // Ask the service layer to build output from the selected local snapshot.
    match render_inventory_resources_tree(arguments.snapshot.as_deref()) {
        Ok(output) => {
            // Print the default stdout output for this inventory command.
            print!("{output}");
            // Save the plain tree as a Markdown report when the user provided `--save`.
            save_inventory_tree_output_if_requested(
                InventoryReportKind::ResourcesTree,
                arguments,
                &output,
            );
        }
        Err(error) => {
            // Explain clearly why the inventory command could not complete.
            println!("Unable to build the Azure resource tree: {error}");
        }
    }
}

// Print Azure resource groups and optionally save the output as Markdown.
pub(super) async fn handle_inventory_groups_list(
    _state: &mut SessionState,
    arguments: &SaveArguments,
) {
    // Ask the service layer to build output from the selected local snapshot.
    match render_inventory_groups_list(arguments.snapshot.as_deref()) {
        Ok(output) => {
            // Print the default stdout output for this inventory command.
            print!("{output}");
            // Save the output when the user provided `--save`.
            save_inventory_output_if_requested(
                InventoryReportKind::GroupsList,
                "Resource Groups List",
                arguments,
                &output,
            );
        }
        Err(error) => {
            // Explain clearly why the inventory command could not complete.
            println!("Unable to list resource groups: {error}");
        }
    }
}

// List saved inventory reports from all local inventory report directories.
pub(super) fn handle_report_list() {
    // Ask the service layer for the filtered, newest-first report list.
    let reports = match list_inventory_reports() {
        Ok(reports) => reports,
        Err(error) => {
            // Show the concrete filesystem problem when listing cannot continue.
            println!("Unable to list inventory reports: {error}");
            return;
        }
    };

    // Stop early with a friendly message when there is nothing to show.
    if reports.is_empty() {
        // Resolve the directory for the message so the user knows where reports are expected.
        match inventory_reports_directory() {
            Ok(directory) => {
                // Include the path because it is the most useful next place to inspect.
                println!("No inventory reports found below {}.", directory.display());
            }
            Err(error) => {
                // Fall back to the underlying path error when even the directory cannot resolve.
                println!("Unable to resolve the inventory directory: {error}");
            }
        }
        return;
    }

    // Print every report path in the newest-first order returned by the service layer.
    for report in reports {
        // Display file size and path so users can identify the report quickly.
        println!("{} bytes  {}", report.size_bytes, report.path.display());
    }
}

// Show one saved inventory report.
pub(super) fn handle_report_show(name: &str) {
    // Ask the service layer to resolve and read the requested report.
    match read_inventory_report(name) {
        Ok(ArtifactMatch::One(report_body)) => {
            // Print the saved Markdown exactly as stored.
            print!("{report_body}");
        }
        Ok(ArtifactMatch::Many(file_names)) => {
            // Explain that deleting or showing an arbitrary match would be unsafe.
            println!("Multiple inventory reports match `{name}`:");
            // Show every matching option so the user can retry with a clearer name.
            for file_name in file_names {
                println!("{file_name}");
            }
        }
        Ok(ArtifactMatch::None) => {
            // Tell the user no report matched the provided name.
            println!("No inventory report found for `{name}`.");
        }
        Err(error) => {
            // Show the concrete filesystem problem when reading cannot continue.
            println!("Unable to show inventory report: {error}");
        }
    }
}

// Delete one saved inventory report.
pub(super) fn handle_report_delete(name: &str) {
    // Ask the service layer to resolve and delete the requested report.
    match delete_inventory_report(name) {
        Ok(ArtifactMatch::One(path)) => {
            // Confirm the exact file that was deleted.
            println!("Deleted inventory report {}", path.display());
        }
        Ok(ArtifactMatch::Many(paths)) => {
            // Explain that deleting an arbitrary match would be unsafe.
            println!("Multiple inventory reports match `{name}`:");
            // Show every matching option so the user can retry with a clearer name.
            for path in paths {
                println!("{}", path.display());
            }
        }
        Ok(ArtifactMatch::None) => {
            // Tell the user no report matched the provided name.
            println!("No inventory report found for `{name}`.");
        }
        Err(error) => {
            // Show the concrete filesystem problem when deleting cannot continue.
            println!("Unable to delete inventory report: {error}");
        }
    }
}

// Save the resource list report through the dedicated Markdown template when requested.
fn save_inventory_resources_list_output_if_requested(arguments: &SaveArguments) {
    // Stop early when the user did not request a saved report.
    let Some(requested_name) = arguments.save.as_deref() else {
        return;
    };

    // Treat the empty `--save` value as an automatic generated file name.
    let requested_name = if requested_name.trim().is_empty() {
        None
    } else {
        Some(requested_name)
    };

    // Render the full resources-list Markdown report from the selected snapshot.
    let markdown =
        match render_saved_inventory_resources_list_markdown(arguments.snapshot.as_deref()) {
            Ok(markdown) => markdown,
            Err(error) => {
                // Explain clearly why saving failed while keeping stdout behavior intact.
                println!("Unable to save the inventory report: {error}");
                return;
            }
        };

    // Ask the service layer to write the report to the resources/list directory.
    match save_inventory_report_text(
        InventoryReportKind::ResourcesList,
        requested_name,
        &markdown,
    ) {
        Ok(path) => {
            // Confirm success and show the final path to the newly created report.
            println!("Inventory report saved to {}", path.display());
        }
        Err(error) => {
            // Explain clearly why saving failed while keeping stdout behavior intact.
            println!("Unable to save the inventory report: {error}");
        }
    }
}

// Save inventory output as Markdown when `--save` was provided.
fn save_inventory_output_if_requested(
    report_kind: InventoryReportKind,
    title: &str,
    arguments: &SaveArguments,
    output: &str,
) {
    // Stop early when the user did not request a saved report.
    let Some(requested_name) = arguments.save.as_deref() else {
        return;
    };

    // Treat the empty `--save` value as an automatic generated file name.
    let requested_name = if requested_name.trim().is_empty() {
        None
    } else {
        Some(requested_name)
    };
    // Wrap the terminal output in a small Markdown document.
    let markdown = render_saved_inventory_markdown(title, output);

    // Ask the service layer to write the report to the correct directory.
    match save_inventory_report_text(report_kind, requested_name, &markdown) {
        Ok(path) => {
            // Confirm success and show the final path to the newly created report.
            println!("Inventory report saved to {}", path.display());
        }
        Err(error) => {
            // Explain clearly why saving failed while keeping stdout behavior intact.
            println!("Unable to save the inventory report: {error}");
        }
    }
}

// Save plain inventory tree output as Markdown when `--save` was provided.
fn save_inventory_tree_output_if_requested(
    report_kind: InventoryReportKind,
    arguments: &SaveArguments,
    output: &str,
) {
    // Stop early when the user did not request a saved report.
    let Some(requested_name) = arguments.save.as_deref() else {
        return;
    };

    // Treat the empty `--save` value as an automatic generated file name.
    let requested_name = if requested_name.trim().is_empty() {
        None
    } else {
        Some(requested_name)
    };
    // Render a Markdown report from the snapshot metadata and the exact tree printed to stdout.
    let markdown =
        match render_saved_inventory_resources_tree_markdown(arguments.snapshot.as_deref(), output)
        {
            Ok(markdown) => markdown,
            Err(error) => {
                // Explain clearly why saving failed while keeping stdout behavior intact.
                println!("Unable to save the inventory report: {error}");
                return;
            }
        };

    // Ask the service layer to write the report to the correct directory.
    match save_inventory_report_text(report_kind, requested_name, &markdown) {
        Ok(path) => {
            // Confirm success and show the final path to the newly created report.
            println!("Inventory report saved to {}", path.display());
        }
        Err(error) => {
            // Explain clearly why saving failed while keeping stdout behavior intact.
            println!("Unable to save the inventory report: {error}");
        }
    }
}
