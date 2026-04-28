// Declare the CLI shape module so Azure-specific Clap types stay together.
mod cli;
// Declare the inventory module so report commands have a focused home.
mod inventory;
// Declare the login module so authentication flow and validation stay together.
mod login;
// Declare the snapshot module so JSON snapshot commands have a focused home.
mod snapshot;
// Declare the state module so cached Azure account handling stays together.
mod state;

// Re-export the top-level Azure command so `src/cli.rs` can compose the full CLI.
pub(crate) use cli::AzureCommand;
// Import the nested command model used by the Azure command runner.
use cli::{
    InventoryCommand, InventoryGroupsCommand, InventoryResourcesCommand, ReportCommand,
    SnapshotCommand, SnapshotCreateCommand,
};
// Import inventory command handlers that do the user-facing report work.
use inventory::{
    handle_inventory_groups_list, handle_inventory_resources_list, handle_inventory_resources_tree,
    handle_report_delete, handle_report_list, handle_report_show,
};
// Import the login handler that resolves arguments and calls Azure CLI.
use login::{handle_login, handle_logout};
// Import snapshot command handlers that do the user-facing JSON snapshot work.
use snapshot::{
    handle_snapshot_create_all, handle_snapshot_create_groups, handle_snapshot_create_resources,
    handle_snapshot_delete, handle_snapshot_list,
};
// Import state helpers used by startup, status and intro rendering.
use state::{SessionState, refresh_and_print_status};

// Import the shared application result type.
use crate::AppResult;

// Run one non-interactive Azure command.
pub(crate) async fn run_command(command: AzureCommand) -> AppResult<()> {
    // Create fresh state so commands that need Azure account details can refresh it.
    let mut state = SessionState::default();

    // Match on the parsed command so every CLI branch stays explicit and easy to read.
    match command {
        AzureCommand::Status => {
            // Refresh the cached account information before showing it.
            refresh_and_print_status(&mut state).await;
        }
        AzureCommand::Inventory(InventoryCommand::Resources(InventoryResourcesCommand::List(
            arguments,
        ))) => {
            // Print resources and optionally save the output as a Markdown report.
            handle_inventory_resources_list(&mut state, &arguments).await;
        }
        AzureCommand::Inventory(InventoryCommand::Resources(InventoryResourcesCommand::Tree(
            arguments,
        ))) => {
            // Print resources as a tree and optionally save a Markdown report.
            handle_inventory_resources_tree(&mut state, &arguments).await;
        }
        AzureCommand::Inventory(InventoryCommand::Groups(InventoryGroupsCommand::List(
            arguments,
        ))) => {
            // Print resource groups and optionally save the output as a Markdown report.
            handle_inventory_groups_list(&mut state, &arguments).await;
        }
        AzureCommand::Snapshot(SnapshotCommand::Create(SnapshotCreateCommand::Resources)) => {
            // Build the JSON resource snapshot and save it to disk.
            handle_snapshot_create_resources(&mut state).await;
        }
        AzureCommand::Snapshot(SnapshotCommand::Create(SnapshotCreateCommand::Groups)) => {
            // Build the JSON resource-group snapshot and save it to disk.
            handle_snapshot_create_groups(&mut state).await;
        }
        AzureCommand::Snapshot(SnapshotCommand::Create(SnapshotCreateCommand::All)) => {
            // Build both JSON snapshot types and save them to disk.
            handle_snapshot_create_all(&mut state).await;
        }
        AzureCommand::Snapshot(SnapshotCommand::List) => {
            // List saved JSON snapshots from disk.
            handle_snapshot_list();
        }
        AzureCommand::Snapshot(SnapshotCommand::Delete { name }) => {
            // Delete one saved JSON snapshot by user-provided name.
            handle_snapshot_delete(&name);
        }
        AzureCommand::Report(ReportCommand::List) => {
            // List saved Markdown inventory reports from disk.
            handle_report_list();
        }
        AzureCommand::Report(ReportCommand::Show { name }) => {
            // Print one saved Markdown inventory report.
            handle_report_show(&name);
        }
        AzureCommand::Report(ReportCommand::Delete { name }) => {
            // Delete one saved Markdown inventory report by user-provided name.
            handle_report_delete(&name);
        }
        AzureCommand::Login(arguments) => {
            // Run the login flow after resolving CLI arguments and config defaults together.
            handle_login(&mut state, &arguments).await;
        }
        AzureCommand::Logout => {
            // Run the logout flow and then show the updated status.
            handle_logout(&mut state).await;
        }
    }

    // Report success after the selected command handler has finished.
    Ok(())
}
