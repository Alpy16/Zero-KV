#!/bin/bash
set -e

# Zero-KV Lifecycle Automation Script
# This script handles the build, bake, serve, and benchmark cycle.

echo "=== Zero-KV Lifecycle Report ==="
echo "Environment: Linux / UDS (Unix Domain Socket)"
echo "Target Connections: 100"
echo "Pipeline Depth: 256"
echo "Dataset Size: 100,000 entries"
echo "--------------------------------"

echo "[1/5] Building release binaries..."
cargo build --release

echo "[2/5] Preparing test dataset (100,000 entries)..."
# Generates exactly 100,000 entries. Using awk for speed and portability.
seq 1 100000 | awk '{print $1 ",value_content_at_key_" $1}' > bench_input.csv

echo "[3/5] Baking the storage engine..."
rm -f storage.db
./target/release/baker bench_input.csv > /dev/null

echo "[4/5] Starting the engine..."
rm -f /tmp/zero-kv.sock
rm -f server.log

# Start the server in the background
RUST_LOG=info ./target/release/kv_store > server.log 2>&1 &
SERVER_PID=$!

# Trap exit signals to ensure the server is killed
trap "kill $SERVER_PID 2>/dev/null || true" EXIT

# Wait for socket
MAX_RETRIES=10
COUNT=0
while [ ! -S /tmp/zero-kv.sock ]; do
    if [ $COUNT -eq $MAX_RETRIES ]; then
        echo "Error: Server failed to start."
        exit 1
    fi
    sleep 0.2
    COUNT=$((COUNT+1))
done

echo "[5/5] Running Saturation Benchmark..."
./target/release/benchmarker

if ! kill -0 $SERVER_PID 2>/dev/null; then
    echo "WARNING: Server crashed during benchmark!"
    exit 1
fi

echo "Cleaning up..."
rm bench_input.csv
echo "=== Cycle Complete ==="