// Expose the Azure shell module so callers can start the Azure shell.
pub(crate) mod azure;
// Expose the dummy shell module so callers can start the example shell.
pub(crate) mod dummy;
// Expose the shared shell engine used by multiple interactive shells.
pub(crate) mod engine;
// Expose the root shell module that acts as the main interactive entry point.
pub(crate) mod root;
