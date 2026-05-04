mod disk;
mod fs_structs;
mod journal;
mod manager;
mod rustfs;

use disk::Disk;
use fs_structs::{Superblock, MAGIC_NUMBER};
use journal::Journal;
use manager::Manager;
use rustfs::RustFS;
use std::env;
use std::path::Path;

fn main() {
    env_logger::init();
    
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <disk.img> <mount_point>", args[0]);
        std::process::exit(1);
    }
    
    let disk_path = &args[1];
    let mount_point = &args[2];
    
    let total_blocks = 2560; // 10MB MVP disk
    
    let (mut disk, superblock) = if Path::new(disk_path).exists() {
        log::info!("Mounting existing disk: {}", disk_path);
        let mut d = Disk::new(disk_path).expect("Failed to open disk");
        
        // Read superblock
        let sb_block = d.read_block(0).unwrap();
        let sb: Superblock = bincode::deserialize(&sb_block).expect("Failed to parse Superblock");
        if sb.magic != MAGIC_NUMBER {
            panic!("Invalid magic number!");
        }
        (d, sb)
    } else {
        log::info!("Creating new disk: {}", disk_path);
        let d = Disk::create_and_format(disk_path, total_blocks).expect("Failed to format disk");
        
        let sb_block = d.read_block(0).unwrap(); // This needs disk mutably if read_block was mut, but Disk's read_block takes &mut self
        // Wait, Disk::create_and_format returned the disk. 
        // We'll read the superblock in a separate step:
        (d, Superblock {
            magic: MAGIC_NUMBER,
            total_blocks,
            journal_start: 1,
            journal_blocks: 100,
            bitmap_start: 101,
            bitmap_blocks: 2,
            inode_table_start: 103,
            inode_table_blocks: 98,
            data_start: 201,
            root_inode: 1,
        })
    };
    
    // Recovery Phase
    let journal = Journal::recover(&mut disk, &superblock).expect("Failed to initialize/recover Journal");
    
    // Manager Initialization
    let manager = Manager::new(&mut disk, superblock).expect("Failed to initialize Manager");
    
    let fs = RustFS { disk, manager, journal };
    
    log::info!("Mounting RustFS at {}", mount_point);
    let options = vec![fuser::MountOption::FSName("rustfs".to_string())];
    
    fuser::mount2(fs, mount_point, &options).unwrap();
}
