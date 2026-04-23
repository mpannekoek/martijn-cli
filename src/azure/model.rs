// Import `Deserialize` so Azure CLI JSON can become typed Rust values.
use serde::Deserialize;
// Import `BTreeMap` so resource tags can be stored as key/value pairs.
use std::collections::BTreeMap;

// Hold the account details that the shell shows in `status`.
#[derive(Debug, Clone)]
pub(crate) struct AzureAccount {
    // Store the friendly subscription name returned by Azure CLI.
    pub(crate) name: String,
    // Store the active subscription identifier.
    pub(crate) subscription_id: String,
    // Store the current Azure user or service principal name.
    pub(crate) user: String,
}

// Store only the resource group fields that the inventory report needs.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct AzureResourceGroupReportItem {
    // Store the resource group name exactly as Azure returns it.
    pub(crate) name: String,
    // Store the Azure region for this resource group.
    pub(crate) location: String,
}

// Store only the resource fields that the inventory report needs.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct AzureResourceReportItem {
    // Store the resource name that users recognize in the Azure portal.
    pub(crate) name: String,
    // Rename the JSON field `type` because `type` is a Rust keyword.
    #[serde(rename = "type")]
    pub(crate) resource_type: String,
    // Use an empty string when Azure does not send a location for this resource.
    #[serde(default)]
    pub(crate) location: String,
    // Store the optional Azure resource kind when that field is present.
    #[serde(default)]
    pub(crate) kind: Option<String>,
    // Store optional Azure tags as a key/value map for readable report output.
    #[serde(default)]
    pub(crate) tags: Option<BTreeMap<String, String>>,
    // Store optional SKU details so the report can show SKU values in table form.
    #[serde(default)]
    pub(crate) sku: Option<AzureResourceSkuReportItem>,
}

// Store only the nested SKU fields that the inventory report needs.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct AzureResourceSkuReportItem {
    // Store the optional SKU name returned by Azure CLI.
    #[serde(default)]
    pub(crate) name: Option<String>,
}

// Combine one resource group with the resources that belong to it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AzureInventoryGroup {
    // Store the resource group metadata that becomes the section heading.
    pub(crate) resource_group: AzureResourceGroupReportItem,
    // Store the group's resources in display order.
    pub(crate) resources: Vec<AzureResourceReportItem>,
}
