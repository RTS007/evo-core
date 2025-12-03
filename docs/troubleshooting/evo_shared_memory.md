# EVO Shared Memory Troubleshooting Guide

This guide helps diagnose and resolve common issues with the EVO Shared Memory.

## Quick Diagnostic Commands

### Check Segment Status
```bash
# List all active shared memory segments
ipcs -m

# Show segment details with process information  
ipcs -mp

# Check EVO-specific segments
ipcs -m | grep evo_

# Monitor memory usage
watch -n 1 'free -h && echo "=== SHM ===" && ipcs -m'
```

### Performance Monitoring
```bash
# Check RT kernel configuration
uname -a | grep PREEMPT
cat /sys/kernel/realtime 2>/dev/null

# Monitor CPU isolation
cat /proc/cmdline | grep isolcpus

# Check huge page allocation
cat /proc/meminfo | grep -i huge
```

### Process Monitoring
```bash
# Find EVO processes
ps aux | grep evo

# Check RT scheduling
ps -eo pid,cls,pri,ni,comm | grep evo

# Monitor file descriptors
lsof | grep "/dev/shm"
```

---

## Common Issues

### 1. Segment Not Found

**Symptom**: `ShmError::SegmentNotFound("segment_name")`

**Causes & Solutions**:

- **Producer not running**: Verify the writer process is active
  ```bash
  ps aux | grep <producer_process>
  ```

- **Segment name mismatch**: Check spelling and case sensitivity
  ```rust
  // Ensure consistent naming
  let writer = SegmentWriter::create("sensor_data", SHM_MIN_SIZE)?; // Producer
  let reader = SegmentReader::open("sensor_data")?;         // Consumer
  ```

- **Permissions issue**: Check segment access permissions
  ```bash
  ls -la /dev/shm/ | grep evo
  # Should show: -rw-rw-r-- for shared access
  ```

- **Cleanup after crash**: Remove orphaned segments
  ```bash
  # Remove specific segment
  rm /dev/shm/evo_sensor_data
  
  # Or use cleanup utility
  cargo run --example cleanup_segments
  ```

### 2. Permission Denied

**Symptom**: `ShmError::PermissionDenied("Access denied")`

**Solutions**:

- **File permissions**: Fix segment file permissions
  ```bash
  sudo chmod 664 /dev/shm/evo_*
  sudo chown :evo /dev/shm/evo_*
  ```

- **User groups**: Add user to EVO group
  ```bash
  sudo usermod -a -G evo $USER
  newgrp evo  # Activate group membership
  ```

- **SELinux context**: Fix SELinux policies (if enabled)
  ```bash
  setsebool -P allow_execmem=on
  setsebool -P allow_execstack=on
  ```

### 3. Allocation Failed

**Symptom**: `ShmError::AllocationFailed("Memory allocation failed")`

**Causes & Solutions**:

- **Insufficient memory**: Check available memory
  ```bash
  free -h
  cat /proc/meminfo | grep -E "(MemAvailable|MemFree)"
  ```

- **SHM limits exceeded**: Increase shared memory limits
  ```bash
  # Check current limits
  cat /proc/sys/kernel/shmmax
  cat /proc/sys/kernel/shmall
  
  # Increase limits (temporary)
  sudo sysctl -w kernel.shmmax=68719476736    # 64GB
  sudo sysctl -w kernel.shmall=16777216       # 64GB in pages
  
  # Permanent (add to /etc/sysctl.conf)
  echo "kernel.shmmax = 68719476736" | sudo tee -a /etc/sysctl.conf
  echo "kernel.shmall = 16777216" | sudo tee -a /etc/sysctl.conf
  ```

- **Memory fragmentation**: Restart system or use huge pages
  ```bash
  # Check fragmentation
  cat /proc/buddyinfo
  
  # Enable huge pages
  echo 1024 | sudo tee /proc/sys/vm/nr_hugepages
  ```

