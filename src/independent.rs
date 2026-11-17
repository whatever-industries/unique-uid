use std::collections::{BTreeMap, HashMap, HashSet};

use avid_reader::AvbIndex;
use avid_reader::component_writer::{source_only_from_payload, write_component};
use avid_reader::file::MAX_FILE_BYTES;
use avid_reader::io::MobId;
use avid_reader::objects::{Node, Value};
use avid_reader::payload_writer::encode_supported_payload;
use avid_reader::primitives_extra::new_unique_mob_id;

use crate::avid::{Document, Error};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    Master,
    Subclip,
    Group,
    Sequence,
    GroupCache,
}

impl Item {
    fn kind(node: &Node) -> Option<Self> {
        match (number(node, "mob_type_id"), number(node, "usage_code")) {
            (Some(2), Some(0)) => Some(Self::Master),
            (Some(1), Some(2)) => Some(Self::Subclip),
            (Some(1), Some(4)) => Some(Self::Group),
            (Some(1), Some(0)) => Some(Self::Sequence),
            (Some(1), Some(5)) => Some(Self::GroupCache),
            _ => None,
        }
    }

    fn follows_copies(self) -> bool {
        matches!(self, Self::Subclip | Self::Group | Self::GroupCache)
    }
}

struct CloneRecord {
    original: u32,
    node: Node,
    changed_refs: bool,
    identity: Option<MobId>,
    source: Option<(MobId, [u32; 2])>,
}

pub(crate) struct IndependentBin {
    index: AvbIndex,
    records: Vec<CloneRecord>,
    rows: Vec<u32>,
    identities: Vec<MobId>,
    reserved: HashSet<MobId>,
    reserved_legacy: HashSet<[u32; 2]>,
    count_offset: usize,
    row_offset: usize,
    hidden_rows: Vec<u32>,
    output_len: usize,
    pub has_timeline: bool,
}

fn unsupported(reason: impl Into<String>) -> Error {
    Error::IndependentBin(reason.into())
}

fn number(node: &Node, field: &str) -> Option<i64> {
    node.get(field).and_then(Value::as_int)
}

fn mob_id(node: &Node) -> Option<MobId> {
    match node.get("mob_id")? {
        Value::MobId(id) => Some(*id),
        _ => None,
    }
}

fn decode(index: &AvbIndex, id: u32) -> Result<Node, Error> {
    index
        .decode_object(id)
        .map_err(|error| unsupported(format!("Could not read object {id}: {error}.")))?
        .ok_or_else(|| {
            let class = index
                .object(id)
                .map(|entry| entry.class)
                .unwrap_or_default();
            unsupported(format!(
                "Unsupported object {}.",
                String::from_utf8_lossy(&class)
            ))
        })
}

fn references(value: &mut Value, visit: &mut impl FnMut(&mut u32)) {
    match value {
        Value::Ref(id) => visit(id),
        Value::RefList(items) | Value::List(items) => {
            for item in items {
                references(item, visit);
            }
        }
        Value::Node(node) => {
            for (_, field) in &mut node.fields {
                references(field, visit);
            }
        }
        Value::Attrs(attrs) => {
            for (_, field) in attrs {
                references(field, visit);
            }
        }
        _ => {}
    }
}

fn opaque(value: &Value) -> bool {
    match value {
        Value::Bytes(bytes) => !bytes.is_empty(),
        Value::RefList(items) | Value::List(items) => items.iter().any(opaque),
        Value::Node(node) => node.fields.iter().any(|(_, value)| opaque(value)),
        Value::Attrs(attrs) => attrs.iter().any(|(_, value)| opaque(value)),
        _ => false,
    }
}

fn set_mob_id(node: &mut Node, id: MobId) {
    for (field, value) in &mut node.fields {
        if *field == "mob_id" {
            *value = Value::MobId(id);
            break;
        }
    }
}

fn tracks(node: &Node) -> Result<Vec<u32>, Error> {
    let Some(Value::List(values)) = node.get("tracks") else {
        return Err(unsupported("Missing clip track list."));
    };
    values
        .iter()
        .map(|value| {
            if let Value::Node(track) = value
                && let Some(id) = track.get("component").and_then(Value::as_index)
            {
                return Ok(id);
            }
            Err(unsupported("A clip track has no component."))
        })
        .collect()
}

