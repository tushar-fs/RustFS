use crate::disk::Disk;
use crate::fs_structs::{DirEntry, FileType, Inode, BLOCK_SIZE};
use crate::journal::Journal;
use crate::manager::Manager;
use fuser::{
    FileAttr, FileType as FuseFileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, Request,
};
use libc::ENOENT;
use std::time::{Duration, UNIX_EPOCH};

pub struct RustFS {
    pub disk: Disk,
    pub manager: Manager,
    pub journal: Journal,
}

impl RustFS {
    fn inode_to_attr(&self, inode: &Inode) -> FileAttr {
        FileAttr {
            ino: inode.ino,
            size: inode.size,
            blocks: (inode.size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: match inode.file_type {
                FileType::File => FuseFileType::RegularFile,
                FileType::Directory => FuseFileType::Directory,
            },
            perm: 0o777,
            nlink: inode.link_count,
            uid: 501,
            gid: 20,
            rdev: 0,
            blksize: BLOCK_SIZE as u32,
            flags: 0,
        }
    }

    fn read_dir_entries(&mut self, inode: &Inode) -> Vec<DirEntry> {
        let mut entries = Vec::new();
        for &block_num in &inode.direct_blocks {
            if block_num == 0 {
                continue;
            }
            if let Ok(block) = self.disk.read_block(block_num) {
                // Deserialize multiple DirEntry from the block
                // For MVP, we'll try to deserialize until error
                let mut offset = 0;
                while offset < BLOCK_SIZE {
                    // Quick length check (first 8 bytes for length in bincode)
                    // It's safer to just deserialize slice and catch err.
                    // Wait, bincode doesn't know where one entry ends unless framed.
                    // Let's frame it: 8-byte length, then data.
                    // Or since we only have a few files in MVP, we just deserialize a Vec<DirEntry>.
                    if let Ok(vec) = bincode::deserialize::<Vec<DirEntry>>(&block) {
                        entries.extend(vec);
                        break;
                    } else {
                        break;
                    }
                }
            }
        }
        entries
    }

    fn write_dir_entries(&mut self, inode: &mut Inode, entries: &Vec<DirEntry>) {
        let encoded = bincode::serialize(entries).unwrap();
        
        let block_num = if inode.direct_blocks[0] == 0 {
            // allocate new block
            let new_block = self.manager.alloc_block(&mut self.journal).unwrap();
            inode.direct_blocks[0] = new_block;
            new_block
        } else {
            inode.direct_blocks[0]
        };

        let mut block = [0u8; BLOCK_SIZE];
        let chunk_size = std::cmp::min(BLOCK_SIZE, encoded.len());
        block[..chunk_size].copy_from_slice(&encoded[..chunk_size]);
        
        self.journal.log_write_block(block_num, &block);
        // also update inode size
        inode.size = chunk_size as u64;
    }
}

const TTL: Duration = Duration::from_secs(1);

impl Filesystem for RustFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &std::ffi::OsStr, reply: ReplyEntry) {
        let name_str = name.to_str().unwrap().to_string();
        
        if let Ok(Some(parent_inode)) = self.manager.get_inode(&mut self.disk, parent) {
            let entries = self.read_dir_entries(&parent_inode);
            for entry in entries {
                if entry.name == name_str {
                    if let Ok(Some(inode)) = self.manager.get_inode(&mut self.disk, entry.ino) {
                        reply.entry(&TTL, &self.inode_to_attr(&inode), 0);
                        return;
                    }
                }
            }
        }
        reply.error(ENOENT);
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        if let Ok(Some(inode)) = self.manager.get_inode(&mut self.disk, ino) {
            reply.attr(&TTL, &self.inode_to_attr(&inode));
        } else {
            reply.error(ENOENT);
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if let Ok(Some(inode)) = self.manager.get_inode(&mut self.disk, ino) {
            let entries = self.read_dir_entries(&inode);
            
            if offset == 0 {
                // root or self
                reply.add(ino, 1, FuseFileType::Directory, ".");
                // parent (just self for simplicity in MVP)
                reply.add(ino, 2, FuseFileType::Directory, "..");
            }
            
            for (i, entry) in entries.iter().enumerate().skip(offset as usize) {
                // i + 3 because . and .. take 1 and 2
                let kind = FuseFileType::RegularFile; // Simplicity: we'd need to lookup ino to get actual type
                if reply.add(entry.ino, (i + 3) as i64, kind, &entry.name) {
                    break;
                }
            }
            reply.ok();
        } else {
            reply.error(ENOENT);
        }
    }

    fn mkdir(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &std::ffi::OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let name_str = name.to_str().unwrap().to_string();
        
        let txn_id = self.journal.start_txn();
        
        if let Ok(Some(mut parent_inode)) = self.manager.get_inode(&mut self.disk, parent) {
            self.journal.log_update_parent_dir(parent);
            
            // Allocate new inode
            if let Ok(Some(new_inode)) = self.manager.alloc_inode(&mut self.disk, &mut self.journal, FileType::Directory) {
                let mut entries = self.read_dir_entries(&parent_inode);
                entries.push(DirEntry {
                    ino: new_inode.ino,
                    name: name_str,
                });
                
                self.write_dir_entries(&mut parent_inode, &entries);
                self.manager.write_inode(&mut self.disk, &mut self.journal, &parent_inode).unwrap();
                
                self.journal.commit_txn(&mut self.disk, txn_id).unwrap();
                self.journal.checkpoint(&mut self.disk).unwrap();
                
                reply.entry(&TTL, &self.inode_to_attr(&new_inode), 0);
                return;
            }
        }
        reply.error(ENOENT);
    }