### 4. Version Conflicts

**Symptom**: Unexpected version numbers or sequence gaps

**Diagnostics**:
```rust
// Add version tracking to your reader
let mut last_version = 0;
loop {
    match reader.read() {
        Ok((data, version)) => {
            if version != last_version + 1 && last_version != 0 {
                eprintln!("Version gap: {} -> {}", last_version, version);
            }
            last_version = version;
        }
        Err(e) => eprintln!("Read error: {}", e),
    }
}
```

**Solutions**:
- **Multiple writers**: Ensure single writer per segment
- **Writer restarts**: Implement proper writer lifecycle management
- **Reader synchronization**: Handle version gaps gracefully in application logic

---

## Performance Problems

### 1. High Latency

**Target Performance**: < 100ns read, < 500ns write

**Diagnostics**:
```rust
use std::time::Instant;

// Measure read latency
let start = Instant::now();
let (data, version) = reader.read()?;
let latency = start.elapsed();
if latency > Duration::from_nanos(100) {
    println!("High read latency: {:?}", latency);
}

// Measure write latency  
let start = Instant::now();
writer.write(data)?;
let latency = start.elapsed();
if latency > Duration::from_nanos(500) {
    println!("High write latency: {:?}", latency);
}
```

**Solutions**:

- **CPU affinity**: Pin processes to specific cores
  ```bash
  taskset -c 2 ./your_evo_app  # Pin to core 2
  ```
  
- **RT priority**: Set real-time scheduling
  ```rust
  use evo_shared_memory::platform::linux::set_rt_priority;
  set_rt_priority(99)?;  // Highest RT priority
  ```

- **Memory alignment**: Ensure proper data structure alignment
  ```rust
  #[repr(C, align(64))]  // Cache line alignment
  struct OptimizedData {
      value: u64,
      // ... fields
  }
  ```

- **NUMA locality**: Bind to specific NUMA nodes
  ```bash
  numactl --cpubind=0 --membind=0 ./your_evo_app
  ```

### 2. High Jitter

**Target**: < 50ns P99.9 jitter

**Diagnostics**:
```rust
// Collect timing samples
let mut latencies = Vec::new();
for _ in 0..10000 {
    let start = Instant::now();
    let _ = reader.read()?;
    latencies.push(start.elapsed().as_nanos());
}

latencies.sort();
let p99_9 = latencies[latencies.len() * 999 / 1000];
println!("P99.9 latency: {}ns", p99_9);
```

**Solutions**:

- **RT kernel**: Use PREEMPT_RT kernel
  ```bash
  uname -a | grep PREEMPT
  # Should show: PREEMPT_RT or PREEMPT
  ```

- **CPU isolation**: Isolate CPUs from kernel tasks
  ```bash
  # Add to kernel command line
  isolcpus=2-7 nohz_full=2-7 rcu_nocbs=2-7
  ```

- **Interrupt routing**: Route interrupts away from RT CPUs
  ```bash
  # Disable irqbalance
  sudo systemctl stop irqbalance
  
  # Route all interrupts to CPU 0
  echo 1 | sudo tee /proc/irq/*/smp_affinity
  ```

### 3. Low Throughput

**Target**: > 1M writes/sec, > 10M reads/sec per reader

**Diagnostics**:
```bash
# Monitor system load
iostat 1
vmstat 1
top -p $(pgrep evo)
```

**Solutions**:

- **Batch operations**: Process multiple items per cycle
- **Polling optimization**: Adjust polling frequency
- **Memory prefetch**: Enable prefetch hints
- **Lock-free algorithms**: Verify lock-free operation

---

## Memory Issues

### 1. Memory Leaks

**Symptoms**: Growing memory usage, eventual allocation failures