fn track_sources(index: &AvbIndex, id: u32, fillers: bool) -> Result<Vec<MobId>, Error> {
    let node = decode(index, id)?;
    if node.class == *b"TCCP" {
        return Ok(Vec::new());
    }
    let leaf = if node.class == *b"SEQU" {
        let Some(Value::RefList(items)) = node.get("components") else {
            return Err(unsupported("Missing clip components."));
        };
        let mut leaf = None;
        for item in items {
            let id = item
                .as_index()
                .ok_or_else(|| unsupported("Missing clip component."))?;
            let component = decode(index, id)?;
            if fillers && component.class == *b"FILL" {
                continue;
            }
            if leaf.is_some() || component.class != *b"SCLP" {
                return Err(unsupported(
                    "A subclip or group angle has multiple sources or an unsupported effect.",
                ));
            }
            leaf = Some(component);
        }
        // A group angle can be empty for one audio lane.
        if leaf.is_none() && fillers {
            return Ok(Vec::new());
        }
        leaf.ok_or_else(|| unsupported("A subclip has no parent clip."))?
    } else {
        node
    };
    if leaf.class != *b"SCLP" {
        return Err(unsupported("Unsupported subclip or group angle component."));
    }
    Ok(vec![mob_id(&leaf).ok_or_else(|| {
        unsupported("A source is missing its full MobID.")
    })?])
}

fn legacy_words(id: MobId) -> [u32; 2] {
    [
        id.material.data1,
        u32::from(id.material.data2) | (u32::from(id.material.data3) << 16),
    ]
}

