use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::Builder;

use crate::avid::{Document, Error, Kind};
use crate::independent::IndependentBin;

pub struct SavedCopy {
    pub path: PathBuf,
    pub uid: u64,
}

#[derive(Default)]
pub struct CopyBatch {
    pub copies: Vec<SavedCopy>,
    pub error: Option<Error>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CopyMode {
    DocumentOnly,
    Independent,
}

enum Snapshot {
    Document(Vec<u8>),
    Independent {
        bin: Box<IndependentBin>,
        first: Option<Vec<u8>>,
    },
}

pub struct PreparedCopies {
    document: Document,
    snapshot: Snapshot,
    stem: OsString,
    extension: String,
    destination: PathBuf,
    count: usize,
    excluded: HashSet<u64>,
}

impl PreparedCopies {
    pub fn has_timeline(&self) -> bool {
        matches!(&self.snapshot, Snapshot::Independent { bin, .. } if bin.has_timeline)
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn save(mut self, timeline_confirmed: bool) -> Result<CopyBatch, Error> {
        if self.has_timeline() && !timeline_confirmed {
            return Err(Error::TimelineConfirmation);
        }
        let mut batch = CopyBatch::default();
        let mut index: u64 = 1;
        for _ in 0..self.count {
            let result = (|| {
                let uid = unique_uid(&mut self.excluded)?;
                let independent = matches!(self.snapshot, Snapshot::Independent { .. });
                let mut independent_bytes;
                let data = match &mut self.snapshot {
                    Snapshot::Document(data) => data,
                    Snapshot::Independent { bin, first } => {
                        independent_bytes = match first.take() {
                            Some(bytes) => bytes,
                            None => bin.render()?,
                        };
                        &mut independent_bytes
                    }
                };
                if independent {
                    Document::parse(data, Kind::Bin)?.replace_uid(data, uid);
                } else {
                    self.document.replace_uid(data, uid);
                }
                let mut file = Builder::new()
                    .prefix(".unique-uid-")
                    .tempfile_in(&self.destination)
                    .map_err(|error| Error::file("prepare a copy in", &self.destination, error))?;
                file.write_all(data)
                    .map_err(|error| Error::file("write a copy in", &self.destination, error))?;

                loop {
                    let suffix = if index == 1 {
                        format!(" Copy.{}", self.extension)
                    } else {
                        format!(" Copy {index}.{}", self.extension)
                    };
                    index += 1;
                    let path = self.destination.join(copy_name(&self.stem, &suffix));
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
}

pub fn prepare_copies(
    source: &Path,
    destination: &Path,
    count: usize,
    mut excluded: HashSet<u64>,
    mode: CopyMode,
) -> Result<PreparedCopies, Error> {
    if count == 0 {
        return Err(Error::InvalidCount);
    }

    let kind = Kind::from_path(source)?;
    let (document, data) = Document::snapshot(source)?;
    let snapshot = if kind == Kind::Bin && mode == CopyMode::Independent {
        let mut bin = Box::new(IndependentBin::prepare(&document, data)?);
        let first = Some(bin.render()?);
        Snapshot::Independent { bin, first }
    } else {
        Snapshot::Document(data)
    };
    excluded.extend([0, document.uid]);
    Ok(PreparedCopies {
        document,
        snapshot,
        stem: source.file_stem().ok_or(Error::UnsupportedFile)?.to_owned(),
        extension: source
            .extension()
            .ok_or(Error::UnsupportedFile)?
            .to_string_lossy()
            .into_owned(),
        destination: destination.to_owned(),
        count,
        excluded,
    })
}

pub fn create_copies(
    source: &Path,
    destination: &Path,
    count: usize,
    excluded: HashSet<u64>,
) -> Result<CopyBatch, Error> {
    prepare_copies(source, destination, count, excluded, CopyMode::DocumentOnly)?.save(false)
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
