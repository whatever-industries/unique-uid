use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::Builder;

use crate::avid::{Document, Error};

pub struct SavedCopy {
    pub path: PathBuf,
    pub uid: u64,
}

#[derive(Default)]
pub struct CopyBatch {
    pub copies: Vec<SavedCopy>,
    pub error: Option<Error>,
}

pub fn create_copies(
    source: &Path,
    destination: &Path,
    count: usize,
    mut excluded: HashSet<u64>,
) -> Result<CopyBatch, Error> {
    if count == 0 {
        return Err(Error::InvalidCount);
    }

    let (document, mut data) = Document::snapshot(source)?;
    let stem = source.file_stem().ok_or(Error::UnsupportedFile)?;
    let extension = source
        .extension()
        .ok_or(Error::UnsupportedFile)?
        .to_string_lossy();
    excluded.extend([0, document.uid]);
    let mut batch = CopyBatch::default();
    let mut index: u64 = 1;

    for _ in 0..count {
        let result = (|| {
            let uid = unique_uid(&mut excluded)?;
            document.replace_uid(&mut data, uid);
            let mut file = Builder::new()
                .prefix(".unique-uid-")
                .tempfile_in(destination)
                .map_err(|error| Error::file("prepare a copy in", destination, error))?;
            file.write_all(&data)
                .map_err(|error| Error::file("write a copy in", destination, error))?;

            loop {
                let suffix = if index == 1 {
                    format!(" Copy.{extension}")
                } else {
                    format!(" Copy {index}.{extension}")
                };
                index += 1;
                let path = destination.join(copy_name(stem, &suffix));
                match file.persist_noclobber(&path) {
                    Ok(_) => return Ok(SavedCopy { path, uid }),
                    Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                        file = error.file;
                    }
                    Err(error) => return Err(Error::file("save", &path, error.error)),
                }
            }
        })();

        match result {
            Ok(copy) => batch.copies.push(copy),
            Err(error) => {
                batch.error = Some(error);
                break;
            }
        }
    }
    Ok(batch)
}

fn unique_uid(excluded: &mut HashSet<u64>) -> Result<u64, Error> {
    loop {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes)?;
        let uid = u64::from_ne_bytes(bytes);
        if excluded.insert(uid) {
            return Ok(uid);
        }
    }
}

fn copy_name(stem: &OsStr, suffix: &str) -> OsString {
    let limit = 255 - suffix.len();
    let mut name = if stem.as_encoded_bytes().len() > limit {
        let stem = stem.to_string_lossy();
        let mut end = limit.min(stem.len());
        while !stem.is_char_boundary(end) {
            end -= 1;
        }
        OsString::from(&stem[..end])
    } else {
        stem.to_owned()
    };
    name.push(suffix);
    name
}
