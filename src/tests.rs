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

mod independent {
    use super::*;
    use crate::{CopyMode, prepare_copies};
    use avid_reader::document::MobKind;
    use avid_reader::io::MobId;
    use avid_reader::objects::{Node, Value};
    use avid_reader::{AvbDocument, AvbFile, AvbIndex};

    fn id(index: u32) -> MobId {
        MobId::from_urn(&format!(
            "060a2b340101010501010f1013000000.{index:08x}000140018000000000000001"
        ))
        .unwrap()
    }

    fn fixture(big: bool) -> Vec<u8> {
        let mut document = AvbDocument::new_empty_bin(2345, "2026-11-17 12:00:00", 1, big);
        let media = document
            .create_composition("Physical media", MobKind::Source, id(1), 200, 1)
            .unwrap();
        let master = document
            .create_composition("Master clip", MobKind::Master, id(2), 200, 1)
            .unwrap();
        let media_track = document
            .create_source_clip(25.0, 1, 200, id(1), 1, 0)
            .unwrap();
        document.append_track(master, 1, media_track).unwrap();

        let subclip = document
            .create_composition("Subclip", MobKind::Composition, id(3), 40, 1)
            .unwrap();
        document
            .set_field(subclip, "usage_code", Value::Int(2))
            .unwrap();
        let parent = document
            .create_source_clip(25.0, 1, 40, id(2), 1, 10)
            .unwrap();
        document.append_track(subclip, 1, parent).unwrap();

        let cut = document
            .create_composition("Timeline", MobKind::Composition, id(4), 25, 1)
            .unwrap();
        let source = document
            .create_source_clip(25.0, 1, 25, id(3), 1, 2)
            .unwrap();
        let track = document.create_sequence(25.0, 1, &[source]).unwrap();
        document.append_track(cut, 1, track).unwrap();

        let outer = document
            .create_composition("Outer cut", MobKind::Composition, id(5), 25, 1)
            .unwrap();
        let nested_source = document
            .create_source_clip(25.0, 1, 25, id(4), 1, 0)
            .unwrap();
        let nested_track = document.create_sequence(25.0, 1, &[nested_source]).unwrap();
        document.append_track(outer, 1, nested_track).unwrap();

        for item in [outer, subclip, master, cut, media] {
            document.add_bin_item(item).unwrap();
        }
        let root = document.root_id();
        let Value::List(mut rows) = document.root().unwrap().get("items").unwrap().clone() else {
            unreachable!()
        };
        let Value::Node(row) = rows.last_mut().unwrap() else {
            unreachable!()
        };
        row.fields
            .iter_mut()
            .find(|(key, _)| *key == "user_placed")
            .unwrap()
            .1 = Value::Bool(false);
        document
            .set_field(root, "items", Value::List(rows))
            .unwrap();
        document.to_bytes().unwrap()
    }

    fn mob(node: &Node) -> MobId {
        let Value::MobId(id) = node.get("mob_id").unwrap() else {
            panic!("missing mob ID")
        };
        *id
    }

    fn rows(bytes: &[u8]) -> (Vec<(u32, bool)>, AvbFile) {
        let file = AvbFile::parse(bytes).unwrap();
        let Value::List(items) = file.root().unwrap().get("items").unwrap() else {
            unreachable!()
        };
        let rows = items
            .iter()
            .map(|item| {
                let Value::Node(row) = item else {
                    unreachable!()
                };
                (
                    row.get("mob").unwrap().as_index().unwrap(),
                    row.get("user_placed") == Some(&Value::Bool(true)),
                )
            })
            .collect();
        (rows, file)
    }

    fn source(file: &AvbFile, item: u32) -> &Node {
        let node = file.node(item).unwrap();
        let Value::List(tracks) = node.get("tracks").unwrap() else {
            unreachable!()
        };
        let Value::Node(track) = &tracks[0] else {
            unreachable!()
        };
        let component = file
            .node(track.get("component").unwrap().as_index().unwrap())
            .unwrap();
        if let Some(Value::RefList(items)) = component.get("components") {
            file.node(items[0].as_index().unwrap()).unwrap()
        } else {
            component
        }
    }

