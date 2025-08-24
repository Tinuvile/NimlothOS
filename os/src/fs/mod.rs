use crate::mm::UserBuffer;

mod inode;
mod stdio;

pub use inode::{OSInode, OpenFlags, list_apps, open_file};
pub use stdio::{Stdin, Stdout};

pub trait File: Send + Sync {
    fn read(&self, buf: UserBuffer) -> usize;
    fn write(&self, buf: UserBuffer) -> usize;
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
}
