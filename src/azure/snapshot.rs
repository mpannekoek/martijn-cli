// Import the snapshot model types that describe the JSON output.
use crate::azure::model::{
    AzureAccount, AzureSnapshotEnvelope, AzureSnapshotNormalizedResource, AzureSnapshotResource,
    AzureSnapshotSubscription,
};
// Import the shared CLI-directory resolver so snapshot paths match config and inventory paths.
use crate::config::resolve_cli_directory;
// Import the shared application result type for fallible helpers.
use crate::AppResult;
// Import SHA-256 helpers so each normalized resource gets a stable fingerprint.
use sha2::{Digest, Sha256};
// Import `Map` and `Value` so we can work with raw Azure JSON safely.
use serde_json::{Map, Value};
// Import `Ordering` so the resource sort function can combine comparisons clearly.
use std::cmp::Ordering;
// Import `PathBuf` so returned paths stay platform-aware.
use std::path::PathBuf;
// Import `Path` only for tests because normal snapshot generation starts from the CLI directory.
#[cfg(test)]
use std::path::Path;
// Import time helpers so snapshot metadata and filenames get readable timestamps.
use time::OffsetDateTime;
// Import the well-known RFC 3339 formatter for machine-readable JSON timestamps.
use time::format_description::well_known::Rfc3339;
// Import the custom filename timestamp formatter macro.
use time::macros::format_description;
// Import `Uuid` so repeated snapshot runs in one second still get unique names.
use uuid::Uuid;

// Build the full snapshot JSON envelope from an Azure account and raw resource list.
pub(crate) fn build_snapshot_envelope(
    account: &AzureAccount,
    raw_resources: Vec<Value>,
) -> AppResult<AzureSnapshotEnvelope> {
    // Capture the current UTC time once so metadata is consistent for the whole file.
    let generated_at = OffsetDateTime::now_utc();
    // Format the timestamp with a standard JSON-friendly representation.
    let generated_at_display = generated_at
        .format(&Rfc3339)
        .map_err(|error| format!("unable to format the snapshot generation timestamp: {error}"))?;

    // Convert raw Azure resources into normalized snapshot entries.
    let mut resources = build_snapshot_resources(raw_resources)?;
    // Sort entries after normalization so output stays deterministic between runs.
    sort_snapshot_resources(&mut resources);

    // Build the small subscription block from the active account information.
    let subscription = AzureSnapshotSubscription {
        id: account.subscription_id.clone(),
        name: account.name.clone(),
        user: account.user.clone(),
    };

    // Return the complete serializable snapshot document.
    Ok(AzureSnapshotEnvelope {
        generated_at: generated_at_display,
        subscription,
        resources,
    })
}

// Convert all raw Azure resources into snapshot entries.
fn build_snapshot_resources(raw_resources: Vec<Value>) -> AppResult<Vec<AzureSnapshotResource>> {
    // Allocate the output vector with enough room for every incoming resource.
    let mut snapshot_resources: Vec<AzureSnapshotResource> =
        Vec::with_capacity(raw_resources.len());

    // Walk through every raw value so each resource can keep its original JSON.
    for raw_resource in raw_resources {
        // Build the stable normalized view from the current raw Azure object.
        let normalized = normalize_snapshot_resource(&raw_resource);
        // Hash the normalized view before moving it into the snapshot entry.
        let fingerprint = fingerprint_normalized_resource(&normalized)?;

        // Store normalized data, its fingerprint, and the original raw JSON together.
        snapshot_resources.push(AzureSnapshotResource {
            normalized,
            fingerprint,
            raw: raw_resource,
        });
    }

    // Return all prepared resources.
    Ok(snapshot_resources)
}