    #[test]
    fn mixed_bin_copies_preserve_original_timeline_dependencies() {
        let mut used_ids = HashSet::new();
        for big in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("Mixed.avb");
            let blocked_name = dir.path().join("Mixed Copy.avb");
            fs::write(&blocked_name, b"existing output").unwrap();
            let original = fixture(big);
            fs::write(&input, &original).unwrap();
            let (old_rows, old_file) = rows(&original);
            let prepare = || {
                prepare_copies(&input, dir.path(), 2, HashSet::new(), CopyMode::Independent)
                    .unwrap()
            };
            let canceled = prepare();
            assert!(canceled.has_timeline());
            assert!(matches!(
                canceled.save(false),
                Err(Error::TimelineConfirmation)
            ));
            drop(prepare());
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
            let results = prepare().save(true).unwrap();
            assert!(results.error.is_none());
            assert_eq!(results.copies.len(), 2);
            assert_eq!(results.copies[0].path, dir.path().join("Mixed Copy 2.avb"));
            assert_eq!(fs::read(&blocked_name).unwrap(), b"existing output");
            assert_eq!(fs::read(&input).unwrap(), original);

            for output in results.copies {
                let copy = fs::read(output.path).unwrap();
                let (new_rows, file) = rows(&copy);
                let index = AvbIndex::from_bytes(copy).unwrap();
                assert_eq!(index.root_index as usize, index.object_count() - 1);
                let original_index = AvbIndex::from_bytes(original.clone()).unwrap();
                for object in 1..original_index.object_count() as u32 {
                    assert_eq!(index.chunk(object), original_index.chunk(object));
                }
                let visible: Vec<_> = new_rows
                    .iter()
                    .filter(|row| row.1)
                    .map(|row| row.0)
                    .collect();
                assert_eq!(visible.len(), 4);
                for (new, (old, _)) in visible.iter().zip(&old_rows) {
                    let copied = file.node(*new).unwrap();
                    let original = old_file.node(*old).unwrap();
                    assert_eq!(copied.get("name"), original.get("name"));
                    assert_ne!(copied.get("attributes"), original.get("attributes"));
                    assert_ne!(mob(copied), mob(original));
                    assert!(used_ids.insert(mob(copied)));
                    assert!(new_rows.contains(&(*old, false)));
                }
                assert_eq!(mob(source(&file, visible[0])), id(4));
                assert_eq!(mob(source(&file, visible[3])), id(3));
                assert_eq!(
                    mob(source(&file, visible[1])),
                    mob(file.node(visible[2]).unwrap())
                );
                assert_eq!(
                    source(&file, visible[1]).get("start_time"),
                    Some(&Value::Int(10))
                );
                assert_eq!(mob(source(&file, visible[2])), id(1));
            }
        }
    }

    #[test]
    fn unknown_editorial_item_can_be_copied_only_in_bin_only_mode() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("Unsupported.avb");
        let mut document = AvbDocument::from_bytes(fixture(false)).unwrap();
        let (items, _) = rows(&fixture(false));
        document
            .set_field(items[0].0, "usage_code", Value::Int(99))
            .unwrap();
        fs::write(&input, document.to_bytes().unwrap()).unwrap();
        assert!(matches!(
            prepare_copies(&input, dir.path(), 2, HashSet::new(), CopyMode::Independent),
            Err(Error::IndependentBin(_))
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        let result = create_copies(&input, dir.path(), 1, HashSet::new()).unwrap();
        assert_eq!(result.copies.len(), 1);
        assert!(result.error.is_none());
    }
    #[test]
    fn invalid_bin_item_reference_creates_no_copies() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Bad ref.avb");
        let mut bytes = fixture(false);
        let index = AvbIndex::from_bytes(bytes.clone()).unwrap();
        let first_item = index.object(index.root_index).unwrap().payload.start + 16;
        bytes[first_item..first_item + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            prepare_copies(
                &path,
                directory.path(),
                2,
                HashSet::new(),
                CopyMode::Independent
            ),
            Err(Error::IndependentBin(_))
        ));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
