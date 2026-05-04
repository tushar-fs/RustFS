# RustFS

RustFS is a Minimum Viable Product (MVP) of a user-space file system written in Rust using the `fuser` crate. The core objective of this project is to demonstrate an understanding of storage systems, block management, metadata caching, and crucially, mid-write crash recovery using a **Write-Ahead Log (WAL)**.

This project was built to showcase systems engineering concepts relevant to high-performance, resilient storage arrays (similar to NetApp's WAFL).

## Architecture & Implementation Details

The project is structured into multiple decoupled modules, simulating a real disk block device over a local file (`disk.img`).

### 1. Disk Layout (`src/disk.rs` & `src/fs_structs.rs`)
The storage backend is logically divided into 4KB blocks.
- **Superblock (Block 0):** Stores metadata about the disk layout, total size, and starting offsets for different regions.
- **Journal Area (Blocks 1-100):** A dedicated cyclic area where transaction operations are logged before they are applied.
- **Bitmap Area (Blocks 101-102):** Tracks free/used blocks and free/used inodes.
- **Inode Table (Blocks 103-200):** A fixed-size array mapping `ino` numbers to file metadata (sizes, types, direct block pointers).
- **Data Blocks (Blocks 201+):** Actual file content and directory entry structures.

### 2. The Write-Ahead Log (WAL) (`src/journal.rs`)
This is the core of the crash-consistency guarantee. When a POSIX operation (like `mkdir` or `write`) occurs, RustFS starts a transaction. Modifications are *not* written directly to their destination data blocks. Instead:
1. `TxnStart(id)` is logged to the in-memory journal.
2. Metadata intents (`AllocateInode`) and physical block writes (`WriteBlock`) are logged.
3. `TxnCommit(id)` is written, and the journal blocks are synchronously flushed to `disk.img` (`disk.sync()`).
4. **Checkpointing:** Only *after* the WAL commit is safe on disk do we write the actual data blocks to their real locations.

### 3. Crash Recovery Simulation
The filesystem includes logic to intentionally panic mid-write to prove the WAL works.
By setting `CRASH_TEST=1`, the filesystem triggers an abrupt exit *after* the `TxnCommit` is written to the journal, but *before* the data is checkpointed to the main disk.

When the filesystem restarts, `Journal::recover()` scans the journal area on mount. It finds the un-checkpointed transaction and physically replays the `WriteBlock` operations, rescuing the data that would otherwise be permanently lost or corrupted!

## Future Scalability & Extensions

While this is an MVP, it lays the groundwork for more advanced storage paradigms:

- **Transitioning to Copy-on-Write (CoW):** A traditional WAL incurs a 2x write penalty. A future iteration of RustFS would evolve into a CoW architecture (similar to NetApp's WAFL). Instead of overwriting old blocks, new data is written to free blocks, and Inode pointers are updated in a single atomic tree update (the "consistency point").
- **Snapshots:** With CoW implemented, instantaneous snapshots become trivial. By taking a read-only lock on the root Inode at a specific point in time, and preventing the block allocator from reusing those specific blocks, the file system can preserve read-only states with zero data duplication.
- **Advanced Caching:** The current MVP uses a basic `HashMap` for the Inode Cache. This could be replaced with a unified buffer cache driven by the **ARC (Adaptive Replacement Cache)** algorithm to improve the cache hit ratio beyond simple LRU.

## Testing Locally

### Prerequisites
- Rust and Cargo (`rustup default stable`)
- FUSE installed on your system (e.g., `libfuse` on Linux, or `macfuse` on macOS).
  > **macOS Note:** You must manually install `macfuse` (e.g., `brew install --cask macfuse`) and explicitly allow the kernel extension in System Settings -> Privacy & Security before the tests will run successfully.

### Running the Tests

1. **Standard POSIX Compliance Test:**
   Compiles the filesystem, formats the `disk.img`, mounts it, and runs a series of standard file/directory creations and deletions.
   ```bash
   ./test_basic.sh
   ```

2. **Crash Recovery Test:**
   Simulates a sudden power loss mid-write and verifies that the Write-Ahead Log successfully recovers the file contents on the next mount.
   ```bash
   ./simulate_crash.sh
   ```
