//! Memory monitoring and alerting system for shared memory segments

use crate::discovery::SegmentDiscovery;
use crate::error::{ShmError, ShmResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime};

/// Configuration for memory monitoring system
#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    /// Check interval for monitoring cycles
    pub check_interval: Duration,
    /// Memory usage threshold for alerts (percentage)
    pub memory_threshold: f64,
    /// Maximum number of orphaned segments before alert
    pub orphan_threshold: usize,
    /// Performance degradation threshold (percentage)
    pub performance_threshold: f64,
    /// Minimum time between identical alerts
    pub alert_cooldown: Duration,
    /// Maximum number of historical metrics to keep
    pub history_size: usize,
    /// Log file path for alerts
    pub log_path: Option<String>,
    /// Enable detailed performance monitoring
    pub detailed_monitoring: bool,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            memory_threshold: 85.0,
            orphan_threshold: 5,
            performance_threshold: 20.0,
            alert_cooldown: Duration::from_secs(300),
            history_size: 1000,
            log_path: Some("/var/log/evo_shm_monitor.log".to_string()),
            detailed_monitoring: false,
        }
    }
}

/// Memory usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    /// Total system memory in bytes
    pub total_memory: u64,
    /// Available memory in bytes
    pub available_memory: u64,
    /// Total shared memory usage in bytes
    pub total_shm_usage: u64,
    /// Number of active segments
    pub active_segments: usize,
    /// Number of orphaned segments
    pub orphaned_segments: usize,
    /// Timestamp of measurement
    pub timestamp: SystemTime,
}

/// Performance metrics for a monitoring cycle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Average read latency in nanoseconds
    pub avg_read_latency: u64,
    /// Average write latency in nanoseconds
    pub avg_write_latency: u64,
    /// Operations per second
    pub ops_per_second: f64,
    /// Memory throughput in bytes per second
    pub throughput_bps: f64,
    /// CPU usage percentage
    pub cpu_usage: f64,
    /// Timestamp of measurement
    pub timestamp: SystemTime,
}

/// Alert types that can be generated
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlertType {
    /// High memory usage alert
    HighMemoryUsage,
    /// Memory leak detected
    MemoryLeak,
    /// Performance degradation
    PerformanceDegradation,
    /// Deadline violation
    DeadlineViolation,
    /// Orphaned segment detected
    OrphanedSegment,
    /// Segment corruption
    SegmentCorruption,
    /// System overload
    SystemOverload,
    /// Writer process death
    WriterProcessDeath,
}

/// Alert severity levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Information only
    Info,
    /// Warning - attention needed
    Warning,
    /// Error - action required
    Error,
    /// Critical - immediate action required
    Critical,
}

/// Alert information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Type of alert
    pub alert_type: AlertType,
    /// Alert severity
    pub severity: AlertSeverity,
    /// Alert message
    pub message: String,
    /// Affected segment name (if applicable)
    pub segment_name: Option<String>,
    /// Timestamp when alert was generated
    pub timestamp: SystemTime,
    /// Additional context data
    pub context: HashMap<String, String>,
}

/// Trait for handling alerts
pub trait AlertHandler: Send + Sync {
    /// Handle an alert
    fn handle_alert(&self, alert: &Alert) -> ShmResult<()>;
}

/// Console alert handler
pub struct ConsoleAlertHandler;

impl AlertHandler for ConsoleAlertHandler {
    fn handle_alert(&self, alert: &Alert) -> ShmResult<()> {
        println!(
            "[{}] {:?}: {}",
            alert
                .timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            alert.severity,
            alert.message
        );
        Ok(())
    }
}

/// Log file alert handler
pub struct LogFileAlertHandler {
    log_path: String,
}

impl LogFileAlertHandler {
    /// Create new log file handler
    pub fn new(log_path: String) -> Self {
        Self { log_path }
    }
}

impl AlertHandler for LogFileAlertHandler {
    fn handle_alert(&self, alert: &Alert) -> ShmResult<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| ShmError::Io { source: e })?;

        let json_alert = serde_json::to_string(alert).map_err(|e| ShmError::Io {
            source: std::io::Error::new(std::io::ErrorKind::Other, e),
        })?;

        writeln!(file, "{}", json_alert).map_err(|e| ShmError::Io { source: e })?;

        Ok(())
    }
}

