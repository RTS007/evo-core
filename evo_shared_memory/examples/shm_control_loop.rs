//! Real-time control system using EVO shared memory
//! Demonstrates deterministic, low-latency communication patterns

use evo_shared_memory::SHM_MIN_SIZE;
use evo_shared_memory::{SegmentReader, SegmentWriter, ShmError, ShmResult};
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::{Duration, Instant};

/// Control command structure for RT control loop
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ControlCommand {
    setpoint: f64,
    kp: f32,
    ki: f32,
    kd: f32,
    enable: bool,
    emergency_stop: bool,
    timestamp: u64,
    sequence_id: u32,
}

/// Process feedback data from controlled system
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ProcessFeedback {
    process_value: f64,
    output_value: f64,
    error: f64,
    timestamp: u64,
    control_active: bool,
    sequence_id: u32,
}

fn serialize_data<T: Serialize>(data: &T) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(serde_json::to_vec(data)?)
}

fn deserialize_data<'a, T: Deserialize<'a>>(
    data: &'a [u8],
) -> Result<T, Box<dyn std::error::Error>> {
    // Create streaming deserializer.
    // It will read one valid JSON object and stop,
    // ignoring anything (zeros, garbage, old data) that comes after.
    let mut stream = serde_json::Deserializer::from_slice(data).into_iter::<T>();

    match stream.next() {
        Some(result) => Ok(result?),
        None => Err("No valid JSON found in buffer".into()),
    }
}

/// RT control loop statistics
#[derive(Debug)]
struct RTStatistics {
    cycles: u64,
    deadline_misses: u64,
    max_latency: Duration,
    min_latency: Duration,
    total_latency: Duration,
}

impl RTStatistics {
    fn new() -> Self {
        Self {
            cycles: 0,
            deadline_misses: 0,
            max_latency: Duration::ZERO,
            min_latency: Duration::from_secs(1),
            total_latency: Duration::ZERO,
        }
    }

    fn record_cycle(&mut self, latency: Duration, deadline_missed: bool) {
        self.cycles += 1;
        self.total_latency += latency;

        if latency > self.max_latency {
            self.max_latency = latency;
        }
        if latency < self.min_latency {
            self.min_latency = latency;
        }

        if deadline_missed {
            self.deadline_misses += 1;
        }
    }

    fn average_latency(&self) -> Duration {
        if self.cycles > 0 {
            self.total_latency / self.cycles as u32
        } else {
            Duration::ZERO
        }
    }

    fn deadline_miss_rate(&self) -> f64 {
        if self.cycles > 0 {
            (self.deadline_misses as f64) / (self.cycles as f64) * 100.0
        } else {
            0.0
        }
    }
}

/// PID Controller for real-time control
struct PIDController {
    kp: f32,
    ki: f32,
    kd: f32,
    integral: f64,
    last_error: f64,
    last_time: Instant,
}

impl PIDController {
    fn new(kp: f32, ki: f32, kd: f32) -> Self {
        Self {
            kp,
            ki,
            kd,
            integral: 0.0,
            last_error: 0.0,
            last_time: Instant::now(),
        }
    }

    fn update(&mut self, setpoint: f64, process_value: f64) -> f64 {
        let now = Instant::now();
        let dt = now.duration_since(self.last_time).as_secs_f64();

        let error = setpoint - process_value;

        // Proportional term
        let p_term = self.kp as f64 * error;

        // Integral term with windup protection
        self.integral += error * dt;
        let i_term = self.ki as f64 * self.integral;

        // Derivative term
        let derivative = if dt > 0.0 {
            (error - self.last_error) / dt
        } else {
            0.0
        };
        let d_term = self.kd as f64 * derivative;

        self.last_error = error;
        self.last_time = now;

        p_term + i_term + d_term
    }

    fn reset(&mut self) {
        self.integral = 0.0;
        self.last_error = 0.0;
        self.last_time = Instant::now();
    }
}

fn simulate_process_value(time: &Instant) -> f64 {
    // Simple simulation
    let elapsed = time.elapsed().as_secs_f64();
    50.0 + 10.0 * (elapsed * 0.2).sin()
}

