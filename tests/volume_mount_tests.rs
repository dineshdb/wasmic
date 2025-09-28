use std::collections::HashMap;
use tempfile::TempDir;
use wasic::config::{ComponentConfig, Profile, VolumeMount};
use wasic::linker::create_wasi_context;

#[test]
fn test_create_wasi_context_with_volume_mounts() {
    // Create a temporary directory for testing
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    // Create a test file in the temp directory
    let test_file_path = temp_path.join("test.txt");
    std::fs::write(&test_file_path, "test content").expect("Failed to write test file");

    // Create volume mounts
    let volume_mounts = vec![
        VolumeMount {
            host_path: temp_path.to_string_lossy().to_string(),
            guest_path: "/tmp".to_string(),
            read_only: false,
        },
        VolumeMount {
            host_path: test_file_path.to_string_lossy().to_string(),
            guest_path: "/tmp/test.txt".to_string(),
            read_only: true,
        },
    ];

    // Create a component config with volume mounts
    let mut components = HashMap::new();
    components.insert(
        "test_component".to_string(),
        ComponentConfig {
            path: Some("test.wasm".to_string()),
            oci: None,
            config: None,
            volumes: volume_mounts,
            cwd: Some(temp_path.to_string_lossy().to_string()),
            env: HashMap::new(),
            description: None,
        },
    );

    // Create a profile with the component
    let profile = Profile {
        components,
        description: None,
    };

    // Test creating WASI context with volume mounts
    let component_config = profile.components.get("test_component").unwrap();
    let result = create_wasi_context(component_config);
    assert!(
        result.is_ok(),
        "Failed to create WASI context with volume mounts: {:?}",
        result.err()
    );

    let _context = result.unwrap();
    // The context was created successfully - that's our main test
    // We can't easily inspect the internal state of wasmtime-wasi 37.0
    assert!(true, "WASI context created successfully with volume mounts");
}

#[test]
fn test_create_wasi_context_with_invalid_path() {
    // Create volume mounts with non-existent path
    let volume_mounts = vec![VolumeMount {
        host_path: "/nonexistent/path".to_string(),
        guest_path: "/tmp".to_string(),
        read_only: false,
    }];

    // Create a component config with invalid volume mounts
    let mut components = HashMap::new();
    components.insert(
        "test_component".to_string(),
        ComponentConfig {
            path: Some("test.wasm".to_string()),
            oci: None,
            config: None,
            volumes: volume_mounts,
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            description: None,
        },
    );

    // Create a profile with the component
    let profile = Profile {
        components,
        description: None,
    };

    // Test creating WASI context with invalid volume mounts
    let component_config = profile.components.get("test_component").unwrap();
    let result = create_wasi_context(component_config);
    assert!(
        result.is_err(),
        "Expected error when creating WASI context with invalid path"
    );
}

#[test]
fn test_create_wasi_context_with_empty_mounts() {
    // Create a component config with no volume mounts
    let mut components = HashMap::new();
    components.insert(
        "test_component".to_string(),
        ComponentConfig {
            path: Some("test.wasm".to_string()),
            oci: None,
            config: None,
            volumes: Vec::new(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            description: None,
        },
    );

    // Create a profile with the component
    let profile = Profile {
        components,
        description: None,
    };

    // Test creating WASI context with no volume mounts
    let component_config = profile.components.get("test_component").unwrap();
    let result = create_wasi_context(component_config);
    assert!(
        result.is_ok(),
        "Failed to create WASI context with empty volume mounts: {:?}",
        result.err()
    );

    let _ = result.unwrap();
    // The context was created successfully - that's our main test
    // We can't easily inspect the internal state of wasmtime-wasi 37.0
    assert!(
        true,
        "WASI context created successfully with empty volume mounts"
    );
}