**Diagnostics**:
```bash
# Monitor process memory
watch -n 1 'ps -o pid,vsz,rss,comm -p $(pgrep evo)'

# Check shared memory usage
watch -n 1 'ipcs -m'

# Memory profiling with valgrind
valgrind --tool=memcheck --leak-check=full ./your_evo_app
```

**Solutions**:

- **Proper cleanup**: Ensure segments are cleaned up
  ```rust
  impl Drop for MyApplication {
      fn drop(&mut self) {
          // Explicit cleanup
          let _ = self.shm_manager.cleanup_all_segments();
      }
  }
  ```

- **Lifecycle management**: Use ShmLifecycleManager
  ```rust
  let mut lifecycle = ShmLifecycleManager::new()?;
  lifecycle.enable_auto_cleanup(Duration::from_secs(30))?;
  ```

### 2. Memory Corruption

**Symptoms**: Invalid data, checksum failures, crashes

**Diagnostics**:
```rust
// Enable corruption detection
let reader = SegmentReader::open_with_options("data", 
    ReaderOptions {
        enable_checksums: true,
        corruption_detection: true,
    })?;

match reader.read() {
    Ok((data, version)) => {
        // Validate data integrity
        if !data.validate_checksum() {
            eprintln!("Data corruption detected!");
        }
    }
    Err(ShmError::DataCorruption(msg)) => {
        eprintln!("Corruption: {}", msg);
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

**Solutions**:

- **Memory barriers**: Ensure proper ordering
- **Atomic operations**: Use atomic updates for version counters
- **Validation**: Implement data checksums and validation

### 3. Huge Page Issues

**Symptoms**: Allocation failures for large segments

**Diagnostics**:
```bash
# Check huge page status
cat /proc/meminfo | grep -i huge
cat /sys/kernel/mm/hugepages/hugepages-*/nr_hugepages
```

**Solutions**:

- **Allocate huge pages**: Reserve huge pages at boot
  ```bash
  # Kernel command line
  hugepagesz=2M hugepages=1024
  
  # Runtime allocation
  echo 1024 | sudo tee /proc/sys/vm/nr_hugepages
  ```

- **Mount hugetlbfs**: Ensure proper filesystem mount
  ```bash
  sudo mkdir -p /mnt/huge
  sudo mount -t hugetlbfs none /mnt/huge
  ```

---

## Real-Time Violations

### 1. Deadline Misses

**Symptoms**: Control loops missing timing deadlines

**Monitoring**:
```rust
use std::time::{Duration, Instant};

let deadline = Duration::from_micros(100); // 100μs deadline
let mut missed_deadlines = 0;

loop {
    let start = Instant::now();
    
    // Critical real-time work
    process_control_loop()?;
    
    let elapsed = start.elapsed();
    if elapsed > deadline {
        missed_deadlines += 1;
        eprintln!("Deadline miss #{}: {:?} > {:?}", 
                 missed_deadlines, elapsed, deadline);
    }
    
    // RT sleep until next cycle
    if elapsed < deadline {
        thread::sleep(deadline - elapsed);
    }
}
```

**Solutions**:

- **Priority configuration**: Increase RT priority
  ```rust
  use evo_shared_memory::platform::linux::set_rt_priority;
  set_rt_priority(99)?;  // Maximum priority
  ```

- **CPU isolation**: Dedicate CPUs to RT tasks
- **Memory locking**: Lock memory pages
  ```rust
  use evo_shared_memory::platform::linux::lock_memory;
  lock_memory()?;  // Prevent page swapping
  ```

### 2. Priority Inversion

**Symptoms**: High-priority tasks blocked by low-priority tasks

**Solutions**:

- **Priority inheritance**: Use proper mutexes
- **Avoid blocking operations**: Use lock-free algorithms
- **RT scheduling**: Configure SCHED_FIFO properly

---

## Integration Problems

### 1. Module Communication Issues

**Diagnostics**:
```bash
# Check all EVO processes
ps aux | grep evo_ | while read line; do
    echo "Process: $line"
    pid=$(echo $line | awk '{print $2}')
    lsof -p $pid | grep "/dev/shm"
