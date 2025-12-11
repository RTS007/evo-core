#!/bin/bash
# EVO Shared Memory Performance Regression Detection
# 
# This script runs comprehensive performance benchmarks and detects regressions
# by comparing results against established baselines. Designed for CI/CD integration.

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BENCHMARK_DIR="$PROJECT_ROOT/target/benchmark_results"
BASELINE_DIR="$PROJECT_ROOT/.ci/performance_baselines"
REPORT_DIR="$PROJECT_ROOT/target/regression_reports"
TEMP_DIR="$(mktemp -d)"

# Performance thresholds (percentage increase that triggers failure)
LATENCY_THRESHOLD=10     # 10% increase in latency
THROUGHPUT_THRESHOLD=5   # 5% decrease in throughput  
JITTER_THRESHOLD=20      # 20% increase in jitter
MEMORY_THRESHOLD=15      # 15% increase in memory usage

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1" >&2
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1" >&2
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1" >&2
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

# Cleanup function
cleanup() {
    local exit_code=$?
    log_info "Cleaning up temporary files..."
    rm -rf "$TEMP_DIR"
    exit $exit_code
}
trap cleanup EXIT

# Help function
show_help() {
    cat << EOF
EVO Shared Memory Performance Regression Detection

USAGE:
    $0 [OPTIONS]

OPTIONS:
    -h, --help              Show this help message
    -b, --benchmark-only    Run benchmarks without regression analysis
    -a, --analyze-only      Analyze existing results without running benchmarks
    -u, --update-baseline   Update baseline with current results
    -f, --force             Continue even if some benchmarks fail
    -v, --verbose           Enable verbose output
    --latency-threshold N   Set latency regression threshold (default: ${LATENCY_THRESHOLD}%)
    --throughput-threshold N Set throughput regression threshold (default: ${THROUGHPUT_THRESHOLD}%)
    --jitter-threshold N    Set jitter regression threshold (default: ${JITTER_THRESHOLD}%)
    --memory-threshold N    Set memory regression threshold (default: ${MEMORY_THRESHOLD}%)

EXAMPLES:
    # Run full regression detection
    $0

    # Update baseline after performance improvements
    $0 --update-baseline

    # Run with custom thresholds
    $0 --latency-threshold 5 --throughput-threshold 10

    # Analyze existing results only
    $0 --analyze-only

ENVIRONMENT VARIABLES:
    CI                      Set to 'true' to enable CI mode
    BENCHMARK_DURATION     Duration for benchmarks in seconds (default: 30)
    BENCHMARK_THREADS      Number of concurrent threads (default: 4)
    RT_PRIORITY           Enable RT scheduling (default: false)

EOF
}

# Parse command line arguments
BENCHMARK_ONLY=false
ANALYZE_ONLY=false
UPDATE_BASELINE=false
FORCE=false
VERBOSE=false
CI_MODE=${CI:-false}

