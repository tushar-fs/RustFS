#!/bin/bash
set -e

# Compile the project
echo "Building RustFS..."
cargo build

# Setup directories and disk
DISK_IMG="disk.img"
MOUNT_POINT="/tmp/rustfs"

# Clean up any previous runs
rm -f $DISK_IMG
mkdir -p $MOUNT_POINT

# Mount the filesystem in the background
echo "Mounting RustFS to $MOUNT_POINT..."
./target/debug/rustfs $DISK_IMG $MOUNT_POINT &
FS_PID=$!

# Wait a second for it to mount
sleep 2

echo "Running standard operations..."

# Create a directory
mkdir $MOUNT_POINT/test_dir
echo "Created directory test_dir"

# Create and write to a file
echo "Hello NetApp!" > $MOUNT_POINT/test_dir/hello.txt
echo "Wrote to test_dir/hello.txt"

# Read from the file
cat $MOUNT_POINT/test_dir/hello.txt

# Remove the file
rm $MOUNT_POINT/test_dir/hello.txt
echo "Removed hello.txt"

# Unmount
echo "Unmounting RustFS..."
umount $MOUNT_POINT || diskutil unmount force $MOUNT_POINT

# Kill process if it didn't exit
kill $FS_PID 2>/dev/null || true

echo "Test complete!"
