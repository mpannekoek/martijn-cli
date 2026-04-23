// Expose the Azure data model types so the shell and services can share them.
pub(crate) mod model;
// Expose the Markdown report helpers for Azure inventory exports.
pub(crate) mod report;
// Expose the Azure service layer that talks to Azure CLI and the filesystem.
pub(crate) mod service;
