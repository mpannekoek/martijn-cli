// Import `Deserialize` so Azure CLI JSON can become typed Rust values.
// Import `Serialize` so snapshot data can become JSON output.
use serde::{Deserialize, Serialize};
// Import `Value` so snapshots can keep flexible Azure JSON fields without losing data.
use serde_json::Value;
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

// Store the complete JSON document that is written for one Azure snapshot.
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct AzureSnapshotEnvelope {
    // Rename this field to the camelCase JSON name requested by the snapshot format.
    #[serde(rename = "generatedAt")]
    pub(crate) generated_at: String,
    // Store subscription metadata so the snapshot remains understandable later.
    pub(crate) subscription: AzureSnapshotSubscription,
    // Store every normalized resource together with its hash and original JSON.
    pub(crate) resources: Vec<AzureSnapshotResource>,
}

// Store subscription metadata inside the snapshot envelope.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct AzureSnapshotSubscription {
    // Store the active subscription ID under the short `id` JSON field.
    pub(crate) id: String,
    // Store the friendly subscription name returned by Azure CLI.
    pub(crate) name: String,
    // Store the Azure user or service principal used while creating the snapshot.
    pub(crate) user: String,
}

// Store one resource entry inside the snapshot document.
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct AzureSnapshotResource {
    // Store the stable subset used for comparisons and hashing.
    pub(crate) normalized: AzureSnapshotNormalizedResource,
    // Store the SHA-256 hash of the normalized JSON.
    pub(crate) fingerprint: String,
    // Store the original Azure CLI JSON object exactly as `serde_json` parsed it.
    pub(crate) raw: Value,
}

// Store the stable resource shape used by snapshot consumers.
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct AzureSnapshotNormalizedResource {
    // Store the Azure resource ID, or an empty string when Azure omitted it.
    pub(crate) id: String,
    // Store the resource name, or an empty string when Azure omitted it.
    pub(crate) name: String,
    // Rename this field because `type` is a Rust keyword but required in JSON.
    #[serde(rename = "type")]
    pub(crate) resource_type: String,
    // Rename this field to match Azure's camelCase `resourceGroup` spelling.
    #[serde(rename = "resourceGroup")]
    pub(crate) resource_group: String,
    // Store the Azure location, or an empty string when Azure omitted it.
    pub(crate) location: String,
    // Store `kind` as flexible JSON because Azure may return strings, null, or omit it.
    pub(crate) kind: Value,
    // Store `sku` as flexible JSON because Azure SKU shapes differ by resource type.
    pub(crate) sku: Value,
    // Store tags as JSON so keys and values stay exactly available to snapshot readers.
    pub(crate) tags: Value,
}
