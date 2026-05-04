use crate::disk::Disk;
use crate::fs_structs::{Superblock, BLOCK_SIZE};
use log::{info, warn};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum JournalOp {
    TxnStart(u64),
    AllocateInode(u64),
    UpdateParentDir(u64),
    WriteBlock(u64, Vec<u8>),
    TxnCommit(u64),
}

pub struct Journal {
    pub journal_start: u64,
    pub journal_blocks: u64,
    pub next_txn_id: u64,
    // For simplicity, we just append to the journal.
    // In a real system, it's a ring buffer.
    pub current_block_offset: u64, 
    pub pending_ops: Vec<JournalOp>,
}

impl Journal {
    pub fn new(superblock: &Superblock) -> Self {
        Journal {
            journal_start: superblock.journal_start,
            journal_blocks: superblock.journal_blocks,
            next_txn_id: 1,
            current_block_offset: 0,
            pending_ops: Vec::new(),
        }
    }

    pub fn start_txn(&mut self) -> u64 {
        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;
        self.pending_ops.push(JournalOp::TxnStart(txn_id));
        info!("Journal: Txn {} Start", txn_id);
        txn_id
    }

    pub fn log_alloc_inode(&mut self, ino: u64) {
        self.pending_ops.push(JournalOp::AllocateInode(ino));
        info!("Journal: Allocate Inode {}", ino);
    }

    pub fn log_update_parent_dir(&mut self, parent_ino: u64) {
        self.pending_ops.push(JournalOp::UpdateParentDir(parent_ino));
        info!("Journal: Update Parent Dir Inode {}", parent_ino);
    }

    pub fn log_write_block(&mut self, block_num: u64, data: &[u8; BLOCK_SIZE]) {
        self.pending_ops.push(JournalOp::WriteBlock(block_num, data.to_vec()));
        // Note: we don't spam info for data blocks, only debug if needed
    }

    pub fn commit_txn(&mut self, disk: &mut Disk, txn_id: u64) -> std::io::Result<()> {
        self.pending_ops.push(JournalOp::TxnCommit(txn_id));
        info!("Journal: Txn {} Commit", txn_id);
        
        // Serialize operations and write to disk journal area.
        // For MVP, we serialize the entire pending_ops list into journal blocks.
        // A real system writes them one by one.
        let encoded = bincode::serialize(&self.pending_ops).unwrap();
        
        // Write to journal blocks
        let mut offset = 0;
        let mut j_block = self.current_block_offset;
        
        while offset < encoded.len() {
            let mut block = [0u8; BLOCK_SIZE];
            let chunk_size = std::cmp::min(BLOCK_SIZE, encoded.len() - offset);
            block[..chunk_size].copy_from_slice(&encoded[offset..offset + chunk_size]);
            
            // Ensure we don't exceed journal size in MVP
            if j_block >= self.journal_blocks {
                panic!("Journal Overflow!");
            }
            
            disk.write_block(self.journal_start + j_block, &block)?;
            offset += chunk_size;
            j_block += 1;
        }
        
        // Sync to disk to ensure WAL is persisted before applying
        disk.sync()?;
        info!("Journal: Synced to disk");
        
        // Advance current block offset for future txns (simplified ring buffer)
        self.current_block_offset = j_block;

        Ok(())
    }

    pub fn checkpoint(&mut self, disk: &mut Disk) -> std::io::Result<()> {
        info!("Journal: Checkpointing transactions to main disk areas...");
        for op in &self.pending_ops {
            if let JournalOp::WriteBlock(block_num, data) = op {
                let mut block = [0u8; BLOCK_SIZE];
                block.copy_from_slice(data);
                disk.write_block(*block_num, &block)?;
            }
        }
        disk.sync()?;
        info!("Journal: Checkpoint complete");
        
        // Clear journal
        self.pending_ops.clear();
        self.current_block_offset = 0;
        
        Ok(())
    }

    pub fn recover(disk: &mut Disk, superblock: &Superblock) -> std::io::Result<Self> {
        info!("Journal: Checking for un-checkpointed transactions...");
        let mut journal = Journal::new(superblock);
        
        // Read journal blocks and try to deserialize
        let mut encoded = Vec::new();
        for i in 0..superblock.journal_blocks {
            let block = disk.read_block(superblock.journal_start + i)?;
            encoded.extend_from_slice(&block);
        }
        
        // If it deserializes, there are pending operations
        if let Ok(ops) = bincode::deserialize::<Vec<JournalOp>>(&encoded) {
            if !ops.is_empty() {
                warn!("Journal: Found pending operations during mount. Replaying WAL!");
                journal.pending_ops = ops;
                // Replay the physical writes
                for op in &journal.pending_ops {
                    match op {
                        JournalOp::TxnStart(id) => info!("Replay: Txn {} Start", id),
                        JournalOp::AllocateInode(ino) => info!("Replay: Allocate Inode {}", ino),
                        JournalOp::UpdateParentDir(ino) => info!("Replay: Update Parent Dir {}", ino),
                        JournalOp::WriteBlock(b_num, data) => {
                            let mut block = [0u8; BLOCK_SIZE];
                            block.copy_from_slice(data);
                            disk.write_block(*b_num, &block)?;
                        },
                        JournalOp::TxnCommit(id) => info!("Replay: Txn {} Commit", id),
                    }
                }
                disk.sync()?;
                info!("Journal: Recovery successful.");
                // Clear after recovery
                journal.pending_ops.clear();
            }
        } else {
            info!("Journal: Clean state, no recovery needed.");
        }
        
        Ok(journal)
    }
}
