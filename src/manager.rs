use crate::disk::Disk;
use crate::fs_structs::{Inode, Superblock, BLOCK_SIZE, FileType};
use crate::journal::Journal;
use std::collections::HashMap;

pub struct Manager {
    pub superblock: Superblock,
    inode_cache: HashMap<u64, Inode>,
    
    // In-memory representations of the bitmaps
    block_bitmap: [u8; BLOCK_SIZE],
    inode_bitmap: [u8; BLOCK_SIZE],
}

impl Manager {
    pub fn new(disk: &mut Disk, superblock: Superblock) -> std::io::Result<Self> {
        let block_bitmap = disk.read_block(superblock.bitmap_start)?;
        let inode_bitmap = disk.read_block(superblock.bitmap_start + 1)?;
        
        Ok(Manager {
            superblock,
            inode_cache: HashMap::new(),
            block_bitmap,
            inode_bitmap,
        })
    }

    // Bitmap utilities
    fn set_bit(bitmap: &mut [u8; BLOCK_SIZE], index: usize) {
        let byte_idx = index / 8;
        let bit_idx = index % 8;
        bitmap[byte_idx] |= 1 << bit_idx;
    }

    fn clear_bit(bitmap: &mut [u8; BLOCK_SIZE], index: usize) {
        let byte_idx = index / 8;
        let bit_idx = index % 8;
        bitmap[byte_idx] &= !(1 << bit_idx);
    }

    fn find_free_bit(bitmap: &[u8; BLOCK_SIZE]) -> Option<usize> {
        for (byte_idx, &byte) in bitmap.iter().enumerate() {
            if byte != 0xFF {
                for bit_idx in 0..8 {
                    if (byte & (1 << bit_idx)) == 0 {
                        return Some(byte_idx * 8 + bit_idx);
                    }
                }
            }
        }
        None
    }

    pub fn alloc_block(&mut self, journal: &mut Journal) -> Option<u64> {
        if let Some(bit_idx) = Self::find_free_bit(&self.block_bitmap) {
            let block_num = bit_idx as u64;
            
            // Limit allocation up to the end of data blocks
            if block_num >= self.superblock.total_blocks {
                return None;
            }

            // Mark as used
            Self::set_bit(&mut self.block_bitmap, bit_idx);
            
            // Log the bitmap update via WAL
            journal.log_write_block(self.superblock.bitmap_start, &self.block_bitmap);
            
            Some(block_num)
        } else {
            None
        }
    }

    pub fn free_block(&mut self, journal: &mut Journal, block_num: u64) {
        Self::clear_bit(&mut self.block_bitmap, block_num as usize);
        journal.log_write_block(self.superblock.bitmap_start, &self.block_bitmap);
    }

    pub fn alloc_inode(&mut self, disk: &mut Disk, journal: &mut Journal, file_type: FileType) -> std::io::Result<Option<Inode>> {
        // Find a free inode index (skip inode 0)
        // Ensure we don't start from 0 if it's already used or reserved
        for idx in 1..self.superblock.inode_table_blocks * (BLOCK_SIZE as u64 / 128) { // approximate max inodes
            let bit_idx = idx as usize;
            if (self.inode_bitmap[bit_idx / 8] & (1 << (bit_idx % 8))) == 0 {
                // Free inode found
                Self::set_bit(&mut self.inode_bitmap, bit_idx);
                journal.log_write_block(self.superblock.bitmap_start + 1, &self.inode_bitmap);
                
                let ino = idx;
                journal.log_alloc_inode(ino);
                
                let inode = Inode::new(ino, file_type);
                self.write_inode(disk, journal, &inode)?;
                self.inode_cache.insert(ino, inode.clone());
                return Ok(Some(inode));
            }
        }
        Ok(None)
    }

    pub fn get_inode(&mut self, disk: &mut Disk, ino: u64) -> std::io::Result<Option<Inode>> {
        if let Some(inode) = self.inode_cache.get(&ino) {
            return Ok(Some(inode.clone()));
        }

        // Calculate location
        // Assume inode serialization size is fixed, for MVP let's say 256 bytes per Inode.
        // Actually, bincode serialization varies slightly, but we can serialize Inodes
        // into exactly 256 byte chunks. Or, simpler: store 1 Inode per block to avoid complexity!
        // No, let's serialize/deserialize the whole block to a vector of inodes.
        // Actually, for MVP: Let's read the block, calculate offset.
        let inodes_per_block = BLOCK_SIZE / 256;
        let block_offset = ino / inodes_per_block as u64;
        let inode_idx = ino % inodes_per_block as u64;
        
        let block_num = self.superblock.inode_table_start + block_offset;
        let block = disk.read_block(block_num)?;
        
        let start = (inode_idx * 256) as usize;
        let end = start + 256;
        
        let inode: Result<Inode, _> = bincode::deserialize(&block[start..end]);
        if let Ok(i) = inode {
            if i.ino == ino {
                self.inode_cache.insert(ino, i.clone());
                return Ok(Some(i));
            }
        }
        
        // If not found or deserialization fails
        Ok(None)
    }

    pub fn write_inode(&mut self, disk: &mut Disk, journal: &mut Journal, inode: &Inode) -> std::io::Result<()> {
        // Cache it
        self.inode_cache.insert(inode.ino, inode.clone());

        let inodes_per_block = BLOCK_SIZE / 256;
        let block_offset = inode.ino / inodes_per_block as u64;
        let inode_idx = inode.ino % inodes_per_block as u64;
        
        let block_num = self.superblock.inode_table_start + block_offset;
        
        // Read existing block
        let mut block = disk.read_block(block_num)?;
        
        // Serialize inode
        let encoded = bincode::serialize(inode).unwrap();
        
        // Write it into the block at the correct offset
        let start = (inode_idx * 256) as usize;
        let end = start + encoded.len();
        block[start..end].copy_from_slice(&encoded);
        
        // Log the block write
        journal.log_write_block(block_num, &block);

        Ok(())
    }
}