while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -b|--benchmark-only)
            BENCHMARK_ONLY=true
            shift
            ;;
        -a|--analyze-only)
            ANALYZE_ONLY=true
            shift
            ;;
        -u|--update-baseline)
            UPDATE_BASELINE=true
            shift
            ;;
        -f|--force)
            FORCE=true
            shift
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        --latency-threshold)
            LATENCY_THRESHOLD="$2"
            shift 2
            ;;
        --throughput-threshold)
            THROUGHPUT_THRESHOLD="$2"
            shift 2
            ;;
        --jitter-threshold)
            JITTER_THRESHOLD="$2"
            shift 2
            ;;
        --memory-threshold)
            MEMORY_THRESHOLD="$2"
            shift 2
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Environment setup
setup_environment() {
    log_info "Setting up performance testing environment..."
    
    # Create directories
    mkdir -p "$BENCHMARK_DIR" "$BASELINE_DIR" "$REPORT_DIR"
    
    # Check if running in CI
    if [[ "$CI_MODE" == "true" ]]; then
        log_info "Running in CI mode"
        # In CI, use shorter durations to avoid timeouts
        export BENCHMARK_DURATION=${BENCHMARK_DURATION:-10}
        export BENCHMARK_THREADS=${BENCHMARK_THREADS:-2}
    else
        export BENCHMARK_DURATION=${BENCHMARK_DURATION:-30}
        export BENCHMARK_THREADS=${BENCHMARK_THREADS:-4}
    fi
    
    # Check RT capabilities (non-critical)
    if [[ "${RT_PRIORITY:-false}" == "true" ]]; then
        if ! command -v chrt &> /dev/null; then
            log_warning "chrt not available - RT scheduling disabled"
        else
            log_info "RT scheduling enabled"
        fi
    fi
    
    # System information
    log_info "System information:"
    echo "  CPU: $(nproc) cores"
    echo "  Memory: $(free -h | grep '^Mem:' | awk '{print $2}')"
    echo "  Kernel: $(uname -r)"
    echo "  Benchmark duration: ${BENCHMARK_DURATION}s"
    echo "  Benchmark threads: ${BENCHMARK_THREADS}"
}

# Build benchmarks
build_benchmarks() {
    log_info "Building performance benchmarks..."
    
    cd "$PROJECT_ROOT"
    
    # Build with optimizations
    if [[ "$VERBOSE" == "true" ]]; then
        cargo build --release --bins --examples
    else
        cargo build --release --bins --examples > /dev/null 2>&1
    fi

    echo -e "$PROJECT_ROOT"
    
    # Verify benchmark binaries exist
    local benchmarks=(
        "./target/release/examples/shm_basic_usage1"  
        "./target/release/examples/shm_basic_usage2"
        "./target/release/examples/shm_1_evo_integration"
        "./target/release/examples/shm_high_throughput"
    )
    
    for benchmark in "${benchmarks[@]}"; do
        if [[ ! -f "$benchmark" ]]; then
            log_error "Benchmark binary not found: $benchmark"
            return 1
        fi
    done
    
    log_success "Benchmarks built successfully"
}

