mod avid;
mod copies;
mod independent;

pub use avid::{Error, read_uid};
pub use copies::{CopyBatch, CopyMode, PreparedCopies, SavedCopy, create_copies, prepare_copies};

#[cfg(test)]
mod tests;
