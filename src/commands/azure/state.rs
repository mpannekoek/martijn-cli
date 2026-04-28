// Import the Azure account model that the CLI caches and displays.
use crate::azure::model::AzureAccount;
// Import the Azure service helper that reads the current account from Azure CLI.
use crate::azure::service::fetch_azure_account;

// Keep the Azure command state in one place.
#[derive(Debug, Default)]
pub(super) struct SessionState {
    // Store the currently detected Azure account.
    // We use `Option` because the user may not be logged in.
    pub(super) account: Option<AzureAccount>,
}

// Print either the current Azure account or a message that no account is active.
pub(super) fn print_status(state: &SessionState) {
    // Match on the optional account because the user may or may not be logged in.
    match &state.account {
        Some(account) => {
            // Show the user, subscription name and subscription identifier.
            println!(
                "Logged in as {} ({}) on subscription {}",
                account.user, account.name, account.subscription_id
            );
        }
        None => {
            // Explain clearly that no Azure login session was detected.
            println!("Not logged in to Azure CLI.");
        }
    }
}

// Refresh the cached Azure account and immediately print the visible status.
pub(super) async fn refresh_and_print_status(state: &mut SessionState) {
    // Update the in-memory account data first.
    refresh_session_state(state).await;
    // Print the new status after the refresh.
    print_status(state);
}

// Refresh the cached Azure account state by asking Azure CLI for the current account.
pub(super) async fn refresh_session_state(state: &mut SessionState) {
    // Fetch the account data and handle both success and failure explicitly.
    match fetch_azure_account().await {
        Ok(account) => {
            // Replace the cached account with the freshly fetched value.
            state.account = account;
        }
        Err(error) => {
            // Clear the cached account when the status check itself failed.
            state.account = None;
            // Show the concrete error so the user understands why status is unavailable.
            println!("Azure status check failed: {error}");
        }
    }
}
