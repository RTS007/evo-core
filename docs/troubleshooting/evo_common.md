# EVO Common Troubleshooting Guide

This guide helps diagnose and resolve common issues with the `evo_common` library.

## Quick Reference

### Crate Import Setup

```toml
# Cargo.toml - use evo alias for shorter imports
[dependencies]
evo = { package = "evo_common", path = "../evo_common" }
```

```rust
// Then use short imports
use evo::shm::consts::*;
use evo::config::{ConfigLoader, SharedConfig};
use evo::prelude::*;
```

---

## Common Issues

### 1. ConfigError::FileNotFound

**Symptom**: `ConfigError::FileNotFound` when calling `ConfigLoader::load()`

**Causes & Solutions**:

- **File path incorrect**: Verify the path exists
  ```bash
  # Check if file exists
  ls -la /path/to/config.toml
  
  # Use absolute paths in production
  let config = MyConfig::load(Path::new("/etc/evo/config.toml"))?;
  ```

- **Working directory mismatch**: Relative paths resolve from CWD
  ```bash
  # Check current working directory
  pwd
  
  # In Rust, verify the path
  println!("Looking for: {:?}", std::fs::canonicalize("config.toml"));
  ```

- **Permissions issue**: Check file read permissions
  ```bash
  chmod 644 /path/to/config.toml
  ```

### 2. ConfigError::ParseError

**Symptom**: `ConfigError::ParseError("...")` with TOML parsing details

**Causes & Solutions**:

- **Invalid TOML syntax**: Validate your TOML file
  ```bash
  # Install TOML validator
  cargo install toml-cli
  
  # Validate file
  toml check config.toml
  ```

- **Missing required fields**: Ensure all non-optional fields are present
  ```toml
  # SharedConfig requires service_name
  [shared]
  service_name = "my-service"  # Required!
  log_level = "info"           # Optional, defaults to "info"
  ```

- **Wrong field types**: Check that values match expected types
  ```toml
  # Correct
  port = 8080          # number
  enabled = true       # boolean
  name = "service"     # string
  
  # Wrong
  port = "8080"        # string, not number!
  ```

- **Table order matters**: Root-level fields before tables
  ```toml
  # Correct: scalar fields first
  port = 8080
  
  [shared]
  service_name = "my-service"
  
  # Wrong: scalar after table
  [shared]
  service_name = "my-service"
  
  port = 8080  # This belongs to [shared] now!
  ```

### 3. ConfigError::ValidationError

**Symptom**: `ConfigError::ValidationError("service_name cannot be empty")`

**Causes & Solutions**:

- **Empty service_name**: Provide a non-empty value
  ```toml
  [shared]
  service_name = "evo-api-01"  # Must be non-empty
  ```

- **Validation not called**: `ConfigLoader::load()` does NOT auto-validate
  ```rust
  // ConfigLoader only parses - manual validation needed
  let config = MyConfig::load(path)?;
  config.shared.validate()?;  // Call validate explicitly!
  ```

### 4. LogLevel Serialization Issues

**Symptom**: Log level not recognized or serialized incorrectly

**Causes & Solutions**:

- **Case sensitivity**: Use lowercase values
  ```toml
  # Correct
  log_level = "debug"
  
  # Wrong
  log_level = "DEBUG"
  log_level = "Debug"
  ```

- **Valid values**: Only these are accepted
  ```toml
  log_level = "trace"  # Most verbose
  log_level = "debug"
  log_level = "info"   # Default
  log_level = "warn"
  log_level = "error"  # Least verbose
  ```

### 5. Import Conflicts with evo Binary

**Symptom**: Compilation errors when both `evo` binary and `evo_common` are used

**Causes & Solutions**:

- **Name collision**: The workspace has an `evo` binary crate
  ```toml
  # Use explicit package rename
  [dependencies]
  evo = { package = "evo_common", path = "../evo_common" }
  ```

- **Alternative: use full crate name**
  ```toml
  [dependencies]
  evo_common = { path = "../evo_common" }
  ```
  ```rust
  use evo_common::shm::consts::*;
  ```

---

## Configuration Examples

### Minimal Config

```toml
[shared]
service_name = "my-service"
```

### Full Config with App-Specific Fields

```toml
# App-specific fields at root level FIRST
port = 8080
database_url = "postgres://localhost/evo"

# Then shared config section
[shared]
log_level = "debug"
service_name = "evo-api-01"
```

### Corresponding Rust Struct

```rust
use evo::config::{ConfigLoader, SharedConfig, ConfigError};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct AppConfig {
    port: u16,
    database_url: String,
    shared: SharedConfig,
}

fn load_config() -> Result<AppConfig, ConfigError> {
    let config = AppConfig::load(Path::new("config.toml"))?;
    config.shared.validate()?;  // Don't forget validation!
    Ok(config)
}
```

---

## SHM Constants Reference

### Available Constants

```rust
use evo::shm::consts::*;

// Magic number for segment validation
// Value: 0x45564F5F53484D00 ("EVO_SHM\0")
EVO_SHM_MAGIC: u64

// Minimum segment size: 4KB (one memory page)
SHM_MIN_SIZE: usize

// Maximum segment size: 1GB
SHM_MAX_SIZE: usize

// Cache line size for alignment: 64 bytes
CACHE_LINE_SIZE: usize
```

### Migration from evo_shared_memory

If upgrading from direct `evo_shared_memory` constant usage:

```rust
// Old (deprecated)
use evo_shared_memory::{SHM_MIN_SIZE, SHM_MAX_SIZE};

// New (correct)
use evo::shm::consts::{SHM_MIN_SIZE, SHM_MAX_SIZE};
```

---

## Debugging Tips

### Enable Detailed Errors

```rust
fn main() {
    match AppConfig::load(Path::new("config.toml")) {
        Ok(config) => println!("Loaded: {:?}", config),
        Err(ConfigError::FileNotFound) => {
            eprintln!("Config file not found. Expected at: config.toml");
            eprintln!("Current directory: {:?}", std::env::current_dir());
        }
        Err(ConfigError::ParseError(msg)) => {
            eprintln!("TOML parse error: {}", msg);
            eprintln!("Check your config.toml syntax");
        }
        Err(ConfigError::ValidationError(msg)) => {
            eprintln!("Validation error: {}", msg);
        }
    }
}
```

### Verify Config Loading

```rust
// Print raw file content before parsing
let content = std::fs::read_to_string("config.toml")?;
println!("Raw config:\n{}", content);

// Then load
let config = AppConfig::load(Path::new("config.toml"))?;
println!("Parsed config: {:?}", config);
```

### Check Import Resolution

```bash
# Verify evo_common is in dependency tree
cargo tree -p evo_common

# Check for duplicate versions
cargo tree -d | grep evo_common
```

---

## Related Documentation

- [evo_shared_memory Troubleshooting](./evo_shared_memory.md)
- [API Documentation](../../target/doc/evo_common/index.html) - Run `cargo doc --open`
