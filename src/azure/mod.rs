// Expose the Azure data model types so commands and services can share them.
pub(crate) mod model;
// Expose the Markdown report helpers for Azure inventory exports.
pub(crate) mod report;
// Expose the JSON snapshot helpers for Azure resource snapshots.
pub(crate) mod snapshot;
// Expose the Azure service layer that talks to Azure CLI and the filesystem.
pub(crate) mod service;