fn main() -> ShmResult<()> {
    println!("EVO Shared Memory - Real-Time Control Loop Example");
    println!("=================================================");

    // Configure RT environment
    setup_rt_environment()?;

    // Start control supervisor
    let supervisor_handle = thread::spawn(|| {
        control_supervisor().unwrap_or_else(|e| {
            eprintln!("Control supervisor error: {}", e);
        });
    });

    // Start RT control loop
    let controller_handle = thread::spawn(|| {
        rt_control_loop().unwrap_or_else(|e| {
            eprintln!("RT control loop error: {}", e);
        });
    });

    // Start process simulation
    let process_handle = thread::spawn(|| {
        process_simulation().unwrap_or_else(|e| {
            eprintln!("Process simulation error: {}", e);
        });
    });

    // Run for 10 seconds
    thread::sleep(Duration::from_secs(10));

    // Wait for threads
    supervisor_handle.join().unwrap();
    controller_handle.join().unwrap();
    process_handle.join().unwrap();

    println!("RT control loop example completed!");
    Ok(())
}

fn setup_rt_environment() -> ShmResult<()> {
    // Note: In a real application, you would configure:
    // 1. RT kernel parameters
    // 2. CPU isolation
    // 3. Memory locking
    // 4. RT scheduling policies

    println!("Setting up RT environment...");

    // This is a placeholder - actual RT setup requires root privileges
    // and proper system configuration

    Ok(())
}

/// Supervisor that sends commands to the control loop
fn control_supervisor() -> ShmResult<()> {
    println!("Starting control supervisor...");

    // Create command channel
    let mut command_writer = SegmentWriter::create("rt_commands", SHM_MIN_SIZE)?;

    let start_time = Instant::now();
    let mut sequence_id = 0;

    loop {
        let elapsed = start_time.elapsed().as_secs_f64();

        // Generate time-varying setpoint (sine wave)
        let setpoint = 50.0 + 20.0 * (elapsed * 0.5).sin();

        let command = ControlCommand {
            setpoint,
            kp: 1.0,
            ki: 0.1,
            kd: 0.05,
            enable: true,
            emergency_stop: false,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
            sequence_id,
        };

        let data = serialize_data(&command).unwrap();
        command_writer.write(&data)?;
        sequence_id += 1;

        if elapsed > 10.0 {
            break;
        }

        // Update commands at 10 Hz
        thread::sleep(Duration::from_millis(100));
    }

    println!("Control supervisor completed");
    Ok(())
}

/// Real-time control loop with strict timing
fn rt_control_loop() -> ShmResult<()> {
    println!("Starting RT control loop...");

    // Wait for supervisor to create command channel
    thread::sleep(Duration::from_millis(50));

    // Open channels
    let mut command_reader = loop {
        match SegmentReader::attach("rt_commands") {
            Ok(r) => break r,
            Err(ShmError::NotFound { .. }) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => return Err(e),
        }
    };

    let mut feedback_writer = SegmentWriter::create("rt_feedback", SHM_MIN_SIZE)?;

    // Initialize PID controller
    let mut pid = PIDController::new(1.0, 0.1, 0.05);
    let mut stats = RTStatistics::new();

    // Control loop parameters
    let cycle_time = Duration::from_nanos(100000); // 10kHz control loop
    let mut current_command = ControlCommand {
        setpoint: 0.0,
        kp: 1.0,
        ki: 0.1,
        kd: 0.05,
        enable: false,
        emergency_stop: false,
        timestamp: 0,
        sequence_id: 0,
    };

    let start_time = Instant::now();

    loop {
        let cycle_start = Instant::now();
        let deadline = cycle_start + cycle_time;

        // Read new commands if available
        if command_reader.has_changed() {
            match command_reader.read() {
                Ok(bytes) => {
                    if let Ok(command) = deserialize_data::<ControlCommand>(bytes) {
                        current_command = command;

                        // Update PID parameters
                        pid.kp = current_command.kp;
                        pid.ki = current_command.ki;
                        pid.kd = current_command.kd;
                    }
                }
                Err(e) => {
                    eprintln!("RT Control: Command read error: {}", e);
                }
            }
        }

        // Emergency stop check
        if current_command.emergency_stop {
            pid.reset();
            println!("RT Control: Emergency stop activated!");
            break;
        }

        // Simulate process value (in real system, read from sensors)
        let process_value = simulate_process_value(&cycle_start);

        // Calculate control output
        let output = if current_command.enable {
            pid.update(current_command.setpoint, process_value)
        } else {
            pid.reset();
            0.0
        };

        // Create feedback
        let feedback = ProcessFeedback {
            process_value,
            output_value: output.clamp(-100.0, 100.0), // Clamp output
            error: current_command.setpoint - process_value,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
            control_active: current_command.enable,
            sequence_id: stats.cycles as u32,
        };

        // Write feedback
        let data = serialize_data(&feedback).unwrap();
        if let Err(e) = feedback_writer.write(&data) {
            eprintln!("RT Control: Feedback write error: {}", e);
        }

        // Check deadline compliance
        let cycle_end = Instant::now();
        let cycle_latency = cycle_end.duration_since(cycle_start);
        let deadline_missed = cycle_end > deadline;

        stats.record_cycle(cycle_latency, deadline_missed);

        // RT sleep until next cycle
        if let Some(remaining) = deadline.checked_duration_since(cycle_end) {
            thread::sleep(remaining);
        }

        // Stop after 10 seconds
        if start_time.elapsed() > Duration::from_secs(10) {
            break;
        }
    }

    // Print RT statistics
    println!("RT Control Loop Statistics:");
    println!("  Cycles: {}", stats.cycles);
    println!(
        "  Deadline misses: {} ({:.2}%)",
        stats.deadline_misses,
        stats.deadline_miss_rate()
    );
    println!("  Average latency: {:?}", stats.average_latency());
    println!("  Min latency: {:?}", stats.min_latency);
    println!("  Max latency: {:?}", stats.max_latency);

    Ok(())
}