# Run individual benchmark
run_benchmark() {
    local name="$1"
    local binary="$2"
    local output_file="$3"
    
    log_info "Running benchmark: $name"
    
    local start_time=$(date +%s.%N)
    local result_file="$TEMP_DIR/${name}_result.json"
    
    # Prepare benchmark environment
    export BENCHMARK_NAME="$name"
    export BENCHMARK_OUTPUT="$result_file"
    export RUST_LOG=${RUST_LOG:-info}
    
    # Run with timeout
    local timeout_duration=$((BENCHMARK_DURATION + 30))
    
    if timeout "${timeout_duration}s" "$binary" > "$TEMP_DIR/${name}_output.log" 2>&1; then
        local end_time=$(date +%s.%N)
        local duration=$(echo "$end_time - $start_time" | bc -l)
        
        # Parse results (this would be customized based on actual benchmark output)
        local latency_avg=$(grep "Average latency" "$TEMP_DIR/${name}_output.log" | awk '{print $3}' | sed 's/[^0-9.]//g' || echo "0")
        local latency_p99=$(grep "P99 latency" "$TEMP_DIR/${name}_output.log" | awk '{print $3}' | sed 's/[^0-9.]//g' || echo "0")
        local throughput=$(grep "Throughput" "$TEMP_DIR/${name}_output.log" | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0")
        local jitter=$(grep "Jitter" "$TEMP_DIR/${name}_output.log" | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0")
        local memory_mb=$(grep "Memory usage" "$TEMP_DIR/${name}_output.log" | awk '{print $3}' | sed 's/[^0-9.]//g' || echo "0")
        
        # Create JSON result
        cat > "$output_file" << EOF
{
    "benchmark": "$name",
    "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "duration": $duration,
    "system": {
        "cpu_cores": $(nproc),
        "memory_gb": $(free -g | grep '^Mem:' | awk '{print $2}'),
        "kernel": "$(uname -r)"
    },
    "metrics": {
        "latency_avg_ns": $latency_avg,
        "latency_p99_ns": $latency_p99,
        "throughput_ops_sec": $throughput,
        "jitter_ns": $jitter,
        "memory_mb": $memory_mb
    },
    "success": true
}
EOF
        
        log_success "Benchmark $name completed (${duration}s)"
        return 0
    else
        # Benchmark failed
        local end_time=$(date +%s.%N)
        local duration=$(echo "$end_time - $start_time" | bc -l)
        
        cat > "$output_file" << EOF
{
    "benchmark": "$name",
    "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "duration": $duration,
    "success": false,
    "error": "Benchmark timeout or failure"
}
EOF
        
        log_error "Benchmark $name failed after ${duration}s"
        if [[ "$FORCE" != "true" ]]; then
            return 1
        fi
        return 0
    fi
}

# Run all benchmarks
run_benchmarks() {
    log_info "Running performance benchmarks..."
    
    local timestamp=$(date +%Y%m%d_%H%M%S)
    local results_dir="$BENCHMARK_DIR/$timestamp"
    mkdir -p "$results_dir"
    
    # Define benchmarks
    local benchmarks=(
        "shm_basic_usage1:./target/release/examples/shm_basic_usage1"  
        "shm_basic_usage2:./target/release/examples/shm_basic_usage2"
        "shm_1_evo_integration:./target/release/examples/shm_1_evo_integration"
        "shm_high_throughput:./target/release/examples/shm_high_throughput"
    )
    
    local failed_count=0
    
    for benchmark_spec in "${benchmarks[@]}"; do
        local name="${benchmark_spec%%:*}"
        local binary="${benchmark_spec##*:}"
        local output_file="$results_dir/${name}.json"
        
        if ! run_benchmark "$name" "$binary" "$output_file"; then
            ((failed_count++))
        fi
    done
    
    # Create summary
    cat > "$results_dir/summary.json" << EOF
{
    "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "total_benchmarks": ${#benchmarks[@]},
    "failed_benchmarks": $failed_count,
    "success": $([ $failed_count -eq 0 ] && echo "true" || echo "false"),
    "results_dir": "$results_dir"
}
EOF
    
    # Store latest results path
    echo "$results_dir" > "$BENCHMARK_DIR/latest"
    
    if [[ $failed_count -eq 0 ]]; then
        log_success "All benchmarks completed successfully"
    else
        log_warning "$failed_count benchmarks failed"
        if [[ "$FORCE" != "true" ]]; then
            return 1
        fi
    fi
    
    return 0
}

# Compare metric with baseline
compare_metric() {
    local metric_name="$1"
    local current_value="$2" 
    local baseline_value="$3"
    local threshold="$4"
    local higher_is_worse="$5"  # true/false
    
    if [[ "$baseline_value" == "0" ]] || [[ -z "$baseline_value" ]]; then
        echo "UNKNOWN"
        return 0
    fi
    
    local change_percent=$(echo "scale=2; ($current_value - $baseline_value) * 100 / $baseline_value" | bc -l)
    
    local regression=false
    if [[ "$higher_is_worse" == "true" ]]; then
        # For metrics where higher is worse (latency, jitter, memory)
        if (( $(echo "$change_percent > $threshold" | bc -l) )); then
            regression=true
        fi
    else
        # For metrics where lower is worse (throughput)
        if (( $(echo "$change_percent < -$threshold" | bc -l) )); then
            regression=true
        fi
    fi
    
    if [[ "$regression" == "true" ]]; then
        echo "REGRESSION"
        return 1
    elif (( $(echo "$change_percent > 0" | bc -l) )) && [[ "$higher_is_worse" == "false" ]]; then
        echo "IMPROVEMENT"
        return 0
    elif (( $(echo "$change_percent < 0" | bc -l) )) && [[ "$higher_is_worse" == "true" ]]; then
        echo "IMPROVEMENT"
        return 0
    else
        echo "STABLE"
        return 0
    fi
}

# Analyze regression for a single benchmark
analyze_benchmark_regression() {
    local current_file="$1"
    local baseline_file="$2"
    local report_file="$3"
    
    if [[ ! -f "$baseline_file" ]]; then
        log_warning "No baseline found for $(basename "$current_file")"
        return 0
    fi
    
    local benchmark_name=$(jq -r '.benchmark' "$current_file")
    
    # Extract metrics
    local curr_latency_avg=$(jq -r '.metrics.latency_avg_ns // 0' "$current_file")
    local curr_latency_p99=$(jq -r '.metrics.latency_p99_ns // 0' "$current_file")
    local curr_throughput=$(jq -r '.metrics.throughput_ops_sec // 0' "$current_file")
    local curr_jitter=$(jq -r '.metrics.jitter_ns // 0' "$current_file")
    local curr_memory=$(jq -r '.metrics.memory_mb // 0' "$current_file")
    
    local base_latency_avg=$(jq -r '.metrics.latency_avg_ns // 0' "$baseline_file")
    local base_latency_p99=$(jq -r '.metrics.latency_p99_ns // 0' "$baseline_file")
    local base_throughput=$(jq -r '.metrics.throughput_ops_sec // 0' "$baseline_file")
    local base_jitter=$(jq -r '.metrics.jitter_ns // 0' "$baseline_file")
    local base_memory=$(jq -r '.metrics.memory_mb // 0' "$baseline_file")
    
    # Analyze each metric
    local latency_avg_status=$(compare_metric "latency_avg" "$curr_latency_avg" "$base_latency_avg" "$LATENCY_THRESHOLD" "true")
    local latency_p99_status=$(compare_metric "latency_p99" "$curr_latency_p99" "$base_latency_p99" "$LATENCY_THRESHOLD" "true")
    local throughput_status=$(compare_metric "throughput" "$curr_throughput" "$base_throughput" "$THROUGHPUT_THRESHOLD" "false")
    local jitter_status=$(compare_metric "jitter" "$curr_jitter" "$base_jitter" "$JITTER_THRESHOLD" "true")
    local memory_status=$(compare_metric "memory" "$curr_memory" "$base_memory" "$MEMORY_THRESHOLD" "true")
    
    # Determine overall status
    local overall_status="PASS"
    if [[ "$latency_avg_status" == "REGRESSION" ]] || \
       [[ "$latency_p99_status" == "REGRESSION" ]] || \
       [[ "$throughput_status" == "REGRESSION" ]] || \
       [[ "$jitter_status" == "REGRESSION" ]] || \
       [[ "$memory_status" == "REGRESSION" ]]; then
        overall_status="FAIL"
    fi
    
    # Create detailed report
    cat > "$report_file" << EOF
{
    "benchmark": "$benchmark_name",
    "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "overall_status": "$overall_status",
    "thresholds": {
        "latency_threshold_percent": $LATENCY_THRESHOLD,
        "throughput_threshold_percent": $THROUGHPUT_THRESHOLD,
        "jitter_threshold_percent": $JITTER_THRESHOLD,
        "memory_threshold_percent": $MEMORY_THRESHOLD
    },
    "metrics": {
        "latency_avg": {
            "current": $curr_latency_avg,
            "baseline": $base_latency_avg,
            "change_percent": $(echo "scale=2; ($curr_latency_avg - $base_latency_avg) * 100 / $base_latency_avg" | bc -l 2>/dev/null || echo "0"),
            "status": "$latency_avg_status"
        },
        "latency_p99": {
            "current": $curr_latency_p99,
            "baseline": $base_latency_p99,
            "change_percent": $(echo "scale=2; ($curr_latency_p99 - $base_latency_p99) * 100 / $base_latency_p99" | bc -l 2>/dev/null || echo "0"),
            "status": "$latency_p99_status"
        },
        "throughput": {
            "current": $curr_throughput,
            "baseline": $base_throughput,
            "change_percent": $(echo "scale=2; ($curr_throughput - $base_throughput) * 100 / $base_throughput" | bc -l 2>/dev/null || echo "0"),
            "status": "$throughput_status"
        },
        "jitter": {
            "current": $curr_jitter,
            "baseline": $base_jitter,
            "change_percent": $(echo "scale=2; ($curr_jitter - $base_jitter) * 100 / $base_jitter" | bc -l 2>/dev/null || echo "0"),
            "status": "$jitter_status"
        },
        "memory": {
            "current": $curr_memory,
            "baseline": $base_memory,
            "change_percent": $(echo "scale=2; ($curr_memory - $base_memory) * 100 / $base_memory" | bc -l 2>/dev/null || echo "0"),
            "status": "$memory_status"
        }
    }
}
EOF
    
    # Log results
    if [[ "$overall_status" == "FAIL" ]]; then
        log_error "Regression detected in $benchmark_name"
        return 1
    else
        log_success "No regression in $benchmark_name"
        return 0
    fi
}

# Analyze all results for regressions
analyze_regressions() {
    log_info "Analyzing performance regressions..."
    
    local latest_dir=$(cat "$BENCHMARK_DIR/latest" 2>/dev/null || echo "")
    if [[ -z "$latest_dir" ]] || [[ ! -d "$latest_dir" ]]; then
        log_error "No benchmark results found"
        return 1
    fi
    
    local timestamp=$(basename "$latest_dir")
    local regression_report_dir="$REPORT_DIR/$timestamp"
    mkdir -p "$regression_report_dir"
    
    local regression_count=0
    local total_benchmarks=0
    
    # Analyze each benchmark
    for result_file in "$latest_dir"/*.json; do
        [[ -f "$result_file" ]] || continue
        [[ "$(basename "$result_file")" != "summary.json" ]] || continue
        
        local benchmark_name=$(basename "$result_file" .json)
        local baseline_file="$BASELINE_DIR/${benchmark_name}.json"
        local report_file="$regression_report_dir/${benchmark_name}.json"
        
        ((total_benchmarks++))
        
        if ! analyze_benchmark_regression "$result_file" "$baseline_file" "$report_file"; then
            ((regression_count++))
        fi
    done
    
    # Create summary report
    local overall_status="PASS"
    if [[ $regression_count -gt 0 ]]; then
        overall_status="FAIL"
    fi
    
    cat > "$regression_report_dir/summary.json" << EOF
{
    "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "overall_status": "$overall_status",
    "total_benchmarks": $total_benchmarks,
    "regressions_detected": $regression_count,
    "thresholds": {
        "latency_threshold_percent": $LATENCY_THRESHOLD,
        "throughput_threshold_percent": $THROUGHPUT_THRESHOLD,
        "jitter_threshold_percent": $JITTER_THRESHOLD,
        "memory_threshold_percent": $MEMORY_THRESHOLD
    }
}
EOF
    
    # Store latest report path
    echo "$regression_report_dir" > "$REPORT_DIR/latest"
    
    # Print summary
    if [[ $regression_count -gt 0 ]]; then
        log_error "Performance regressions detected: $regression_count/$total_benchmarks benchmarks"
        return 1
    else
        log_success "No performance regressions detected ($total_benchmarks benchmarks analyzed)"
        return 0
    fi
}

# Update baselines with current results
update_baselines() {
    log_info "Updating performance baselines..."
    
    local latest_dir=$(cat "$BENCHMARK_DIR/latest" 2>/dev/null || echo "")
    if [[ -z "$latest_dir" ]] || [[ ! -d "$latest_dir" ]]; then
        log_error "No benchmark results found to use as baseline"
        return 1
    fi
    
    local updated_count=0
    
    for result_file in "$latest_dir"/*.json; do
        [[ -f "$result_file" ]] || continue
        [[ "$(basename "$result_file")" != "summary.json" ]] || continue
        
        local benchmark_name=$(basename "$result_file" .json)
        local baseline_file="$BASELINE_DIR/${benchmark_name}.json"
        
        # Verify the result is successful before using as baseline
        local success=$(jq -r '.success // false' "$result_file")
        if [[ "$success" == "true" ]]; then
            cp "$result_file" "$baseline_file"
            ((updated_count++))
            log_info "Updated baseline for $benchmark_name"
        else
            log_warning "Skipping failed benchmark $benchmark_name for baseline update"
        fi
    done
    
    log_success "Updated $updated_count baselines"
    return 0
}

# Generate human-readable report
generate_report() {
    local latest_report_dir=$(cat "$REPORT_DIR/latest" 2>/dev/null || echo "")
    if [[ -z "$latest_report_dir" ]] || [[ ! -d "$latest_report_dir" ]]; then
        log_warning "No regression analysis report found"
        return 0
    fi
    
    local summary_file="$latest_report_dir/summary.json"
    if [[ ! -f "$summary_file" ]]; then
        log_warning "No summary report found"
        return 0
    fi
    
    local overall_status=$(jq -r '.overall_status' "$summary_file")
    local total_benchmarks=$(jq -r '.total_benchmarks' "$summary_file")
    local regressions=$(jq -r '.regressions_detected' "$summary_file")
    
    echo
    echo "=========================================="
    echo "EVO Shared Memory Performance Report"
    echo "=========================================="
    echo "Timestamp: $(jq -r '.timestamp' "$summary_file")"
    echo "Overall Status: $overall_status"
    echo "Benchmarks Analyzed: $total_benchmarks"
    echo "Regressions Detected: $regressions"
    echo
    
    if [[ $regressions -gt 0 ]]; then
        echo "REGRESSIONS DETECTED:"
        echo "===================="
        
        for report_file in "$latest_report_dir"/*.json; do
            [[ -f "$report_file" ]] || continue
            [[ "$(basename "$report_file")" != "summary.json" ]] || continue
            
            local status=$(jq -r '.overall_status' "$report_file")
            if [[ "$status" == "FAIL" ]]; then
                local benchmark=$(jq -r '.benchmark' "$report_file")
                echo "- $benchmark:"
                
                # Show regressed metrics
                for metric in latency_avg latency_p99 throughput jitter memory; do
                    local metric_status=$(jq -r ".metrics.${metric}.status" "$report_file")
                    if [[ "$metric_status" == "REGRESSION" ]]; then
                        local change=$(jq -r ".metrics.${metric}.change_percent" "$report_file")
                        echo "  * ${metric}: ${change}% change"
                    fi
                done
            fi
        done
    else
        echo "✅ No performance regressions detected!"
    fi
    
    echo
    echo "Report details: $latest_report_dir"
    echo "=========================================="
}

# Main execution
main() {
    log_info "EVO Shared Memory Performance Regression Detection"
    
    # Setup
    setup_environment
    
    # Check for required tools
    for tool in jq bc timeout; do
        if ! command -v "$tool" &> /dev/null; then
            log_error "Required tool not found: $tool"
            exit 1
        fi
    done
    
    # Execute based on mode
    if [[ "$ANALYZE_ONLY" != "true" ]]; then
        build_benchmarks || exit 1
        run_benchmarks || exit 1
    fi
    
    if [[ "$UPDATE_BASELINE" == "true" ]]; then
        update_baselines || exit 1
    fi
    
    if [[ "$BENCHMARK_ONLY" != "true" ]] && [[ "$UPDATE_BASELINE" != "true" ]]; then
        analyze_regressions || exit 1
    fi
    
    # Always generate report if we have results
    generate_report
    
    log_success "Performance regression detection completed"
}

# Run main function
main "$@"