// Build the stable normalized resource shape from raw Azure JSON.
pub(crate) fn normalize_snapshot_resource(raw_resource: &Value) -> AzureSnapshotNormalizedResource {
    // Store an object reference when Azure returned an object, or `None` for other JSON types.
    let raw_object = raw_resource.as_object();

    // Read required string fields explicitly so missing values become empty strings.
    let id = read_string_field(raw_object, "id");
    // Read the resource name from Azure's `name` field.
    let name = read_string_field(raw_object, "name");
    // Read Azure's `type` field into a Rust-friendly field name.
    let resource_type = read_string_field(raw_object, "type");
    // Read Azure's camelCase resource group field.
    let resource_group = read_string_field(raw_object, "resourceGroup");
    // Read the region or keep an empty string when Azure omitted it.
    let location = read_string_field(raw_object, "location");
    // Clone optional JSON fields so the snapshot keeps their original shape.
    let kind = read_json_field_or_null(raw_object, "kind");
    // Clone SKU as JSON because Azure services use different SKU object shapes.
    let sku = read_json_field_or_null(raw_object, "sku");
    // Clone tags only when they are an object, otherwise use an empty object.
    let tags = read_tags_field_or_empty_object(raw_object);

    // Return the exact normalized field set requested for snapshots.
    AzureSnapshotNormalizedResource {
        id,
        name,
        resource_type,
        resource_group,
        location,
        kind,
        sku,
        tags,
    }
}

// Read one string field from an optional JSON object.
fn read_string_field(raw_object: Option<&Map<String, Value>>, field_name: &str) -> String {
    // Try to look up the field only when the raw value was a JSON object.
    let Some(field_value) = raw_object.and_then(|object| object.get(field_name)) else {
        return String::new();
    };

    // Keep string values exactly as Azure provided them.
    match field_value.as_str() {
        Some(text_value) => text_value.to_owned(),
        None => String::new(),
    }
}

// Read one JSON field or return JSON null when it is missing.
fn read_json_field_or_null(raw_object: Option<&Map<String, Value>>, field_name: &str) -> Value {
    // Try to clone the field from the raw Azure object.
    match raw_object.and_then(|object| object.get(field_name)) {
        Some(field_value) => sort_json_value(field_value),
        None => Value::Null,
    }
}

// Read the tags field and require it to be a JSON object.
fn read_tags_field_or_empty_object(raw_object: Option<&Map<String, Value>>) -> Value {
    // Try to clone tags only when Azure returned an object.
    match raw_object.and_then(|object| object.get("tags")) {
        Some(field_value @ Value::Object(_)) => sort_json_value(field_value),
        _ => Value::Object(Map::new()),
    }
}

// Clone a JSON value while sorting every object by key.
fn sort_json_value(value: &Value) -> Value {
    // Keep arrays stable by sorting any objects nested inside their items.
    match value {
        Value::Array(items) => {
            // Prepare a new array with the same number of items.
            let mut sorted_items: Vec<Value> = Vec::with_capacity(items.len());

            // Recursively normalize every item in the array.
            for item in items {
                sorted_items.push(sort_json_value(item));
            }

            // Return the rebuilt array.
            Value::Array(sorted_items)
        }
        Value::Object(object) => {
            // Collect keys first so we can insert them in a deterministic order.
            let mut keys: Vec<&String> = object.keys().collect();
            // Sort keys lexicographically so JSON serialization stays stable.
            keys.sort();

            // Prepare a fresh JSON object for the sorted key/value pairs.
            let mut sorted_object = Map::new();

            // Insert values in sorted key order.
            for key in keys {
                // Look up the value again by key and recursively sort nested objects.
                if let Some(nested_value) = object.get(key) {
                    sorted_object.insert(key.clone(), sort_json_value(nested_value));
                }
            }

            // Return the rebuilt object.
            Value::Object(sorted_object)
        }
        _ => {
            // Primitive JSON values have no key order to normalize.
            value.clone()
        }
    }
}

// Calculate the SHA-256 fingerprint for one normalized resource.
pub(crate) fn fingerprint_normalized_resource(
    normalized: &AzureSnapshotNormalizedResource,
) -> AppResult<String> {
    // Serialize the normalized struct into stable compact JSON bytes.
    let normalized_json = serde_json::to_vec(normalized)
        .map_err(|error| format!("unable to serialize normalized snapshot resource: {error}"))?;
    // Create a fresh SHA-256 hasher for this one resource.
    let mut hasher = Sha256::new();
    // Feed the normalized JSON bytes into the hasher by borrowing the byte vector.
    hasher.update(&normalized_json);
    // Finish the hash calculation and receive the raw hash bytes.
    let hash_bytes = hasher.finalize();
    // Prepare a lowercase hexadecimal string with two characters for every byte.
    let mut fingerprint = String::with_capacity(hash_bytes.len() * 2);

    // Convert every hash byte into lowercase hexadecimal text.
    for byte in hash_bytes {
        fingerprint.push_str(&format!("{byte:02x}"));
    }

    // Return the final 64-character SHA-256 fingerprint.
    Ok(fingerprint)
}

