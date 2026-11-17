use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Choose an Avid bin (.avb) or script (.avc).")]
    UnsupportedFile,
    #[error("Choose a regular file.")]
    NotRegularFile,
    #[error("The file does not have a supported Avid document header.")]
    InvalidContainer,
    #[error("The document’s root object does not match its file extension.")]
    UnexpectedRoot,
    #[error("The Avid root object uses an unsupported layout.")]
    UnsupportedRootLayout,
    #[error("The file has an incomplete or invalid Avid object.")]
    Truncated,
    #[error("Independent metadata is unavailable for this bin. {0}")]
    IndependentBin(String),
    #[error("This bin needs confirmation because it contains a timeline.")]
    TimelineConfirmation,
    #[error("Enter a copy count of at least 1.")]
    InvalidCount,
    #[error("Could not {} {}: {error}", .action, .path.display())]
    File {
        action: &'static str,
        path: PathBuf,
        error: io::Error,
    },
    #[error("Could not generate a random UID: {0}")]
    Random(#[from] getrandom::Error),
}

impl Error {
    pub(crate) fn file(action: &'static str, path: &Path, error: io::Error) -> Self {
        Self::File {
            action,
            path: path.to_owned(),
            error,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Bin,
    Script,
}

impl Kind {
    pub(crate) fn from_path(path: &Path) -> Result<Self, Error> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("avb") => Ok(Self::Bin),
            Some(ext) if ext.eq_ignore_ascii_case("avc") => Ok(Self::Script),
            _ => Err(Error::UnsupportedFile),
        }
    }
}

pub(crate) struct Document {
    pub uid: u64,
    pub offset: usize,
    pub count_offset: usize,
    kind: Kind,
    order: ByteOrder,
}

pub fn read_uid(path: &Path) -> Result<u64, Error> {
    Ok(Document::snapshot(path)?.0.uid)
}

impl Document {
    pub(crate) fn snapshot(path: &Path) -> Result<(Self, Vec<u8>), Error> {
        let kind = Kind::from_path(path)?;
        if !fs::metadata(path)
            .map_err(|error| Error::file("read", path, error))?
            .is_file()
        {
            return Err(Error::NotRegularFile);
        }
        let data = fs::read(path).map_err(|error| Error::file("read", path, error))?;
        Ok((Self::parse(&data, kind)?, data))
    }

    pub(crate) fn parse(data: &[u8], kind: Kind) -> Result<Self, Error> {
        let order = match data.get(..2) {
            Some([6, 0]) => ByteOrder::Little,
            Some([0, 6]) => ByteOrder::Big,
            _ => return Err(Error::InvalidContainer),
        };
        let mut reader = Reader {
            data,
            offset: 2,
            order,
        };
        if reader.take(6)? != b"Domain"
            || !order.tag_matches(reader.take(4)?, b"OBJD")
            || reader.text()? != b"AObjDoc"
            || reader.take(1)? != [4]
        {
            return Err(Error::InvalidContainer);
        }

        reader.text()?; // Save date.
        let count_offset = reader.offset;
        let count = reader.u32()? as usize;
        let root_index = reader.u32()? as usize;
        reader.take(20)?;
        reader.text()?; // Creator.
        reader.take(16)?;
        if root_index == 0 || root_index > count || count > (data.len() - reader.offset) / 8 {
            return Err(Error::Truncated);
        }

        let mut root = None;
        for index in 1..=count {
            let tag = reader.take(4)?;
            let size = reader.u32()? as usize;
            let start = reader.offset;
            let payload = reader.take(size)?;
            if index == root_index {
                let matches = match kind {
                    Kind::Bin => [b"ABIN", b"BINF"]
                        .iter()
                        .any(|&name| order.tag_matches(tag, name)),
                    Kind::Script => order.tag_matches(tag, b"Scpt"),
                };
                if !matches {
                    return Err(Error::UnexpectedRoot);
                }
                let Some((&3, body)) = payload.split_last() else {
                    return Err(Error::Truncated);
                };
                root = Some((start, body));
            }
        }
        if reader.offset != data.len() {
            return Err(Error::Truncated);
        }
        let (root_offset, body) = root.ok_or(Error::Truncated)?;
        let mut reader = Reader {
            data: body,
            offset: 0,
            order,
        };

        match (kind, reader.take(2)?) {
            (Kind::Bin, [2, 14 | 15]) => {
                reader.take(4)?; // Bin-view reference.
            }
            (Kind::Script, [2, 2 | 3]) => {
                reader.take(10)?; // Text reference and font fields.
                if reader.u16()? != 1 {
                    return Err(Error::UnsupportedRootLayout);
                }
                reader.take(18)?; // Rectangle, scroll position and slate height.
                let slates = reader.u32()? as usize;
                reader.take(slates.checked_mul(4).ok_or(Error::Truncated)?)?;
            }
            _ => return Err(Error::UnsupportedRootLayout),
        }

        let offset = root_offset + reader.offset;
        let uid = match kind {
            Kind::Bin => reader.u64()?,
            Kind::Script => (u64::from(reader.u32()?) << 32) | u64::from(reader.u32()?),
        };
        Ok(Self {
            uid,
            offset,
            count_offset,
            kind,
            order,
        })
    }

    pub(crate) fn replace_uid(&self, data: &mut [u8], uid: u64) {
        let field = &mut data[self.offset..self.offset + 8];
        match (self.kind, self.order) {
            (Kind::Bin, ByteOrder::Little) => field.copy_from_slice(&uid.to_le_bytes()),
            (_, ByteOrder::Big) => field.copy_from_slice(&uid.to_be_bytes()),
            (Kind::Script, ByteOrder::Little) => {
                field[..4].copy_from_slice(&((uid >> 32) as u32).to_le_bytes());
                field[4..].copy_from_slice(&(uid as u32).to_le_bytes());
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    fn tag_matches(self, wire: &[u8], tag: &[u8; 4]) -> bool {
        match self {
            Self::Little => wire.iter().eq(tag.iter().rev()),
            Self::Big => wire == tag,
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
    order: ByteOrder,
}

impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], Error> {
        let end = self.offset.checked_add(size).ok_or(Error::Truncated)?;
        let bytes = self.data.get(self.offset..end).ok_or(Error::Truncated)?;
        self.offset = end;
        Ok(bytes)
    }

    fn text(&mut self) -> Result<&'a [u8], Error> {
        let size = usize::from(self.u16()?);
        self.take(size)
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let bytes = self.take(2)?.try_into().expect("two bytes");
        Ok(match self.order {
            ByteOrder::Little => u16::from_le_bytes(bytes),
            ByteOrder::Big => u16::from_be_bytes(bytes),
        })
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?.try_into().expect("four bytes");
        Ok(match self.order {
            ByteOrder::Little => u32::from_le_bytes(bytes),
            ByteOrder::Big => u32::from_be_bytes(bytes),
        })
    }

    fn u64(&mut self) -> Result<u64, Error> {
        let bytes = self.take(8)?.try_into().expect("eight bytes");
        Ok(match self.order {
            ByteOrder::Little => u64::from_le_bytes(bytes),
            ByteOrder::Big => u64::from_be_bytes(bytes),
        })
    }
}
