// Test script to verify volume control configuration

use services::services::config::{Config, NotificationConfig};
use serde_json;

fn main() {
    println!("Testing volume control configuration...\n");

    // Test 1: Default configuration
    let default_config = Config::default();
    println!("✅ Default config created");
    println!("  - Version: {}", default_config.config_version);
    println!("  - Sound enabled: {}", default_config.notifications.sound_enabled);
    println!("  - Sound volume: {}", default_config.notifications.sound_volume);
    assert_eq!(default_config.notifications.sound_volume, 100);

    // Test 2: Migration from v7 to v8
    let v7_json = r#"{
        "config_version": "v7",
        "theme": "SYSTEM",
        "executor_profile": {"profile_id": "claude_code"},
        "disclaimer_acknowledged": true,
        "onboarding_acknowledged": true,
        "github_login_acknowledged": true,
        "telemetry_acknowledged": true,
        "notifications": {
            "sound_enabled": true,
            "push_enabled": false,
            "sound_file": "GENIE_NOTIFY1"
        },
        "editor": {"editor_type": "VSCODE"},
        "github": {},
        "show_release_notes": false,
        "language": "EN",
        "git_branch_prefix": "af"
    }"#;

    let migrated_config = Config::from(v7_json.to_string());
    println!("\n✅ Migration from v7 successful");
    println!("  - Version after migration: {}", migrated_config.config_version);
    println!("  - Sound volume after migration: {}", migrated_config.notifications.sound_volume);
    assert_eq!(migrated_config.config_version, "v8");
    assert_eq!(migrated_config.notifications.sound_volume, 100); // Default for backward compatibility

    // Test 3: Serialization with volume
    let mut custom_config = Config::default();
    custom_config.notifications.sound_volume = 50;

    let serialized = serde_json::to_string_pretty(&custom_config).unwrap();
    println!("\n✅ Serialization with custom volume (50%)");

    // Test 4: Deserialization with volume
    let v8_json = r#"{
        "config_version": "v8",
        "theme": "DARK",
        "executor_profile": {"profile_id": "claude_code"},
        "disclaimer_acknowledged": true,
        "onboarding_acknowledged": true,
        "github_login_acknowledged": true,
        "telemetry_acknowledged": true,
        "notifications": {
            "sound_enabled": true,
            "push_enabled": true,
            "sound_file": "ABSTRACT_SOUND1",
            "sound_volume": 75
        },
        "editor": {"editor_type": "CURSOR"},
        "github": {},
        "show_release_notes": false,
        "language": "EN",
        "git_branch_prefix": "forge"
    }"#;

    let deserialized: Config = serde_json::from_str(v8_json).unwrap();
    println!("\n✅ Deserialization with custom volume");
    println!("  - Sound volume from JSON: {}", deserialized.notifications.sound_volume);
    assert_eq!(deserialized.notifications.sound_volume, 75);

    println!("\n🎉 All tests passed! Volume control configuration is working correctly.");
}