// Sort snapshot resources by stable human-readable keys.
fn sort_snapshot_resources(resources: &mut [AzureSnapshotResource]) {
    // Compare normalized fields in the requested deterministic order.
    resources.sort_by(|left, right| compare_snapshot_resources(left, right));
}

// Compare two snapshot resources using type, resource group, name, then ID.
fn compare_snapshot_resources(
    left: &AzureSnapshotResource,
    right: &AzureSnapshotResource,
) -> Ordering {
    // Lowercase each sort key so differences in casing do not reshuffle output.
    left.normalized
        .resource_type
        .to_lowercase()
        .cmp(&right.normalized.resource_type.to_lowercase())
        .then_with(|| {
            left.normalized
                .resource_group
                .to_lowercase()
                .cmp(&right.normalized.resource_group.to_lowercase())
        })
        .then_with(|| {
            left.normalized
                .name
                .to_lowercase()
                .cmp(&right.normalized.name.to_lowercase())
        })
        .then_with(|| {
            left.normalized
                .id
                .to_lowercase()
                .cmp(&right.normalized.id.to_lowercase())
        })
}

// Resolve the final snapshot output directory inside the shared CLI folder.
pub(crate) fn resolve_snapshot_output_directory() -> AppResult<PathBuf> {
    // Resolve `~/.martijn/cli` once so snapshot files live beside the other CLI data.
    let cli_directory = resolve_cli_directory()?;
    // Append the fixed snapshot folder below the shared CLI directory.
    Ok(cli_directory.join("snapshot"))
}

// Append the fixed `.martijn/cli/snapshot` directory under a known home directory.
#[cfg(test)]
pub(crate) fn build_snapshot_output_directory_from_home(home_directory: &Path) -> PathBuf {
    // Join path segments one by one so separators stay correct on every platform.
    home_directory.join(".martijn").join("cli").join("snapshot")
}

// Build a unique snapshot filename that includes a timestamp and a short UUID fragment.
pub(crate) fn build_snapshot_file_name() -> String {
    // Capture the current UTC time for the timestamp part of the filename.
    let now = OffsetDateTime::now_utc();
    // Describe the compact timestamp format used in the filename.
    let file_name_format = format_description!("[year][month][day]-[hour][minute][second]");
    // Format the current time using the compact filename-safe representation.
    let timestamp = now
        .format(&file_name_format)
        .expect("filename timestamp formatting should succeed");
    // Generate a random UUID so repeated runs in the same second still stay unique.
    let unique_id = Uuid::new_v4().simple().to_string();
    // Keep only the first eight characters so the filename stays compact.
    let short_unique_id = &unique_id[..8];

    // Return the final filename with the required prefix and `.json` extension.
    format!("azure-snapshot-{timestamp}-{short_unique_id}.json")
}

#[cfg(test)]
mod tests {
    // Import the snapshot helpers that these tests verify.
    use super::{
        build_snapshot_file_name, build_snapshot_output_directory_from_home,
        fingerprint_normalized_resource, normalize_snapshot_resource,
    };
    // Import `json` so tests can build representative Azure JSON values clearly.
    use serde_json::json;
    // Import `PathBuf` so path-building helpers can be tested with platform-aware values.
    use std::path::PathBuf;

