// Import the Azure service layer so inventory commands can delegate real Azure work.
use crate::azure::service::{
    generate_inventory_report, inventory_reports_directory, list_inventory_reports,
};

// Import the shell session state so inventory commands can check the active account.
use super::state::{SessionState, refresh_session_state};

// Build the Azure inventory report, save it as Markdown and tell the user where it lives.
pub(super) async fn handle_inventory_generate(state: &mut SessionState) {
    // Refresh the login state first so the command works with the latest Azure session.
    refresh_session_state(state).await;

    // Stop early when no Azure account is active.
    let Some(account) = state.account.as_ref() else {
        println!("Not logged in to Azure CLI. Run `login <tenant>` first.");
        return;
    };

    // Ask the service layer to build and write the inventory report.
    match generate_inventory_report(account).await {
        Ok(output_file_path) => {
            // Confirm success and show the final path to the newly created report.
            println!("Azure inventory saved to {}", output_file_path.display());
        }
        Err(error) => {
            // Explain clearly why the report could not be generated.
            println!("Unable to generate the Azure inventory report: {error}");
        }
    }
}

// List saved Azure inventory reports from the local inventory directory.
pub(super) fn handle_inventory_list() {
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
                    "No Azure inventory reports found in {}.",
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
        // Display the full path so users can open or copy the report directly.
        println!("{}", report.path.display());
    }
}
