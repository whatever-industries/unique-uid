use std::collections::HashSet;
use std::fs;

use super::avid::{Document, Kind};
use super::{Error, create_copies, read_uid};

const ORIGINAL: u64 = 0x1234_abcd_7654_fedc;
const REPLACEMENT: u64 = 0xdefa_9876_4567_9abc;

#[test]
fn patches_only_the_root_uid_in_both_byte_orders() {
    for (kind, big_endian, expected) in [
        (
            Kind::Bin,
            false,
            [0xbc, 0x9a, 0x67, 0x45, 0x76, 0x98, 0xfa, 0xde],
        ),
        (
            Kind::Script,
            false,
            [0x76, 0x98, 0xfa, 0xde, 0xbc, 0x9a, 0x67, 0x45],
        ),
        (Kind::Bin, true, REPLACEMENT.to_be_bytes()),
        (Kind::Script, true, REPLACEMENT.to_be_bytes()),
    ] {
        let (original, offset) = fixture(kind, big_endian);
        let document = Document::parse(&original, kind).unwrap();
        assert_eq!(document.uid, ORIGINAL);
        assert_eq!(document.offset, offset);
        let mut copy = original.clone();
        document.replace_uid(&mut copy, REPLACEMENT);
        assert_eq!(copy[..offset], original[..offset]);
        assert_eq!(copy[offset..offset + 8], expected);
        assert_eq!(copy[offset + 8..], original[offset + 8..]);
        assert_eq!(Document::parse(&copy, kind).unwrap().uid, REPLACEMENT);
    }
}

#[test]
fn rejects_mismatched_and_malformed_files() {
    let (bytes, offset) = fixture(Kind::Script, false);
    assert!(matches!(
        Document::parse(&bytes, Kind::Bin),
        Err(Error::UnexpectedRoot)
    ));
    assert!(matches!(
        Document::parse(&bytes[..bytes.len() - 1], Kind::Script),
        Err(Error::Truncated)
    ));
    let mut bad_slates = bytes.clone();
    bad_slates[offset - 12..offset - 8].fill(255);
    assert!(matches!(
        Document::parse(&bad_slates, Kind::Script),
        Err(Error::Truncated)
    ));
    let mut bad_rectangle = bytes;
    bad_rectangle[offset - 32] = 7;
    assert!(matches!(
        Document::parse(&bad_rectangle, Kind::Script),
        Err(Error::UnsupportedRootLayout)
    ));
}

#[test]
fn batch_skips_existing_names_and_preserves_originals() {
    for (kind, extension) in [(Kind::Bin, "AVB"), (Kind::Script, "AVC")] {
        let directory = tempfile::tempdir().unwrap();
        let (bytes, offset) = fixture(kind, false);
        let source = directory.path().join(format!("Sample.{extension}"));
        let existing = directory.path().join(format!("Sample Copy.{extension}"));
        fs::write(&source, &bytes).unwrap();
        fs::write(&existing, b"existing").unwrap();

        let batch =
            create_copies(&source, directory.path(), 3, HashSet::from([REPLACEMENT])).unwrap();
        assert!(batch.error.is_none());
        assert_eq!(batch.copies.len(), 3);
        let mut used = HashSet::from([0, ORIGINAL, REPLACEMENT]);
        for (index, copy) in (2..=4).zip(&batch.copies) {
            assert_eq!(
                copy.path.file_name().unwrap().to_str().unwrap(),
                format!("Sample Copy {index}.{extension}")
            );
            assert!(used.insert(copy.uid));
            let output = fs::read(&copy.path).unwrap();
            assert_eq!(output[..offset], bytes[..offset]);
            assert_eq!(output[offset + 8..], bytes[offset + 8..]);
            assert_eq!(read_uid(&copy.path).unwrap(), copy.uid);
        }
        assert_eq!(fs::read(&source).unwrap(), bytes);
        assert_eq!(fs::read(&existing).unwrap(), b"existing");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 5);
    }
}

#[test]
fn concurrent_batches_claim_different_names() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("Sample.avb");
    fs::write(&source, fixture(Kind::Bin, false).0).unwrap();

    std::thread::scope(|scope| {
        let start = || create_copies(&source, directory.path(), 3, HashSet::new()).unwrap();
        let one = scope.spawn(start);
        let two = scope.spawn(start);
        let names: HashSet<_> = [one.join().unwrap(), two.join().unwrap()]
            .into_iter()
            .flat_map(|batch| {
                assert!(batch.error.is_none());
                assert_eq!(batch.copies.len(), 3);
                batch.copies.into_iter().map(|copy| copy.path)
            })
            .collect();
        assert_eq!(names.len(), 6);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 7);
    });
}

