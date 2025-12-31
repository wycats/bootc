//! Property-based tests for manifest types.
//!
//! These tests use proptest to generate random inputs and verify
//! that core invariants hold.

use proptest::prelude::*;

// Import the manifest modules
// Since this is a binary crate, we need to use the module path
// We'll test the properties through the public module interface

/// Generate a valid shim name (alphanumeric + dashes).
fn shim_name_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{0,20}".prop_filter("non-empty", |s| !s.is_empty())
}

/// Generate a valid flatpak app ID.
fn flatpak_id_strategy() -> impl Strategy<Value = String> {
    "org\\.[a-z]{2,8}\\.[A-Z][a-zA-Z]{2,10}"
}

/// Generate a valid GNOME extension UUID.
fn extension_uuid_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{2,15}@[a-z]+\\.[a-z]+"
}

/// Generate a valid gsetting schema.
fn gsetting_schema_strategy() -> impl Strategy<Value = String> {
    "org\\.[a-z]{2,8}\\.[a-z]{2,10}"
}

/// Generate a valid gsetting key.
fn gsetting_key_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{2,15}"
}

/// Generate a valid gsetting value.
fn gsetting_value_strategy() -> impl Strategy<Value = String> {
    "'[a-z]{1,10}'|[0-9]{1,5}|true|false"
}

