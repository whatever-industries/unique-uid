mod avid;
mod copies;

pub use avid::{Error, read_uid};
pub use copies::{CopyBatch, SavedCopy, create_copies};

#[cfg(test)]
mod tests;
