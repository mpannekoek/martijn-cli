// Import the Azure service layer so snapshot commands can delegate real Azure work.
use crate::azure::service::{
    ArtifactMatch, delete_snapshot, generate_group_snapshot, generate_resource_snapshot,
    list_snapshots, snapshot_kind_label,
};

// Import the shell session state so snapshot commands can check the active account.
use super::state::{SessionState, refresh_session_state};

// Build the Azure resource snapshot, save it as JSON and tell the user where it lives.
pub(super) async fn handle_snapshot_create_resources(state: &mut SessionState) {
    // Refresh the login state first so the command works with the latest Azure session.
    refresh_session_state(state).await;

    // Stop early when no Azure account is active.
    let Some(account) = state.account.as_ref() else {
        println!("Not logged in to Azure CLI. Run `login <tenant>` first.");
        return;
    };

    // Ask the service layer to build and write the resource snapshot.
    match generate_resource_snapshot(account).await {
        Ok(output_file_path) => {
            // Confirm success and show the final path to the newly created snapshot.
            println!(
                "Azure resource snapshot saved to {}",
                output_file_path.display()
            );
        }
        Err(error) => {
            // Explain clearly why the snapshot could not be generated.
            println!("Unable to generate the Azure resource snapshot: {error}");
        }
    }
}

// Build the Azure resource-group snapshot, save it as JSON and tell the user where it lives.
pub(super) async fn handle_snapshot_create_groups(state: &mut SessionState) {
    // Refresh the login state first so the command works with the latest Azure session.
    refresh_session_state(state).await;

    // Stop early when no Azure account is active.
    let Some(account) = state.account.as_ref() else {
        println!("Not logged in to Azure CLI. Run `login <tenant>` first.");
        return;
    };

    // Ask the service layer to build and write the group snapshot.
    match generate_group_snapshot(account).await {
        Ok(output_file_path) => {
            // Confirm success and show the final path to the newly created snapshot.
            println!(
                "Azure group snapshot saved to {}",
                output_file_path.display()
            );
        }
        Err(error) => {
            // Explain clearly why the snapshot could not be generated.
            println!("Unable to generate the Azure group snapshot: {error}");
        }
    }
}

// Build both Azure snapshot types.
pub(super) async fn handle_snapshot_create_all(state: &mut SessionState) {
    // Refresh the login state first so both commands share the latest Azure session.
    refresh_session_state(state).await;

    // Stop early when no Azure account is active.
    let Some(account) = state.account.as_ref() else {
        println!("Not logged in to Azure CLI. Run `login <tenant>` first.");
        return;
    };

    // Ask the service layer to build and write the resource snapshot first.
    match generate_resource_snapshot(account).await {
        Ok(output_file_path) => {
            // Confirm success and show the final path to the newly created snapshot.
            println!(
                "Azure resource snapshot saved to {}",
                output_file_path.display()
            );
        }
        Err(error) => {
            // Explain clearly why the first snapshot could not be generated.
            println!("Unable to generate the Azure resource snapshot: {error}");
            return;
        }
    }

    // Ask the service layer to build and write the group snapshot after resources succeed.
    match generate_group_snapshot(account).await {
        Ok(output_file_path) => {
            // Confirm success and show the final path to the newly created snapshot.
            println!(
                "Azure group snapshot saved to {}",
                output_file_path.display()
            );
        }
        Err(error) => {
            // Explain clearly why the second snapshot could not be generated.
            println!("Unable to generate the Azure group snapshot: {error}");
        }
    }
}

// List saved Azure snapshots from the local snapshot directories.
pub(super) fn handle_snapshot_list() {
    // Ask the service layer for the filtered, newest-first snapshot list.
    let snapshots = match list_snapshots() {
        Ok(snapshots) => snapshots,
        Err(error) => {
            // Show the concrete filesystem problem when listing cannot continue.
            println!("Unable to list Azure snapshots: {error}");
            return;
        }
    };

    // Stop early with a friendly message when there is nothing to show.
    if snapshots.is_empty() {
        println!("No Azure snapshots found.");
        return;
    }

    // Print every snapshot path in the newest-first order returned by the service layer.
    for snapshot in snapshots {
        // Display kind, size and path so users can identify the snapshot quickly.
        println!(
            "{}  {}  {} bytes  {}",
            snapshot_kind_label(snapshot.kind),
            snapshot.file_name,
            snapshot.size_bytes,
            snapshot.path.display()
        );
    }
}

// Delete one saved Azure snapshot.
pub(super) fn handle_snapshot_delete(name: &str) {
    // Ask the service layer to resolve and delete the requested snapshot.
    match delete_snapshot(name) {
        Ok(ArtifactMatch::One(path)) => {
            // Confirm the exact file that was deleted.
            println!("Deleted Azure snapshot {}", path.display());
        }
        Ok(ArtifactMatch::Many(paths)) => {
            // Explain that deleting an arbitrary match would be unsafe.
            println!("Multiple Azure snapshots match `{name}`:");
            // Show every matching option so the user can retry with a clearer name.
            for path in paths {
                println!("{}", path.display());
            }
        }
        Ok(ArtifactMatch::None) => {
            // Tell the user no snapshot matched the provided name.
            println!("No Azure snapshot found for `{name}`.");
        }
        Err(error) => {
            // Show the concrete filesystem problem when deleting cannot continue.
            println!("Unable to delete Azure snapshot: {error}");
        }
    }
}