done
```

**Solutions**:

- **Startup sequencing**: Ensure proper module startup order
- **Health checks**: Implement module health monitoring
- **Graceful degradation**: Handle module failures

### 2. Version Compatibility

**Symptoms**: Incompatible data formats between modules

**Solutions**:

- **Schema versioning**: Include version in data structures
  ```rust
  #[repr(C)]
  struct VersionedData {
      schema_version: u32,  // Always first field
      data: ActualData,
  }
  ```

- **Backward compatibility**: Support multiple versions
- **Migration tools**: Provide data migration utilities

---

## Platform-Specific Issues

### Linux Issues

1. **Tmpfs not mounted**: Ensure `/dev/shm` is available
   ```bash
   mount | grep shm
   # Should show: tmpfs on /dev/shm type tmpfs
   ```

2. **RT kernel not available**: Install PREEMPT_RT kernel
   ```bash
   # Check current kernel
   uname -a
   
   # Install RT kernel (Ubuntu/Debian)
   sudo apt install linux-image-rt-amd64
   ```

3. **Permission model**: Configure systemd or udev rules
   ```bash
   # Create udev rule for EVO devices
   cat > /etc/udev/rules.d/99-evo-shm.rules << EOF
   KERNEL=="shm/evo_*", GROUP="evo", MODE="0664"
   EOF
   ```

---

## Debug Tools and Utilities

### 1. Built-in Diagnostics

```rust
// Enable debug logging
use evo_shared_memory::init_tracing;
init_tracing();

// Use debug reader
let reader = SegmentReader::open_debug("segment_name")?;
```

### 2. External Tools

```bash
# Shared memory analysis
shmemanalyze.py /dev/shm/evo_*

# Performance profiling
perf record -g ./your_evo_app
perf report

# System call tracing
strace -e shmat,shmget,shmctl ./your_evo_app

# Memory debugging
valgrind --tool=drd --trace-children=yes ./your_evo_app
```

### 3. Custom Monitoring

```rust
// Create monitoring application
use evo_shared_memory::{SegmentDiscovery, ShmLifecycleManager};

fn monitor_all_segments() -> ShmResult<()> {
    let discovery = SegmentDiscovery::new()?;
    let segments = discovery.list_all_segments()?;
    
    for segment_info in segments {
        println!("Segment: {}", segment_info.name);
        println!("  Size: {} bytes", segment_info.size);
        println!("  Readers: {}", segment_info.reader_count);
        println!("  Writer PID: {}", segment_info.writer_pid);
        println!("  Last update: {:?}", segment_info.last_update);
    }
    
    Ok(())
}
```

---

## Getting Help

### Support Channels

- **GitHub Issues**: Report bugs and feature requests
- **Documentation**: Check the API documentation and examples
- **Community**: Join the EVO community discussions

### Bug Reports

Include the following information:

1. **Environment**:
   - OS version: `uname -a`
   - Kernel type: `uname -r`
   - EVO version: `cargo --version`

2. **Configuration**:
   - RT setup: `/proc/cmdline`
   - Memory limits: `ulimit -a`
   - Shared memory: `ipcs -lm`

3. **Logs**:
   - Application logs with debug enabled
   - System logs: `journalctl -xe`
   - Performance data if relevant

4. **Reproduction**:
   - Minimal example demonstrating the issue
   - Steps to reproduce
   - Expected vs actual behavior

### Performance Optimization Consultation

For production deployments requiring specific performance guarantees:

1. **System profiling**: Provide detailed performance requirements
2. **Configuration review**: Share system configuration
3. **Benchmark results**: Include performance measurements
4. **Use case analysis**: Describe your specific application patterns

---

*This troubleshooting guide is continuously updated based on user feedback and field experience. For the latest version, check the documentation repository.*