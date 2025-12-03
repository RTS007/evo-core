//! Linux-specific shared memory operations

use crate::error::{ShmError, ShmResult};
use memmap2::{MmapMut, MmapOptions};
use nix::unistd::getpid;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::RawFd;

/// Linux-specific memory mapping configuration
pub struct LinuxMemoryConfig {
    /// Use MAP_LOCKED for RT performance
    pub locked: bool,
    /// Use MAP_HUGETLB for large segments (>2MB)
    pub huge_pages: bool,
    /// NUMA node binding
    pub numa_node: Option<u32>,
    /// NUMA memory policy
    pub numa_policy: NumaPolicy,
}

/// NUMA memory allocation policy
#[derive(Debug, Clone, Copy)]
pub enum NumaPolicy {
    /// Default system policy
    Default,
    /// Bind to specific NUMA node
    Bind(u32),
    /// Prefer specific NUMA node but allow fallback
    Preferred(u32),
    /// Interleave across NUMA nodes
    Interleave,
}

impl Default for LinuxMemoryConfig {
    fn default() -> Self {
        Self {
            locked: true,
            huge_pages: false,
            numa_node: None,
            numa_policy: NumaPolicy::Default,
        }
    }
}

/// Create memory-mapped segment with Linux-specific optimizations
pub fn create_segment_mmap(
    path: &str,
    size: usize,
    config: &LinuxMemoryConfig,
) -> Result<MmapMut, ShmError> {
    // Create or open the shared memory file
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600) // Owner read/write only
        .open(path)?;

    // Set file size
    file.set_len(size as u64)?;

    // Create memory mapping
    let mut mmap_options = MmapOptions::new();

    if config.locked {
        // Lock pages in memory for RT performance
        mmap_options.populate();
    }

    let mmap = unsafe { mmap_options.map_mut(&file)? };

    Ok(mmap)
}

/// Attach to existing segment
pub fn attach_segment_mmap(path: &str) -> ShmResult<MmapMut> {
    let file = OpenOptions::new().read(true).write(true).open(path)?;

    let mmap = unsafe { MmapOptions::new().map_mut(&file)? };
    Ok(mmap)
}

/// Process death detection using pidfd (Linux 5.1+)
pub fn setup_process_death_detection(pid: u32) -> Result<RawFd, ShmError> {
    // For now, return error since pidfd is not available in nix 0.27
    // This will be enhanced when pidfd support is stabilized
    Err(ShmError::ProcessNotFound { pid })
}

/// Check if process is alive using kill(pid, 0)
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;

        // Use a null signal (None) to test for process existence without sending a signal
        match kill(Pid::from_raw(pid as i32), None) {
            Ok(_) => true,
            Err(nix::Error::ESRCH) => false, // No such process
            Err(nix::Error::EPERM) => true,  // Process exists but no permission to signal
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Fallback for non-Linux systems
        false
    }
}

/// Get current process ID
pub fn get_current_pid() -> u32 {
    getpid().as_raw() as u32
}

/// Apply NUMA memory policy to a memory region
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn apply_numa_policy(
    addr: *mut std::ffi::c_void,
    size: usize,
    policy: NumaPolicy,
) -> ShmResult<()> {
    match policy {
        NumaPolicy::Default => Ok(()),
        NumaPolicy::Bind(node) => {
            // Use syscall directly for mbind
            let nodemask = 1u64 << node;
            let result = unsafe {
                libc::syscall(
                    libc::SYS_mbind,
                    addr,
                    size,
                    2, // MPOL_BIND
                    &nodemask as *const u64,
                    64, // maxnode
                    0,  // flags
                )
            };

            if result == 0 {
                Ok(())
            } else {
                Err(ShmError::Io {
                    source: std::io::Error::last_os_error(),
                })
            }
        }
        NumaPolicy::Preferred(node) => {
            let nodemask = 1u64 << node;
            let result = unsafe {
                libc::syscall(
                    libc::SYS_mbind,
                    addr,
                    size,
                    1, // MPOL_PREFERRED
                    &nodemask as *const u64,
                    64,
                    0,
                )
            };

            if result == 0 {
                Ok(())
            } else {
                Err(ShmError::Io {
                    source: std::io::Error::last_os_error(),
                })
            }
        }
        NumaPolicy::Interleave => {
            let result = unsafe {
                libc::syscall(
                    libc::SYS_mbind,
                    addr,
                    size,
                    3, // MPOL_INTERLEAVE
                    std::ptr::null::<u64>(),
                    0,
                    0,
                )
            };

            if result == 0 {
                Ok(())
            } else {
                Err(ShmError::Io {
                    source: std::io::Error::last_os_error(),
                })
            }
        }
    }
}

/// Enable huge pages for memory region (Linux-specific)
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn enable_huge_pages(addr: *mut std::ffi::c_void, size: usize) -> ShmResult<()> {
    // Use madvise to suggest huge page usage
    let result = unsafe { libc::madvise(addr, size, libc::MADV_HUGEPAGE) };

    if result == 0 {
        Ok(())
    } else {
        Err(ShmError::Io {
            source: std::io::Error::last_os_error(),
        })
    }
}

/// Check if huge pages are available
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn huge_pages_available() -> bool {
    std::fs::read_to_string("/proc/meminfo")
        .map(|content| content.contains("HugePages_Total"))
        .unwrap_or(false)
}

/// Get optimal NUMA node for current thread
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn get_current_numa_node() -> ShmResult<u32> {
    use std::fs;

    // Read current CPU from /proc/self/stat
    let stat = fs::read_to_string("/proc/self/stat").map_err(|e| ShmError::Io { source: e })?;

    let fields: Vec<&str> = stat.split_whitespace().collect();
    if fields.len() < 39 {
        return Err(ShmError::Io {
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid /proc/self/stat format",
            ),
        });
    }

    // Field 38 is the CPU number (0-indexed)
    let cpu: u32 = fields[38].parse().map_err(|_| ShmError::Io {
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid CPU number"),
    })?;

    // Read NUMA node for this CPU
    let node_path = format!("/sys/devices/system/cpu/cpu{}/node", cpu);
    match fs::read_to_string(&node_path) {
        Ok(content) => content.trim().parse().map_err(|_| ShmError::Io {
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid NUMA node number",
            ),
        }),
        Err(_) => Ok(0), // Fallback to node 0 if topology info not available
    }
}

/// Stub implementations for non-Linux platforms
#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub fn apply_numa_policy(
    _addr: *mut std::ffi::c_void,
    _size: usize,
    _policy: NumaPolicy,
) -> ShmResult<()> {
    Ok(()) // No-op on non-Linux
}

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub fn enable_huge_pages(_addr: *mut std::ffi::c_void, _size: usize) -> ShmResult<()> {
    Ok(()) // No-op on non-Linux
}

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub fn huge_pages_available() -> bool {
    false
}

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub fn get_current_numa_node() -> ShmResult<u32> {
    Ok(0) // Always return node 0 on non-Linux
}
