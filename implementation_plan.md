# RustFS Implementation Plan

This document outlines the step-by-step plan for building the "RustFS" Minimum Viable Product (MVP) - a user-space file system in Rust that demonstrates mid-write crash recovery using a Write-Ahead Log (WAL).

## User Review Required

Please review the proposed disk layout, FUSE operation scope, and the crash simulation strategy.

## Open Questions

1. **Dependency Limits:** Is it okay to use `bincode` and `serde` for easy serialization/deserialization of in-memory structures to disk blocks, or do you prefer manual byte-shifting? (I plan to use `bincode` and `serde` as suggested in the prompt).
2. **Disk Size:** For the MVP, I plan to fix the `disk.img` size to 10MB to keep things simple and easy to manage/test. Does this sound appropriate?

## Proposed Changes

### Project Setup and Dependencies

- Initialize a new Rust library/binary project in the workspace (`/Users/tusharsingh/Desktop/RustFS`).
- Add dependencies to `Cargo.toml`: `fuser`, `libc`, `log`, `env_logger`, `bincode`, `serde`, `serde_derive`.

### 1. Disk Layout & Core Structures

#### [NEW] `src/disk.rs`
- Defines constants: `BLOCK_SIZE` (4KB).
- Defines the layout locations (in blocks):
  - Block 0: Superblock (Magic number, layout offsets).
  - Blocks 1-100: Journal Area.
  - Blocks 101-102: Block/Inode Bitmaps.
  - Blocks 103-200: Inode Table (Array of fixed-size Inode structures).
  - Blocks 201+: Data Blocks.
- Logic to read/write 4KB blocks directly from the `disk.img` file.
- `init_disk()` function to create a zeroed-out disk and set up the superblock and root directory Inode.

#### [NEW] `src/fs_structs.rs`
- `Superblock`: Tracks disk layout.
- `Inode`: File metadata (type, size, direct block pointers).
- `DirEntry`: Used inside directory data blocks (maps filename string to Inode number).

### 2. Block and Metadata Managers

#### [NEW] `src/manager.rs`
- **Bitmap Manager:** Logic to scan the bitmap blocks to find and allocate free data blocks and Inodes.
- **Inode Cache:** An in-memory cache (using `HashMap`) to store active Inodes. Reads from disk if not in cache, writes to disk (via WAL) when modified.
- **Data Block Manager:** Logic to read/write specific data blocks for a given Inode.

### 3. Write-Ahead Log (WAL)

#### [NEW] `src/journal.rs`
- Defines the `Txn` (Transaction) and `Operation` enums (e.g., `AllocBlock(u64)`, `UpdateInode(u64, Inode)`, `WriteData(u64, [u8; 4096])`).
- **Logging Flow:** Methods to append operations to the journal, write a `Commit` marker, and sync to disk.
- **Checkpointing:** Applying committed operations from the journal to the main disk areas (Bitmap, Inodes, Data) and then clearing the journal.
- **Recovery:** On initialization, scans the journal area. If a committed transaction is found that hasn't been checkpointed, it replays those operations to the main disk structures.

### 4. FUSE Integration

#### [NEW] `src/rustfs.rs`
- Implements the `fuser::Filesystem` trait.
- Wires FUSE operations to internal managers:
  - `lookup`, `getattr`: Read from Inode cache.
  - `readdir`: Read data blocks of a directory Inode and parse `DirEntry` records.
  - `mkdir`: Start Txn, allocate Inode, allocate directory block, update parent directory entries, commit Txn, checkpoint.
  - `unlink`: Start Txn, remove parent `DirEntry`, mark Inode and blocks free, commit Txn, checkpoint.
  - `open`, `read`, `write`: Resolve path, read/modify data blocks (writes are also journaled for crash consistency).

#### [NEW] `src/main.rs`
- CLI entry point. Parses mount path and `disk.img` path.
- Handles initialization (recovery or creating new disk).
- Mounts the FUSE filesystem using `fuser::mount2`.

### 5. Testing Harness & Crash Simulation

#### [NEW] `test_basic.sh`
- Compiles the RustFS project (`cargo build`).
- Creates a `disk.img`, mounts RustFS to `/tmp/rustfs`.
- Performs `mkdir`, `touch`, `echo`, `cat`, `ls`, and `rm` to verify POSIX compliance.
- Unmounts safely.

#### [NEW] `simulate_crash.sh`
- A script to demonstrate crash recovery.
- Mounts the filesystem in the background.
- Touches a trigger file or sets an environment variable that causes the `rustfs` process to exit (`std::process::exit(1)`) *after* the Txn commit is written to the journal but *before* checkpointing.
- Remounts the filesystem to trigger the recovery replay.
- Verifies that the file and data are intact and correctly present in the main disk structures after recovery.

## Verification Plan

### Automated Tests
- The `test_basic.sh` will serve as the primary automated integration test.

### Manual Verification
- The crash simulation script will be run manually to observe the WAL replaying journal logs on startup.
- Reviewing the raw hex dump of `disk.img` before and after recovery to prove that the WAL successfully updated the main data structures post-crash.
