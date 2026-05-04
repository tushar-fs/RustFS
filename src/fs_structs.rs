use serde::{Deserialize, Serialize};

pub const BLOCK_SIZE: usize = 4096;
pub const MAGIC_NUMBER: u64 = 0x525553544653; // "RUSTFS"

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Superblock {
    pub magic: u64,
    pub total_blocks: u64,
    pub journal_start: u64,
    pub journal_blocks: u64,
    pub bitmap_start: u64,
    pub bitmap_blocks: u64,
    pub inode_table_start: u64,
    pub inode_table_blocks: u64,
    pub data_start: u64,
    pub root_inode: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Inode {
    pub ino: u64,
    pub file_type: FileType,
    pub size: u64,
    pub link_count: u32,
    // Direct block pointers for simplicity in MVP.
    // 12 direct blocks * 4KB = 48KB max file size for MVP.
    pub direct_blocks: [u64; 12],
}

impl Inode {
    pub fn new(ino: u64, file_type: FileType) -> Self {
        Inode {
            ino,
            file_type,
            size: 0,
            link_count: 1, // At least 1 link upon creation
            direct_blocks: [0; 12],
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DirEntry {
    pub ino: u64,
    pub name: String,
}