/// Simulate the controlled process
fn process_simulation() -> ShmResult<()> {
    println!("Starting process simulation...");

    // Wait for control loop to start
    thread::sleep(Duration::from_millis(100));

    let mut feedback_reader = loop {
        match SegmentReader::attach("rt_feedback") {
            Ok(r) => {
                println!("Process: Successfully attached to 'rt_feedback'");
                break r;
            }
            Err(ShmError::NotFound { .. }) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => return Err(e),
        }
    };

    let start_time = Instant::now();
    let mut last_printed_sequence = 0;
    // Add flag to always print the first received packet
    let mut first_packet_received = false;

    loop {
        if feedback_reader.has_changed() {
            match feedback_reader.read() {
                Ok(bytes) => {
                    match deserialize_data::<ProcessFeedback>(&bytes) {
                        Ok(feedback) => {
                            // ALWAYS print first packet to confirm communication
                            // OR print every 500 cycles (reduced from 1000 for safety)
                            if !first_packet_received
                                || feedback.sequence_id >= last_printed_sequence + 500
                            {
                                println!(
                                    "Process: [Seq={}] PV={:.2}, Output={:.2}, Active={}",
                                    feedback.sequence_id,
                                    feedback.process_value,
                                    feedback.output_value,
                                    feedback.control_active
                                );

                                last_printed_sequence = feedback.sequence_id;
                                first_packet_received = true;
                            }
                        }
                        Err(e) => {
                            // IMPORTANT: Don't swallow errors!
                            eprintln!("Process: Deserialization error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Process: Feedback read error: {}", e);
                }
            }
        } else {
            // Optional: short sleep to avoid burning CPU in while(true) loop
            // if library doesn't block
            thread::sleep(Duration::from_millis(1));
        }

        if start_time.elapsed() > Duration::from_secs(10) {
            break;
        }

        // Remove long sleep 10ms, which could cause rhythm loss
        // Instead we rely on has_changed() and short sleep above
    }

    println!("Process simulation finished");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_controller() {
        let mut pid = PIDController::new(1.0, 0.1, 0.05);

        // Test basic proportional response
        let output = pid.update(100.0, 90.0); // Error = 10
        assert!(
            output > 0.0,
            "PID should produce positive output for positive error"
        );

        // Test setpoint reached
        let output = pid.update(100.0, 100.0); // Error = 0
        // Output should be small (just integral/derivative terms)

        pid.reset();
        assert_eq!(pid.integral, 0.0);
    }

    #[test]
    fn test_rt_statistics() {
        let mut stats = RTStatistics::new();

        stats.record_cycle(Duration::from_micros(100), false);
        stats.record_cycle(Duration::from_micros(200), true);

        assert_eq!(stats.cycles, 2);
        assert_eq!(stats.deadline_misses, 1);
        assert_eq!(stats.deadline_miss_rate(), 50.0);
        assert_eq!(stats.average_latency(), Duration::from_micros(150));
    }
}