/// Main memory monitoring system
pub struct MemoryMonitor {
    config: MonitoringConfig,
    discovery: SegmentDiscovery,
    running: Arc<AtomicBool>,
    metrics_history: Arc<Mutex<VecDeque<MemoryStats>>>,
    alert_handlers: Vec<Box<dyn AlertHandler>>,
    last_alert_times: Arc<Mutex<HashMap<AlertType, SystemTime>>>,
    total_alerts: AtomicU64,
    monitoring_cycles: AtomicU64,
}

impl MemoryMonitor {
    /// Create a new memory monitor with default configuration
    pub fn new() -> ShmResult<Self> {
        Self::with_config(MonitoringConfig::default())
    }

    /// Create a new memory monitor with custom configuration
    pub fn with_config(config: MonitoringConfig) -> ShmResult<Self> {
        let history_size = config.history_size; // Extract before move
        Ok(Self {
            config,
            discovery: SegmentDiscovery::new(),
            running: Arc::new(AtomicBool::new(false)),
            metrics_history: Arc::new(Mutex::new(VecDeque::with_capacity(history_size))),
            alert_handlers: Vec::new(),
            last_alert_times: Arc::new(Mutex::new(HashMap::new())),
            total_alerts: AtomicU64::new(0),
            monitoring_cycles: AtomicU64::new(0),
        })
    }

    /// Add an alert handler
    pub fn add_alert_handler(&mut self, handler: Box<dyn AlertHandler>) {
        self.alert_handlers.push(handler);
    }

    /// Start monitoring in background thread
    pub fn start_monitoring(&self) -> ShmResult<()> {
        if self.running.load(Ordering::Relaxed) {
            return Err(ShmError::Io {
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "Monitor already running",
                ),
            });
        }

        self.running.store(true, Ordering::Relaxed);

        // Clone necessary data for background thread
        let running = Arc::clone(&self.running);
        let config = self.config.clone();
        let metrics_history = Arc::clone(&self.metrics_history);
        let last_alert_times = Arc::clone(&self.last_alert_times);

        std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                if let Err(e) = Self::monitoring_cycle(&config, &metrics_history, &last_alert_times)
                {
                    eprintln!("Monitoring cycle error: {}", e);
                }