#[test]
fn candidate_hard_link_cannot_modify_or_replace_the_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("Scene.avb");
    let occupied = directory.path().join("Scene Copy.avb");
    let (original, _) = fixture(Kind::Bin, false);
    fs::write(&source, &original).unwrap();
    fs::hard_link(&source, &occupied).unwrap();

    let batch = create_copies(&source, directory.path(), 1, HashSet::new()).unwrap();
    assert!(batch.error.is_none());
    assert_eq!(batch.copies.len(), 1);
    assert_eq!(
        batch.copies[0].path,
        directory.path().join("Scene Copy 2.avb")
    );
    assert_eq!(fs::read(&source).unwrap(), original);
    assert_eq!(fs::read(&occupied).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn candidate_dangling_symlink_is_skipped() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("Scene.avc");
    let occupied = directory.path().join("Scene Copy.avc");
    let missing = directory.path().join("Unrelated.avc");
    fs::write(&source, fixture(Kind::Script, false).0).unwrap();
    std::os::unix::fs::symlink(&missing, &occupied).unwrap();

    let batch = create_copies(&source, directory.path(), 1, HashSet::new()).unwrap();
    assert!(batch.error.is_none());
    assert_eq!(batch.copies.len(), 1);
    assert_eq!(
        batch.copies[0].path,
        directory.path().join("Scene Copy 2.avc")
    );
    assert_eq!(fs::read_link(&occupied).unwrap(), missing);
    assert!(!missing.exists());
}

#[test]
fn malformed_source_does_not_create_output_or_touch_an_existing_copy() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("Scene.avc");
    let occupied = directory.path().join("Scene Copy.avc");
    let (mut malformed, _) = fixture(Kind::Script, false);
    malformed.pop();
    fs::write(&source, &malformed).unwrap();
    let (original, _) = fixture(Kind::Script, true);
    fs::write(&occupied, &original).unwrap();

    assert!(matches!(
        create_copies(&source, directory.path(), 1, HashSet::new()),
        Err(Error::Truncated)
    ));
    assert_eq!(fs::read(&source).unwrap(), malformed);
    assert_eq!(fs::read(&occupied).unwrap(), original);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

fn fixture(kind: Kind, big_endian: bool) -> (Vec<u8>, usize) {
    let number = |value: u64, width: usize| {
        if big_endian {
            value.to_be_bytes()[8 - width..].to_vec()
        } else {
            value.to_le_bytes()[..width].to_vec()
        }
    };
    let tag = |value: &[u8; 4]| {
        if big_endian {
            value.to_vec()
        } else {
            value.iter().rev().copied().collect()
        }
    };
    let root_tag = match kind {
        Kind::Bin => b"ABIN",
        Kind::Script => b"Scpt",
    };

    let mut data = number(6, 2);
    data.extend_from_slice(b"Domain");
    data.extend(tag(b"OBJD"));
    data.extend(number(7, 2));
    data.extend_from_slice(b"AObjDoc");
    data.push(4);
    data.extend(number(0, 2));
    data.extend(number(3, 4));
    data.extend(number(2, 4));
    data.extend([0; 38]);

    let mut append_chunk = |name, payload: &[u8]| {
        let start = data.len() + 8;
        data.extend(tag(name));
        data.extend(number(payload.len() as u64, 4));
        data.extend_from_slice(payload);
        start
    };

    // The first object is a decoy with the same tag as the real root.
    append_chunk(root_tag, &[0xa0, 0xa1, 0xa2]);
    let mut payload;
    let relative_offset;
    match kind {
        Kind::Bin => {
            payload = vec![2, 14];
            payload.extend(number(1, 4));
            relative_offset = 6;
            payload.extend(number(ORIGINAL, 8));
        }
        Kind::Script => {
            payload = vec![2, 3];
            payload.extend(number(1, 4));
            payload.extend([0; 6]);
            payload.extend(number(1, 2));
            payload.extend([0; 18]);
            payload.extend(number(2, 4));
            payload.extend(number(1, 4));
            payload.extend(number(3, 4));
            relative_offset = 44;
            payload.extend(number(ORIGINAL >> 32, 4));
            payload.extend(number(ORIGINAL & 0xffff_ffff, 4));
        }
    }
    payload.extend([0xa3, 0xa4, 3]);
    let payload_start = append_chunk(root_tag, &payload);
    append_chunk(b"ATTR", &[0xb0, 0xb1, 0xb2, 0xb3]);
    (data, payload_start + relative_offset)
}