    fn mknod(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &std::ffi::OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        let name_str = name.to_str().unwrap().to_string();
        let txn_id = self.journal.start_txn();
        
        if let Ok(Some(mut parent_inode)) = self.manager.get_inode(&mut self.disk, parent) {
            self.journal.log_update_parent_dir(parent);
            
            if let Ok(Some(new_inode)) = self.manager.alloc_inode(&mut self.disk, &mut self.journal, FileType::File) {
                let mut entries = self.read_dir_entries(&parent_inode);
                entries.push(DirEntry {
                    ino: new_inode.ino,
                    name: name_str,
                });
                
                self.write_dir_entries(&mut parent_inode, &entries);
                self.manager.write_inode(&mut self.disk, &mut self.journal, &parent_inode).unwrap();
                
                self.journal.commit_txn(&mut self.disk, txn_id).unwrap();
                self.journal.checkpoint(&mut self.disk).unwrap();
                
                reply.entry(&TTL, &self.inode_to_attr(&new_inode), 0);
                return;
            }
        }
        reply.error(ENOENT);
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &std::ffi::OsStr, reply: ReplyEmpty) {
        let name_str = name.to_str().unwrap().to_string();
        let txn_id = self.journal.start_txn();
        
        if let Ok(Some(mut parent_inode)) = self.manager.get_inode(&mut self.disk, parent) {
            self.journal.log_update_parent_dir(parent);
            let mut entries = self.read_dir_entries(&parent_inode);
            
            if let Some(pos) = entries.iter().position(|e| e.name == name_str) {
                let target_ino = entries[pos].ino;
                entries.remove(pos);
                
                self.write_dir_entries(&mut parent_inode, &entries);
                self.manager.write_inode(&mut self.disk, &mut self.journal, &parent_inode).unwrap();
                
                // For MVP, we don't strictly free the blocks, just remove the dir entry to demonstrate unlink.
                // In a full implementation, we'd free the target_ino's direct blocks and then free the inode bit.
                
                self.journal.commit_txn(&mut self.disk, txn_id).unwrap();
                self.journal.checkpoint(&mut self.disk).unwrap();
                
                reply.ok();
                return;
            }
        }
        reply.error(ENOENT);
    }

    // `rmdir` can be mapped to unlink for simplicity in the MVP.
    fn rmdir(&mut self, req: &Request, parent: u64, name: &std::ffi::OsStr, reply: ReplyEmpty) {
        self.unlink(req, parent, name, reply);
    }

    fn write(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyWrite,
    ) {
        let txn_id = self.journal.start_txn();
        
        if let Ok(Some(mut inode)) = self.manager.get_inode(&mut self.disk, ino) {
            // For MVP, we only write to the first direct block (simplifies logic)
            // It allows up to 4KB writes.
            let block_num = if inode.direct_blocks[0] == 0 {
                let nb = self.manager.alloc_block(&mut self.journal).unwrap();
                inode.direct_blocks[0] = nb;
                nb
            } else {
                inode.direct_blocks[0]
            };
            
            let mut block = self.disk.read_block(block_num).unwrap_or([0u8; BLOCK_SIZE]);
            
            let write_len = std::cmp::min(data.len(), BLOCK_SIZE - offset as usize);
            let start = offset as usize;
            let end = start + write_len;
            
            block[start..end].copy_from_slice(&data[..write_len]);
            
            self.journal.log_write_block(block_num, &block);
            
            if offset as u64 + write_len as u64 > inode.size {
                inode.size = offset as u64 + write_len as u64;
            }
            self.manager.write_inode(&mut self.disk, &mut self.journal, &inode).unwrap();
            
            // Check if we want to simulate a crash here!
            if std::env::var("CRASH_TEST").is_ok() {
                // If CRASH_TEST is set, we crash BEFORE checkpointing but AFTER writing journal
                // Wait, we need to commit txn first
                self.journal.commit_txn(&mut self.disk, txn_id).unwrap();
                log::warn!("CRASH_TEST is set! Simulating a mid-write crash NOW!");
                std::process::exit(1);
            } else {
                self.journal.commit_txn(&mut self.disk, txn_id).unwrap();
                self.journal.checkpoint(&mut self.disk).unwrap();
            }

            reply.written(write_len as u32);
        } else {
            reply.error(ENOENT);
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        if let Ok(Some(inode)) = self.manager.get_inode(&mut self.disk, ino) {
            let block_num = inode.direct_blocks[0];
            if block_num == 0 {
                reply.data(&[]);
                return;
            }
            
            if let Ok(block) = self.disk.read_block(block_num) {
                let start = offset as usize;
                let end = std::cmp::min(start + size as usize, inode.size as usize);
                if start < end {
                    reply.data(&block[start..end]);
                } else {
                    reply.data(&[]);
                }
            } else {
                reply.error(ENOENT);
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn setattr(
        &mut self,
        _req: &Request,
        ino: u64,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<fuser::TimeOrNow>,
        _mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        let txn_id = self.journal.start_txn();
        if let Ok(Some(mut inode)) = self.manager.get_inode(&mut self.disk, ino) {
            if let Some(s) = size {
                inode.size = s;
            }
            self.manager.write_inode(&mut self.disk, &mut self.journal, &inode).unwrap();
            
            self.journal.commit_txn(&mut self.disk, txn_id).unwrap();
            self.journal.checkpoint(&mut self.disk).unwrap();
            
            reply.attr(&TTL, &self.inode_to_attr(&inode));
        } else {
            reply.error(ENOENT);
        }
    }
}