                std::thread::sleep(config.check_interval);
            }
        });

        Ok(())
    }

    /// Stop monitoring
    pub fn stop_monitoring(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// Check if monitoring is active
    pub fn is_monitoring(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get current memory statistics
    pub fn get_memory_stats(&self) -> ShmResult<MemoryStats> {
        let discovery = &self.discovery;
        let segments = discovery.list_segments()?;

        let mut total_shm_usage = 0;
        let mut active_segments = 0;
        let mut orphaned_segments = 0;

        for segment in &segments {
            total_shm_usage += segment.size as u64;
            if crate::platform::is_process_alive(segment.writer_pid) {
                active_segments += 1;
            } else {
                orphaned_segments += 1;
            }
        }

        let (total_memory, available_memory) = get_system_memory_info()?;

        Ok(MemoryStats {
            total_memory,
            available_memory,
            total_shm_usage,
            active_segments,
            orphaned_segments,
            timestamp: SystemTime::now(),
        })
    }

    /// Get historical memory statistics
    pub fn get_memory_history(&self) -> Vec<MemoryStats> {
        self.metrics_history
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect()
    }

    /// Get total number of alerts generated
    pub fn get_total_alerts(&self) -> u64 {
        self.total_alerts.load(Ordering::Relaxed)
    }

    /// Get number of monitoring cycles completed
    pub fn get_monitoring_cycles(&self) -> u64 {
        self.monitoring_cycles.load(Ordering::Relaxed)
    }

    /// Force a monitoring cycle for testing
    pub fn force_monitoring_cycle(&self) -> ShmResult<()> {
        Self::monitoring_cycle(&self.config, &self.metrics_history, &self.last_alert_times)
    }

    /// Internal monitoring cycle implementation
    fn monitoring_cycle(
        config: &MonitoringConfig,
        metrics_history: &Arc<Mutex<VecDeque<MemoryStats>>>,
        last_alert_times: &Arc<Mutex<HashMap<AlertType, SystemTime>>>,
    ) -> ShmResult<()> {
        let discovery = SegmentDiscovery::new();
        let segments = discovery.list_segments()?;

        let mut total_shm_usage = 0;
        let mut active_segments = 0;
        let mut orphaned_segments = 0;

        for segment in &segments {
            total_shm_usage += segment.size as u64;
            if crate::platform::is_process_alive(segment.writer_pid) {
                active_segments += 1;
            } else {
                orphaned_segments += 1;
                // Log orphaned segment for potential cleanup
                eprintln!(
                    "Orphaned segment detected: {} (writer PID {} not alive)",
                    segment.name, segment.writer_pid
                );
            }
        }

        let (total_memory, available_memory) = get_system_memory_info()?;

        let stats = MemoryStats {
            total_memory,
            available_memory,
            total_shm_usage,
            active_segments,
            orphaned_segments,
            timestamp: SystemTime::now(),
        };

        // Store metrics in history
        {
            let mut history = metrics_history.lock().unwrap();
            if history.len() >= config.history_size {
                history.pop_front();
            }
            history.push_back(stats.clone());
        }

        // Check for alert conditions
        let memory_usage_percent =
            (stats.total_shm_usage as f64 / stats.total_memory as f64) * 100.0;

        if memory_usage_percent > config.memory_threshold {
            Self::maybe_send_alert(
                &AlertType::HighMemoryUsage,
                AlertSeverity::Warning,
                format!("High shared memory usage: {:.1}%", memory_usage_percent),
                None,
                last_alert_times,
                config.alert_cooldown,
            )?;
        }

        if stats.orphaned_segments > config.orphan_threshold {
            Self::maybe_send_alert(
                &AlertType::OrphanedSegment,
                AlertSeverity::Error,
                format!("Too many orphaned segments: {}", stats.orphaned_segments),
                None,
                last_alert_times,
                config.alert_cooldown,
            )?;
        }

        Ok(())
    }

    /// Send alert if cooldown period has passed
    fn maybe_send_alert(
        alert_type: &AlertType,
        severity: AlertSeverity,
        message: String,
        segment_name: Option<String>,
        last_alert_times: &Arc<Mutex<HashMap<AlertType, SystemTime>>>,
        cooldown: Duration,
    ) -> ShmResult<()> {
        let now = SystemTime::now();
        let mut last_times = last_alert_times.lock().unwrap();

        if let Some(last_time) = last_times.get(alert_type) {
            if now.duration_since(*last_time).unwrap_or(Duration::ZERO) < cooldown {
                return Ok(()); // Still in cooldown period
            }
        }

        // Send the alert
        let alert = Alert {
            alert_type: alert_type.clone(),
            severity,
            message,
            segment_name,
            timestamp: now,
            context: HashMap::new(),
        };

        // For now, just log to console since we can't access handlers from static context
        println!("ALERT: {:?} - {}", alert.severity, alert.message);

        last_times.insert(alert.alert_type.clone(), alert.timestamp);
        Ok(())
    }
}

// Utility functions

/// Get memory statistics for a specific segment
pub fn get_segment_memory_stats(segment_name: &str) -> ShmResult<(u64, f64)> {
    let discovery = SegmentDiscovery::new();
    let segments = discovery.list_segments()?;

    let segment = segments
        .iter()
        .find(|s| s.name == segment_name)
        .ok_or_else(|| ShmError::NotFound {
            name: segment_name.to_string(),
        })?;

    let used_memory = estimate_used_memory(&segment.name, segment.size as u64)?;
    let access_frequency = estimate_access_frequency(&segment.name)?;

    Ok((used_memory, access_frequency))
}

/// Estimate used memory for a segment (placeholder implementation)
fn estimate_used_memory(_segment_name: &str, allocated_size: u64) -> ShmResult<u64> {
    // For now, assume 80% usage - in real implementation this would
    // analyze the segment's internal structure
    Ok((allocated_size as f64 * 0.8) as u64)
}

/// Estimate access frequency for a segment (placeholder implementation)  
fn estimate_access_frequency(_segment_name: &str) -> ShmResult<f64> {
    // Placeholder - would track actual access patterns in real implementation
    Ok(100.0) // operations per second
}

/// Get system memory information from /proc/meminfo
fn get_system_memory_info() -> ShmResult<(u64, u64)> {
    let meminfo =
        std::fs::read_to_string("/proc/meminfo").map_err(|e| ShmError::Io { source: e })?;

    let mut total_memory = 0;
    let mut available_memory = 0;

    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            if let Some(value) = line.split_whitespace().nth(1) {
                total_memory = value.parse::<u64>().unwrap_or(0) * 1024; // Convert KB to bytes
            }
        } else if line.starts_with("MemAvailable:") {
            if let Some(value) = line.split_whitespace().nth(1) {
                available_memory = value.parse::<u64>().unwrap_or(0) * 1024; // Convert KB to bytes
            }
        }
    }

    Ok((total_memory, available_memory))
}

