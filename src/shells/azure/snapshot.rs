// Import the Azure service layer so snapshot commands can delegate real Azure work.
use crate::azure::service::generate_resource_snapshot;

// Import the shell session state so snapshot commands can check the active account.
use super::state::{SessionState, refresh_session_state};

// Build the Azure resource snapshot, save it as JSON and tell the user where it lives.
pub(super) async fn handle_snapshot_generate(state: &mut SessionState) {
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
            println!("Azure snapshot saved to {}", output_file_path.display());
        }
        Err(error) => {
            // Explain clearly why the snapshot could not be generated.
            println!("Unable to generate the Azure snapshot: {error}");
        }
    }
}
