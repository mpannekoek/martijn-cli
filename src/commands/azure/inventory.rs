// Import the Azure service layer so inventory commands can delegate real Azure work.
use crate::azure::service::{
    ArtifactMatch, InventoryReportKind, delete_inventory_report, inventory_reports_directory,
    list_inventory_reports, read_inventory_report, render_inventory_groups_list,
    render_inventory_resources_list, render_inventory_resources_tree,
    render_saved_inventory_markdown, save_inventory_report_text,
};

// Import the parsed save option so handlers can decide whether to write a report.
use super::cli::SaveArguments;

// Import the Azure session state so inventory commands can check the active account.
use super::state::{SessionState, refresh_session_state};

// Print Azure resources and optionally save the output as Markdown.
pub(super) async fn handle_inventory_resources_list(
    state: &mut SessionState,
    arguments: &SaveArguments,
) {
    // Refresh the login state first so the command works with the latest Azure session.
    refresh_session_state(state).await;

    // Stop early when no Azure account is active.
    let Some(_) = state.account.as_ref() else {
        println!("Not logged in to Azure CLI. Run `login <tenant>` first.");
        return;
    };

    // Ask the service layer to build the terminal output.
    match render_inventory_resources_list().await {
        Ok(output) => {
            // Print the default stdout output for this inventory command.
            print!("{output}");
            // Save the output when the user provided `--save`.
            save_inventory_output_if_requested(
                InventoryReportKind::ResourcesList,
                "Azure Resources List",
                arguments,
                &output,
            );
        }
        Err(error) => {
            // Explain clearly why the inventory command could not complete.
            println!("Unable to list Azure resources: {error}");
        }
    }
}

// Print Azure resources as a tree and optionally save the output as Markdown.
pub(super) async fn handle_inventory_resources_tree(
    state: &mut SessionState,
    arguments: &SaveArguments,
) {
    // Refresh the login state first so the command works with the latest Azure session.
    refresh_session_state(state).await;

    // Stop early when no Azure account is active.
    let Some(_) = state.account.as_ref() else {
        println!("Not logged in to Azure CLI. Run `login <tenant>` first.");
        return;
    };

    // Ask the service layer to build the terminal output.
    match render_inventory_resources_tree().await {
        Ok(output) => {
            // Print the default stdout output for this inventory command.
            print!("{output}");
            // Save the output when the user provided `--save`.
            save_inventory_output_if_requested(
                InventoryReportKind::ResourcesTree,
                "Azure Resources Tree",
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
    state: &mut SessionState,
    arguments: &SaveArguments,
) {
    // Refresh the login state first so the command works with the latest Azure session.
    refresh_session_state(state).await;

    // Stop early when no Azure account is active.
    let Some(_) = state.account.as_ref() else {
        println!("Not logged in to Azure CLI. Run `login <tenant>` first.");
        return;
    };

    // Ask the service layer to build the terminal output.
    match render_inventory_groups_list().await {
        Ok(output) => {
            // Print the default stdout output for this inventory command.
            print!("{output}");
            // Save the output when the user provided `--save`.
            save_inventory_output_if_requested(
                InventoryReportKind::GroupsList,
                "Azure Resource Groups List",
                arguments,
                &output,
            );
        }
        Err(error) => {
            // Explain clearly why the inventory command could not complete.
            println!("Unable to list Azure resource groups: {error}");
        }
    }
}

// List saved Azure inventory reports from all local inventory report directories.
pub(super) fn handle_report_list() {
    // Ask the service layer for the filtered, newest-first report list.
    let reports = match list_inventory_reports() {
        Ok(reports) => reports,
        Err(error) => {
            // Show the concrete filesystem problem when listing cannot continue.
            println!("Unable to list Azure inventory reports: {error}");
            return;
        }
    };

    // Stop early with a friendly message when there is nothing to show.
    if reports.is_empty() {
        // Resolve the directory for the message so the user knows where reports are expected.
        match inventory_reports_directory() {
            Ok(directory) => {
                // Include the path because it is the most useful next place to inspect.
                println!(
                    "No Azure inventory reports found below {}.",
                    directory.display()
                );
            }
            Err(error) => {
                // Fall back to the underlying path error when even the directory cannot resolve.
                println!("Unable to resolve the Azure inventory directory: {error}");
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

// Show one saved Azure inventory report.
pub(super) fn handle_report_show(name: &str) {
    // Ask the service layer to resolve and read the requested report.
    match read_inventory_report(name) {
        Ok(ArtifactMatch::One(report_body)) => {
            // Print the saved Markdown exactly as stored.
            print!("{report_body}");
        }
        Ok(ArtifactMatch::Many(file_names)) => {
            // Explain that deleting or showing an arbitrary match would be unsafe.
            println!("Multiple Azure inventory reports match `{name}`:");
            // Show every matching option so the user can retry with a clearer name.
            for file_name in file_names {
                println!("{file_name}");
            }
        }
        Ok(ArtifactMatch::None) => {
            // Tell the user no report matched the provided name.
            println!("No Azure inventory report found for `{name}`.");
        }
        Err(error) => {
            // Show the concrete filesystem problem when reading cannot continue.
            println!("Unable to show Azure inventory report: {error}");
        }
    }
}

// Delete one saved Azure inventory report.
pub(super) fn handle_report_delete(name: &str) {
    // Ask the service layer to resolve and delete the requested report.
    match delete_inventory_report(name) {
        Ok(ArtifactMatch::One(path)) => {
            // Confirm the exact file that was deleted.
            println!("Deleted Azure inventory report {}", path.display());
        }
        Ok(ArtifactMatch::Many(paths)) => {
            // Explain that deleting an arbitrary match would be unsafe.
            println!("Multiple Azure inventory reports match `{name}`:");
            // Show every matching option so the user can retry with a clearer name.
            for path in paths {
                println!("{}", path.display());
            }
        }
        Ok(ArtifactMatch::None) => {
            // Tell the user no report matched the provided name.
            println!("No Azure inventory report found for `{name}`.");
        }
        Err(error) => {
            // Show the concrete filesystem problem when deleting cannot continue.
            println!("Unable to delete Azure inventory report: {error}");
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
            println!("Azure inventory report saved to {}", path.display());
        }
        Err(error) => {
            // Explain clearly why saving failed while keeping stdout behavior intact.
            println!("Unable to save the Azure inventory report: {error}");
        }
    }
}