/// Check if an alert should be throttled based on recent similar alerts
#[allow(dead_code)]
fn should_throttle_alert(
    alert_type: &AlertType,
    last_alert_times: &Arc<Mutex<HashMap<AlertType, SystemTime>>>,
    cooldown: Duration,
) -> bool {
    let last_times = last_alert_times.lock().unwrap();
    if let Some(last_time) = last_times.get(alert_type) {
        if let Ok(elapsed) = SystemTime::now().duration_since(*last_time) {
            return elapsed < cooldown;
        }
    }
    false
}

/// Generate system health report
pub fn generate_health_report() -> ShmResult<String> {
    let discovery = SegmentDiscovery::new();
    let segments = discovery.list_segments()?;
    let (total_memory, available_memory) = get_system_memory_info()?;

    let mut report = String::new();
    report.push_str("=== EVO Shared Memory Health Report ===\n\n");

    report.push_str(&format!("System Memory:\n"));
    report.push_str(&format!(
        "  Total: {:.2} GB\n",
        total_memory as f64 / 1024.0 / 1024.0 / 1024.0
    ));
    report.push_str(&format!(
        "  Available: {:.2} GB\n",
        available_memory as f64 / 1024.0 / 1024.0 / 1024.0
    ));

    report.push_str(&format!("\nShared Memory Segments: {}\n", segments.len()));

    let mut total_shm_usage = 0;
    let mut active_count = 0;
    let mut orphaned_count = 0;

    for segment in &segments {
        total_shm_usage += segment.size as u64;
        if crate::platform::is_process_alive(segment.writer_pid) {
            active_count += 1;
        } else {
            orphaned_count += 1;
        }
    }

    report.push_str(&format!("  Active: {}\n", active_count));
    report.push_str(&format!("  Orphaned: {}\n", orphaned_count));
    report.push_str(&format!(
        "  Total Usage: {:.2} MB\n",
        total_shm_usage as f64 / 1024.0 / 1024.0
    ));
    report.push_str(&format!(
        "  Usage Percentage: {:.1}%\n",
        (total_shm_usage as f64 / total_memory as f64) * 100.0
    ));

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitoring_config_default() {
        let config = MonitoringConfig::default();
        assert_eq!(config.memory_threshold, 85.0);
        assert_eq!(config.orphan_threshold, 5);
    }

    #[test]
    fn test_memory_monitor_creation() {
        let monitor = MemoryMonitor::new().unwrap();
        assert!(!monitor.is_monitoring());
        assert_eq!(monitor.get_total_alerts(), 0);
    }

    #[test]
    fn test_alert_severity_ordering() {
        assert!(AlertSeverity::Critical > AlertSeverity::Error);
        assert!(AlertSeverity::Error > AlertSeverity::Warning);
        assert!(AlertSeverity::Warning > AlertSeverity::Info);
    }
}