    #[test]
    fn normalizes_representative_raw_azure_resource() {
        // Build one Azure resource with all fields that the snapshot cares about.
        let raw_resource = json!({
            "id": "/subscriptions/sub-123/resourceGroups/rg-app/providers/Microsoft.Web/sites/app-api",
            "name": "app-api",
            "type": "Microsoft.Web/sites",
            "resourceGroup": "rg-app",
            "location": "westeurope",
            "kind": "app",
            "sku": {
                "name": "P1v3"
            },
            "tags": {
                "env": "prod"
            }
        });

        // Normalize the raw JSON into the stable snapshot shape.
        let normalized = normalize_snapshot_resource(&raw_resource);

        // Confirm that string fields are copied from Azure JSON.
        assert_eq!(
            normalized.id,
            "/subscriptions/sub-123/resourceGroups/rg-app/providers/Microsoft.Web/sites/app-api"
        );
        // Confirm that the name field is copied.
        assert_eq!(normalized.name, "app-api");
        // Confirm that the Azure `type` field is copied into `resource_type`.
        assert_eq!(normalized.resource_type, "Microsoft.Web/sites");
        // Confirm that the resource group field keeps Azure's value.
        assert_eq!(normalized.resource_group, "rg-app");
        // Confirm that the location field keeps Azure's value.
        assert_eq!(normalized.location, "westeurope");
        // Confirm that optional JSON fields keep their original JSON shape.
        assert_eq!(normalized.kind, json!("app"));
        // Confirm that SKU remains JSON instead of being flattened.
        assert_eq!(normalized.sku, json!({ "name": "P1v3" }));
        // Confirm that tags remain a JSON object.
        assert_eq!(normalized.tags, json!({ "env": "prod" }));
    }

    #[test]
    fn normalizes_missing_optional_fields_to_defaults() {
        // Build one minimal Azure resource without optional fields.
        let raw_resource = json!({
            "id": "resource-id",
            "name": "storage",
            "type": "Microsoft.Storage/storageAccounts",
            "resourceGroup": "rg-data",
            "location": "northeurope"
        });

        // Normalize the raw JSON into the stable snapshot shape.
        let normalized = normalize_snapshot_resource(&raw_resource);

        // Confirm that missing kind becomes JSON null.
        assert_eq!(normalized.kind, json!(null));
        // Confirm that missing SKU becomes JSON null.
        assert_eq!(normalized.sku, json!(null));
        // Confirm that missing tags become an empty JSON object.
        assert_eq!(normalized.tags, json!({}));
    }

    #[test]
    fn fingerprint_is_stable_for_same_normalized_resource() {
        // Build one raw Azure resource for repeatable normalization.
        let raw_resource = json!({
            "id": "resource-id",
            "name": "storage",
            "type": "Microsoft.Storage/storageAccounts",
            "resourceGroup": "rg-data",
            "location": "northeurope"
        });
        // Normalize the raw JSON once.
        let normalized = normalize_snapshot_resource(&raw_resource);

        // Calculate the fingerprint twice from the same normalized value.
        let first_fingerprint =
            fingerprint_normalized_resource(&normalized).expect("first fingerprint should hash");
        let second_fingerprint =
            fingerprint_normalized_resource(&normalized).expect("second fingerprint should hash");

        // Confirm that hashing the same normalized JSON is deterministic.
        assert_eq!(first_fingerprint, second_fingerprint);
        // Confirm that SHA-256 is represented as 64 lowercase hexadecimal characters.
        assert_eq!(first_fingerprint.len(), 64);
    }

    #[test]
    fn snapshot_output_directory_builds_under_martijn_cli_snapshot() {
        // Build a fake Unix-style home path for the path helper.
        let home_directory = PathBuf::from("/home/martijn");
        // Build the snapshot directory from that home path.
        let snapshot_directory = build_snapshot_output_directory_from_home(&home_directory);

        // Confirm that the helper appends `.martijn/cli/snapshot` in order.
        assert_eq!(
            snapshot_directory,
            home_directory.join(".martijn").join("cli").join("snapshot")
        );
    }

    #[test]
    fn snapshot_file_name_uses_expected_prefix_and_extension() {
        // Build one generated filename.
        let file_name = build_snapshot_file_name();

        // Confirm that the filename uses the expected prefix.
        assert!(file_name.starts_with("azure-snapshot-"));
        // Confirm that the filename ends with the JSON extension.
        assert!(file_name.ends_with(".json"));
    }
}
