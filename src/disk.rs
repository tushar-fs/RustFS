use crate::fs_structs::{Superblock, BLOCK_SIZE, MAGIC_NUMBER};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub struct Disk {
    file: File,
}

impl Disk {
    pub fn new<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        Ok(Disk { file })
    }

    pub fn create_and_format<P: AsRef<Path>>(path: P, total_blocks: u64) -> std::io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        // Zero out the disk
        let zero_block = [0u8; BLOCK_SIZE];
        for _ in 0..total_blocks {
            file.write_all(&zero_block)?;
        }

        let mut disk = Disk { file };
        
        // Define layout
        let superblock = Superblock {
            magic: MAGIC_NUMBER,
            total_blocks,
            journal_start: 1,
            journal_blocks: 100,
            bitmap_start: 101,
            bitmap_blocks: 2,
            inode_table_start: 103,
            inode_table_blocks: 98,
            data_start: 201,
            root_inode: 1, // root inode is inode 1
        };

        // Write superblock
        let encoded = bincode::serialize(&superblock).unwrap();
        let mut block = [0u8; BLOCK_SIZE];
        block[..encoded.len()].copy_from_slice(&encoded);
        disk.write_block(0, &block)?;

        Ok(disk)
    }

    pub fn read_block(&mut self, block_num: u64) -> std::io::Result<[u8; BLOCK_SIZE]> {
        let mut block = [0u8; BLOCK_SIZE];
        self.file.seek(SeekFrom::Start(block_num * BLOCK_SIZE as u64))?;
        self.file.read_exact(&mut block)?;
        Ok(block)
    }

    pub fn write_block(&mut self, block_num: u64, block: &[u8; BLOCK_SIZE]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(block_num * BLOCK_SIZE as u64))?;
        self.file.write_all(block)?;
        Ok(())
    }

    pub fn sync(&mut self) -> std::io::Result<()> {
        self.file.sync_all()
    }
}
