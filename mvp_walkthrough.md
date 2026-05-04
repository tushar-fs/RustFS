# RustFS MVP Walkthrough

The "RustFS" MVP has been successfully implemented! This is a user-space file system in Rust utilizing the `fuser` crate. It effectively demonstrates block management, an in-memory metadata cache, and, most importantly, mid-write crash recovery using a **Write-Ahead Log (WAL)**.

## Architecture & Implementation Details

The project is structured into multiple decoupled modules:

### 1. Disk Layout (`disk.rs` & `fs_structs.rs`)
- The storage backend is a single local file (`disk.img`), logically divided into 4KB blocks.
- **Superblock (Block 0):** Stores metadata about the layout.
- **Journal Area (Blocks 1-100):** A dedicated area where operations are logged before they are applied.
- **Bitmap Area (Blocks 101-102):** Tracks free/used blocks and inodes.
- **Inode Table (Blocks 103-200):** Fixed-size array mapping `ino` to file metadata and direct block pointers.
- **Data Blocks (Blocks 201+):** Actual file content and directory structures.

### 2. The Write-Ahead Log (`journal.rs`)
This is the core of the crash-consistency guarantee.
When a FUSE operation (like `mkdir` or `write`) occurs, the `RustFS` system starts a transaction. Instead of writing directly to the data blocks, modifications are appended to `pending_ops` in the `Journal`:
1. `TxnStart(id)` is logged.
2. Metadata intents like `AllocateInode(ino)` and physical block writes `WriteBlock(block_num, data)` are logged.
3. `TxnCommit(id)` is written, and the journal blocks are synchronously flushed to `disk.img` (`disk.sync()`).
4. **Checkpointing:** Only *after* the WAL commit is safe on disk do we write the actual data blocks to their real locations.

### 3. Crash Recovery (Simulation)
I've included a script `simulate_crash.sh` that demonstrates the recovery. It works by setting a `CRASH_TEST` environment variable.
Inside `rustfs.rs`, if this variable is detected during a file `write`, the filesystem purposefully triggers a `std::process::exit(1)` **after** the `TxnCommit` is written to the journal, but **before** the data is checkpointed to the main disk.

When the system restarts, `Journal::recover()` scans the first 100 blocks. It finds the un-checkpointed transaction and physically replays the `WriteBlock` operations, rescuing the data that would otherwise be lost!

---

## Discussing Extensions at NetApp

When you present this to the NetApp interviewers, you can leverage this MVP to talk about more advanced storage paradigms:

### 1. Transitioning to Copy-on-Write (WAFL)
> [!IMPORTANT]
> **NetApp WAFL** is arguably their most famous innovation. 
**The Limitation here:** A traditional WAL requires writing the data twice (once to the log, once to the destination block), incurring a 2x write penalty.
**The Extension:** You can discuss how you would evolve RustFS from a journaled system to a Copy-on-Write (CoW) architecture. Instead of overwriting old blocks, new data is written to free blocks. Once written, the Inode pointers are updated in a single atomic tree update (the "consistency point"). This eliminates the need for a physical data journal entirely.

### 2. Snapshots
**The Limitation here:** If a user runs `rm` accidentally, journaling doesn't help—it just ensures the deletion is crash-consistent.
**The Extension:** If you successfully argue the transition to CoW, you can explain that Snapshots become trivial. By simply taking a read-only lock on the root Inode at a specific point in time, and preventing the block allocator from reusing those specific blocks, you get instantaneous snapshots (a staple NetApp feature).

### 3. Advanced Caching Strategies
> [!TIP]
> **Performance Edge:** The MVP uses a basic `HashMap` for the Inode Cache.
**The Extension:** Discuss replacing this with a unified buffer cache (for both Inodes and Data Blocks) driven by the **ARC (Adaptive Replacement Cache)** algorithm, which IBM and ZFS popularized. This proves you understand cache hit ratios beyond simple LRU.

### 4. Performance Benchmarking with `fio`
**The Limitation here:** FUSE has significant overhead due to context switching between user/kernel space.
**The Extension:** Explain that to test RustFS, you would run `fio` (Flexible I/O Tester) scripts simulating random vs. sequential writes, and compare the throughput against native `ext4`. Demonstrating that you can scientifically identify system bottlenecks will show you think like a true systems engineer.