fn source_words(words: [u32; 2], old: MobId, new: MobId) -> Result<[u32; 2], Error> {
    let [old_a, old_b] = legacy_words(old);
    let [new_a, new_b] = legacy_words(new);
    match words {
        [0, 0] => Ok(words),
        [a, b] if [a, b] == [old_b, old_a] && old_a != old_b => Ok([new_b, new_a]),
        [a, b] if [a, b] == [old_a, old_b] && old_a != old_b => Ok([new_a, new_b]),
        _ => Err(unsupported("A source uses an unrecognized legacy MobID.")),
    }
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32, big_endian: bool) {
    let value = if big_endian {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    bytes[offset..offset + 4].copy_from_slice(&value);
}

struct BinItems {
    kinds: BTreeMap<u32, Item>,
    ids: HashMap<MobId, u32>,
    reserved: HashSet<MobId>,
}

impl BinItems {
    fn read(index: &AvbIndex) -> Result<Self, Error> {
        let mut kinds = BTreeMap::new();
        let mut ids = HashMap::new();
        for id in 1..index.object_count() as u32 {
            if index
                .object(id)
                .is_some_and(|object| object.class == *b"CMPO")
            {
                let node = decode(index, id)?;
                if let Some(mid) = mob_id(&node) {
                    ids.entry(mid)
                        .and_modify(|stored| *stored = 0)
                        .or_insert(id);
                }
                if let Some(kind) = Item::kind(&node) {
                    kinds.insert(id, kind);
                }
            }
        }
        let reserved = ids.keys().copied().collect();
        Ok(Self {
            kinds,
            ids,
            reserved,
        })
    }

    fn item_for(&self, id: MobId) -> Option<(u32, Item)> {
        let &object = self.ids.get(&id)?;
        Some((object, *self.kinds.get(&object)?))
    }

    fn validate_subclip(&self, index: &AvbIndex, id: u32) -> Result<(), Error> {
        let mut parent = None;
        for track in tracks(&decode(index, id)?)? {
            for source in track_sources(index, track, false)? {
                if !matches!(self.item_for(source), Some((_, Item::Master)))
                    || parent.is_some_and(|previous| previous != source)
                {
                    return Err(unsupported(
                        "Only subclips with one identifiable master parent are supported.",
                    ));
                }
                parent = Some(source);
            }
        }
        if parent.is_none() {
            return Err(unsupported("A subclip has no identifiable master parent."));
        }
        Ok(())
    }

    fn group_caches(&self, index: &AvbIndex, id: u32) -> Result<HashSet<u32>, Error> {
        let mut caches = HashSet::new();
        let mut saw_selector = false;
        for track in tracks(&decode(index, id)?)? {
            let lane = decode(index, track)?;
            if lane.class == *b"TCCP" {
                continue;
            }
            if lane.class != *b"SLCT" {
                return Err(unsupported(
                    "Only ordinary synced groups with direct selectors are supported.",
                ));
            }
            saw_selector = true;
            for angle in tracks(&lane)? {
                let leaf = decode(index, angle)?;
                if leaf.class != *b"SCLP" {
                    return Err(unsupported(
                        "A group angle uses an unsupported effect or layout.",
                    ));
                }
                let target = mob_id(&leaf)
                    .ok_or_else(|| unsupported("A group angle is missing its full MobID."))?;
                match self.item_for(target) {
                    Some((cache, Item::GroupCache)) => {
                        caches.insert(cache);
                    }
                    Some((_, Item::Master | Item::Subclip)) => {}
                    _ => {
                        return Err(unsupported(
                            "A group angle has an unresolved or nested source.",
                        ));
                    }
                }
            }
        }
        if !saw_selector {
            return Err(unsupported("The group has no supported angle selectors."));
        }
        for &cache in &caches {
            for track in tracks(&decode(index, cache)?)? {
                for source in track_sources(index, track, true)? {
                    match self.item_for(source) {
                        Some((_, Item::Master)) => {}
                        Some((object, Item::Subclip)) => self.validate_subclip(index, object)?,
                        _ => {
                            return Err(unsupported(
                                "A group angle has an unresolved or nested source.",
                            ));
                        }
                    }
                }
            }
        }
        Ok(caches)
    }
}

impl IndependentBin {
    pub(crate) fn prepare(document: &Document, bytes: Vec<u8>) -> Result<Self, Error> {
        let index = AvbIndex::from_bytes(bytes).map_err(|error| unsupported(error.to_string()))?;
        let root = decode(&index, index.root_index)?;
        let Some(Value::List(items)) = root.get("items") else {
            return Err(unsupported("The bin item list is unreadable."));
        };
        let row_offset = document.offset
            + 8
            + if root.get("large_bin") == Some(&Value::Bool(true)) {
                4
            } else {
                2
            };
        let table = BinItems::read(&index)?;
        let mut owners = BTreeMap::new();
        let mut visible = Vec::with_capacity(items.len());
        let mut caches = HashSet::new();
        let mut has_timeline = false;
        for item in items {
            let Value::Node(row) = item else {
                return Err(unsupported("A bin item has no clip record."));
            };
            let id = row
                .get("mob")
                .and_then(Value::as_index)
                .ok_or_else(|| unsupported("A bin item has no clip record."))?;
            let item = decode(&index, id)?;
            if item.class != *b"CMPO" {
                return Err(unsupported("A bin item is not an Avid composition."));
            }
            visible.push(id);
            // Native bins keep referenced mobs in the root's item list with
            // user_placed clear. Leave those rows in place.
            if row.get("user_placed") == Some(&Value::Bool(false))
                || number(&item, "mob_type_id") == Some(3)
            {
                continue;
            }
            let kind = match table.kinds.get(&id).copied() {
                Some(Item::GroupCache) | None => {
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Unnamed item");
                    return Err(unsupported(format!("Unsupported bin item “{name}”.")));
                }
                Some(kind) => kind,
            };
            if owners.insert(id, kind).is_some() {
                continue;
            }
            match kind {
                Item::Subclip => table.validate_subclip(&index, id)?,
                Item::Group => caches.extend(table.group_caches(&index, id)?),
                Item::Sequence => has_timeline = true,
                _ => {}
            }
        }
        owners.extend(caches.into_iter().map(|id| (id, Item::GroupCache)));

        let mut identities = HashSet::with_capacity(owners.len());
        for &id in owners.keys() {
            let original = decode(&index, id)?;
            let identity = mob_id(&original)
                .ok_or_else(|| unsupported("A bin item is missing its full MobID."))?;
            if table.ids.get(&identity) != Some(&id)
                || identity.length != 0x13
                || identity.label[..4] != [6, 10, 43, 52]
            {
                return Err(unsupported(
                    "A bin item has an ambiguous or unsupported MobID.",
                ));
            }
            identities.insert(identity);
        }

        let first = index.object_count() as u32;
        let mut records = Vec::new();
        let mut output_len =
            index.original_bytes().len() + index.object(index.root_index).unwrap().chunk.len();
        let mut owner_roots = HashMap::with_capacity(owners.len());
        for (&id, &kind) in &owners {
            let mut mapped = HashMap::new();
            let mut open = HashSet::new();
            let mut pending = vec![(id, None::<Node>)];
            // Emit children before owners. Media Composer reads their object
            // references while loading the file in object-table order.
            while let Some((object, waiting)) = pending.pop() {
                if let Some(mut node) = waiting {
                    let mut changed_refs = false;
                    for (_, value) in &mut node.fields {
                        references(value, &mut |reference| {
                            if let Some(&new) = mapped.get(reference) {
                                *reference = new;
                                changed_refs = true;
                            }
                        });
                    }
                    let identity = if object == id { mob_id(&node) } else { None };
                    let source = if kind.follows_copies() && node.class == *b"SCLP" {
                        mob_id(&node)
                            .filter(|target| identities.contains(target))
                            .map(|target| {
                                let source = source_only_from_payload(
                                    node.class,
                                    index.payload(object).unwrap(),
                                    index.big_endian,
                                )
                                .map_err(|error| unsupported(error.to_string()))?;
                                let words = source
                                    .source_mob_words
                                    .ok_or_else(|| unsupported("Missing legacy source MobID."))?;
                                source_words(words, target, target)?;
                                Ok::<_, Error>((target, words))
                            })
                            .transpose()?
                    } else {
                        None
                    };
                    if changed_refs || identity.is_some() || source.is_some() {
                        let original_node = decode(&index, object)?;
                        let original = index.payload(object).unwrap();
                        let rebuilt = encode_supported_payload(
                            &original_node,
                            Some(original),
                            index.big_endian,
                            Some(first - 1),
                            original.len(),
                        )
                        .map_err(|error| {
                            unsupported(format!(
                                "Could not preserve {}: {error}.",
                                node.class_str()
                            ))
                        })?;
                        if rebuilt != original {
                            return Err(unsupported(format!(
                                "Could not preserve all fields of {}.",
                                node.class_str()
                            )));
                        }
                    }
                    mapped.insert(object, first + records.len() as u32);
                    open.remove(&object);
                    records.push(CloneRecord {
                        original: object,
                        node,
                        changed_refs,
                        identity,
                        source,
                    });
                    continue;
                }

                if mapped.contains_key(&object) {
                    continue;
                }
                if !open.insert(object) {
                    return Err(unsupported("The bin has a cyclic object-table reference."));
                }
                let mut node = decode(&index, object)?;
                if node.fields.iter().any(|(name, value)| {
                    // ECCP stores an eight-byte film edgecode header.
                    !(node.class == *b"ECCP" && *name == "header") && opaque(value)
                }) {
                    return Err(unsupported(format!(
                        "An opaque {} field could contain clip references.",
                        node.class_str()
                    )));
                }
                let chunk_len = index.object(object).unwrap().chunk.len();
                output_len = output_len
                    .checked_add(chunk_len)
                    .filter(|&len| len as u64 <= MAX_FILE_BYTES)
                    .ok_or_else(|| {
                        unsupported("The independent copy would exceed the AVB size limit.")
                    })?;
                let mut children = Vec::new();
                for (_, value) in &mut node.fields {
                    references(value, &mut |reference| {
                        if index
                            .object(*reference)
                            .is_some_and(|entry| entry.class != *b"CMPO")
                        {
                            children.push((*reference, None));
                        }
                    });
                }
                pending.push((object, Some(node)));
                pending.extend(children);
            }
            owner_roots.insert(id, mapped[&id]);
        }
        let mut hidden_rows = Vec::new();
        for (&id, &kind) in &owners {
            if kind == Item::GroupCache {
                hidden_rows.push(owner_roots[&id]);
            } else if has_timeline {
                hidden_rows.push(id);
            }
        }
        let rows: Vec<_> = visible
            .into_iter()
            .enumerate()
            .map(|(position, id)| {
                let Value::Node(row) = &items[position] else {
                    unreachable!()
                };
                if row.get("user_placed") == Some(&Value::Bool(true)) {
                    owner_roots.get(&id).copied().unwrap_or(id)
                } else {
                    id
                }
            })
            .collect();
        if root.get("large_bin") == Some(&Value::Bool(false))
            && rows.len() + hidden_rows.len() > usize::from(u16::MAX)
        {
            return Err(unsupported("The copied bin has too many rows."));
        }
        output_len = output_len
            .checked_add(13 * hidden_rows.len())
            .filter(|&len| len as u64 <= MAX_FILE_BYTES)
            .ok_or_else(|| unsupported("The independent copy would exceed the AVB size limit."))?;
        let reserved_legacy = table.reserved.iter().copied().map(legacy_words).collect();
        Ok(Self {
            index,
            records,
            rows,
            identities: identities.into_iter().collect(),
            reserved: table.reserved,
            reserved_legacy,
            count_offset: document.count_offset,
            row_offset,
            hidden_rows,
            output_len,
            has_timeline,
        })
    }

    fn next_identities(&mut self) -> Result<HashMap<MobId, MobId>, Error> {
        let mut result = HashMap::with_capacity(self.identities.len());
        // Some native groups and their private caches share legacy words.
        let mut aliases: HashMap<[u32; 2], [u32; 2]> = HashMap::new();
        for &old in &self.identities {
            let legacy = legacy_words(old);
            loop {
                let random = new_unique_mob_id().map_err(|error| unsupported(error.to_string()))?;
                let mut new = MobId {
                    material: random.material,
                    ..old
                };
                if let Some(&[first, second]) = aliases.get(&legacy) {
                    new.material.data1 = first;
                    new.material.data2 = second as u16;
                    new.material.data3 = (second >> 16) as u16;
                } else if !self.reserved_legacy.insert(legacy_words(new)) {
                    continue;
                }
                if self.reserved.insert(new) {
                    aliases.entry(legacy).or_insert_with(|| legacy_words(new));
                    result.insert(old, new);
                    break;
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn render(&mut self) -> Result<Vec<u8>, Error> {
        let identities = self.next_identities()?;
        let root_id = (self.index.object_count() + self.records.len()) as u32;
        let big = self.index.big_endian;
        let mut bytes = Vec::with_capacity(self.output_len);
        bytes.extend_from_slice(self.index.original_bytes());
        put_u32(&mut bytes, self.count_offset, root_id, big);
        put_u32(&mut bytes, self.count_offset + 4, root_id, big);
        for record in &self.records {
            let original = self.index.chunk(record.original).unwrap();
            if !record.changed_refs && record.identity.is_none() && record.source.is_none() {
                bytes.extend_from_slice(original);
                continue;
            }
            let mut node = record.node.clone();
            let old_payload = &original[8..];
            if let Some(id) = record.identity {
                set_mob_id(&mut node, identities[&id]);
            }
            let payload = if let Some((old, words)) = record.source {
                let new = identities[&old];
                set_mob_id(&mut node, new);
                let mut source = source_only_from_payload(node.class, old_payload, big)
                    .map_err(|error| unsupported(error.to_string()))?;
                source.source_mob_words = Some(source_words(words, old, new)?);
                write_component(&node, big, source, old_payload.len())
            } else {
                encode_supported_payload(
                    &node,
                    Some(old_payload),
                    big,
                    Some(root_id),
                    old_payload.len(),
                )
            }
            .map_err(|error| {
                unsupported(format!("Could not copy {}: {error}.", node.class_str()))
            })?;
            if payload.len() != old_payload.len() {
                return Err(unsupported(format!(
                    "Copying {} would change its layout.",
                    node.class_str()
                )));
            }
            bytes.extend_from_slice(&original[..8]);
            bytes.extend_from_slice(&payload);
        }
        let root = self.index.object(self.index.root_index).unwrap();
        let root_bytes = self.index.chunk(self.index.root_index).unwrap();
        let first_row = self.row_offset - root.chunk.start;
        let end_of_rows = first_row + 13 * self.rows.len();
        let mut new_root = root_bytes[..end_of_rows].to_vec();
        for (row, &id) in self.rows.iter().enumerate() {
            put_u32(&mut new_root, first_row + 13 * row, id, big);
        }
        for &id in &self.hidden_rows {
            let offset = new_root.len();
            new_root.extend_from_slice(&[0; 13]);
            put_u32(&mut new_root, offset, id, big);
            let hidden_pos = if big {
                (-30_000i16).to_be_bytes()
            } else {
                (-30_000i16).to_le_bytes()
            };
            new_root[offset + 4..offset + 6].copy_from_slice(&hidden_pos);
            new_root[offset + 6..offset + 8].copy_from_slice(&hidden_pos);
        }
        new_root.extend_from_slice(&root_bytes[end_of_rows..]);
        let row_count = self.rows.len() + self.hidden_rows.len();
        if root_bytes[9] == 15 {
            put_u32(&mut new_root, first_row - 4, row_count as u32, big);
        } else {
            let count = if big {
                (row_count as u16).to_be_bytes()
            } else {
                (row_count as u16).to_le_bytes()
            };
            new_root[first_row - 2..first_row].copy_from_slice(&count);
        }
        let payload_len = (new_root.len() - 8) as u32;
        put_u32(&mut new_root, 4, payload_len, big);
        bytes.extend_from_slice(&new_root);
        Ok(bytes)
    }
}
