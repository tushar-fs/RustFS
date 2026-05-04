#!/bin/bash
set -e

echo "Building RustFS..."
cargo build

DISK_IMG="disk.img"
MOUNT_POINT="/tmp/rustfs"

# Start fresh
rm -f $DISK_IMG
mkdir -p $MOUNT_POINT

echo "=== Phase 1: Mount with CRASH_TEST enabled ==="
# Mount the filesystem in the background with CRASH_TEST flag
# This will cause the fs to intentionally crash (exit 1) right after writing a data block 
# to the WAL but BEFORE it checkpoints the data to the main disk structures.
CRASH_TEST=1 ./target/debug/rustfs $DISK_IMG $MOUNT_POINT &
FS_PID=$!

sleep 2

echo "Writing file to trigger the mid-write crash..."
# The RustFS process will intentionally call process::exit(1) inside `write()`
echo "Crash me!" > $MOUNT_POINT/crash_test.txt || true

# Give it a moment to actually crash
sleep 2

# Cleanup mount point because the process died abruptly
diskutil unmount force $MOUNT_POINT 2>/dev/null || umount -f $MOUNT_POINT 2>/dev/null || true

echo "=== Phase 2: Remount and Recover ==="
echo "Remounting RustFS to trigger WAL replay..."
# Mount again, this time WITHOUT the crash flag
./target/debug/rustfs $DISK_IMG $MOUNT_POINT &
FS_PID2=$!

sleep 2

echo "Checking if the WAL successfully recovered the file..."
if cat $MOUNT_POINT/crash_test.txt | grep "Crash me!"; then
    echo "SUCCESS: File content was perfectly recovered from the Write-Ahead Log!"
else
    echo "FAILURE: File content is missing!"
fi

echo "Cleaning up..."
umount $MOUNT_POINT || diskutil unmount force $MOUNT_POINT
kill $FS_PID2 2>/dev/null || true

echo "Crash simulation complete!"