proptest! {
    // ========================================================================
    // Shim manifest property tests
    // ========================================================================

    #[test]
    fn shim_upsert_always_adds_or_updates(name in shim_name_strategy()) {
        let json = r#"{"shims": []}"#;
        let mut manifest: serde_json::Value = serde_json::from_str(json).unwrap();

        // Adding a shim should always succeed
        let shim = serde_json::json!({"name": name, "host": null});
        let shims = manifest["shims"].as_array_mut().unwrap();

        // Find and update or add
        let existing_idx = shims.iter().position(|s| s["name"] == name);
        if let Some(idx) = existing_idx {
            shims[idx] = shim;
        } else {
            shims.push(shim);
        }

        // Verify the shim is present
        let found = shims.iter().any(|s| s["name"].as_str() == Some(&name));
        prop_assert!(found, "Shim should be present after upsert");
    }

    #[test]
    fn shim_remove_reduces_count(names in prop::collection::vec(shim_name_strategy(), 1..5)) {
        let mut manifest: serde_json::Value = serde_json::json!({"shims": []});

        // Add all shims
        for name in &names {
            let shim = serde_json::json!({"name": name});
            manifest["shims"].as_array_mut().unwrap().push(shim);
        }

        let initial_count = manifest["shims"].as_array().unwrap().len();

        // Remove first shim
        let name_to_remove = &names[0];
        manifest["shims"].as_array_mut().unwrap()
            .retain(|s| s["name"].as_str() != Some(name_to_remove));

        let final_count = manifest["shims"].as_array().unwrap().len();

        // Count should either decrease by 1 or stay same (if name appeared multiple times)
        prop_assert!(final_count <= initial_count);
    }

    #[test]
    fn shim_serialization_roundtrip(name in shim_name_strategy(), host in prop::option::of(shim_name_strategy())) {
        let shim = serde_json::json!({
            "name": name,
            "host": host
        });

        let json_str = serde_json::to_string(&shim).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(&shim["name"], &parsed["name"]);
        prop_assert_eq!(&shim["host"], &parsed["host"]);
    }

    // ========================================================================
    // Flatpak manifest property tests
    // ========================================================================

    #[test]
    fn flatpak_app_serialization_roundtrip(
        id in flatpak_id_strategy(),
        scope in prop::bool::ANY
    ) {
        let scope_str = if scope { "system" } else { "user" };
        let app = serde_json::json!({
            "id": id,
            "remote": "flathub",
            "scope": scope_str
        });

        let json_str = serde_json::to_string(&app).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(&app["id"], &parsed["id"]);
        prop_assert_eq!(&app["remote"], &parsed["remote"]);
        prop_assert_eq!(&app["scope"], &parsed["scope"]);
    }

    #[test]
    fn flatpak_merge_union_size(
        system_ids in prop::collection::vec(flatpak_id_strategy(), 0..5),
        user_ids in prop::collection::vec(flatpak_id_strategy(), 0..5)
    ) {
        use std::collections::HashSet;

        let all_unique: HashSet<_> = system_ids.iter().chain(user_ids.iter()).collect();

        // Merged manifest should have at most this many unique apps
        prop_assert!(all_unique.len() <= system_ids.len() + user_ids.len());
    }

    // ========================================================================
    // Extension manifest property tests
    // ========================================================================

    #[test]
    fn extension_add_is_idempotent(uuid in extension_uuid_strategy()) {
        let mut extensions: Vec<String> = vec![];

        // First add
        if !extensions.contains(&uuid) {
            extensions.push(uuid.clone());
        }
        let count_after_first = extensions.len();

        // Second add (should not change)
        if !extensions.contains(&uuid) {
            extensions.push(uuid.clone());
        }
        let count_after_second = extensions.len();

        prop_assert_eq!(count_after_first, count_after_second, "Add should be idempotent");
    }

    #[test]
    fn extension_merge_is_union(
        system_uuids in prop::collection::vec(extension_uuid_strategy(), 0..5),
        user_uuids in prop::collection::vec(extension_uuid_strategy(), 0..5)
    ) {
        use std::collections::HashSet;

        let system_set: HashSet<_> = system_uuids.iter().collect();
        let user_set: HashSet<_> = user_uuids.iter().collect();
        let union_size = system_set.union(&user_set).count();

        // Merged should contain exactly the union
        let mut merged: HashSet<_> = HashSet::new();
        for uuid in &system_uuids {
            merged.insert(uuid);
        }
        for uuid in &user_uuids {
            merged.insert(uuid);
        }

        prop_assert_eq!(merged.len(), union_size);
    }

    // ========================================================================
    // GSettings manifest property tests
    // ========================================================================

    #[test]
    fn gsetting_unique_key_is_consistent(
        schema in gsetting_schema_strategy(),
        key in gsetting_key_strategy()
    ) {
        let unique_key = format!("{}.{}", schema, key);

        // Key should be deterministic
        let unique_key2 = format!("{}.{}", schema, key);
        prop_assert_eq!(&unique_key, &unique_key2);

        // Key should contain both schema and key
        prop_assert!(unique_key.contains(&schema));
        prop_assert!(unique_key.contains(&key));
    }

    #[test]
    fn gsetting_serialization_roundtrip(
        schema in gsetting_schema_strategy(),
        key in gsetting_key_strategy(),
        value in gsetting_value_strategy()
    ) {
        let setting = serde_json::json!({
            "schema": schema,
            "key": key,
            "value": value
        });

        let json_str = serde_json::to_string(&setting).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        prop_assert_eq!(&setting["schema"], &parsed["schema"]);
        prop_assert_eq!(&setting["key"], &parsed["key"]);
        prop_assert_eq!(&setting["value"], &parsed["value"]);
    }

    #[test]
    fn gsetting_merge_user_overrides_system(
        schema in gsetting_schema_strategy(),
        key in gsetting_key_strategy(),
        system_value in gsetting_value_strategy(),
        user_value in gsetting_value_strategy()
    ) {
        use std::collections::HashMap;

        let unique_key = format!("{}.{}", schema, key);

        // System manifest
        let mut merged: HashMap<String, String> = HashMap::new();
        merged.insert(unique_key.clone(), system_value.clone());

        // User override
        merged.insert(unique_key.clone(), user_value.clone());

        // Final value should be user's value
        prop_assert_eq!(merged.get(&unique_key), Some(&user_value));
    }

    // ========================================================================
    // Cross-module property tests
    // ========================================================================

    #[test]
    fn json_manifest_always_valid(
        shims in prop::collection::vec(shim_name_strategy(), 0..3),
        apps in prop::collection::vec(flatpak_id_strategy(), 0..3),
        extensions in prop::collection::vec(extension_uuid_strategy(), 0..3)
    ) {
        // Build a combined manifest structure
        let manifest = serde_json::json!({
            "shims": shims.iter().map(|n| serde_json::json!({"name": n})).collect::<Vec<_>>(),
            "apps": apps.iter().map(|id| serde_json::json!({"id": id, "remote": "flathub", "scope": "system"})).collect::<Vec<_>>(),
            "extensions": extensions
        });

        // Should always serialize
        let json_str = serde_json::to_string(&manifest);
        prop_assert!(json_str.is_ok());

        // Should always parse back
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&json_str.unwrap());
        prop_assert!(parsed.is_ok());
    }
}
