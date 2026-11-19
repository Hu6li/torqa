//! How much of a region's buildings OpenStreetMap describes beyond their outline: heights,
//! levels, roof shapes, colours and materials, and buildings drawn in parts. The vector tiles
//! carry only an outline, a height and a colour, so this measures what a building extract
//! would add (ADR 0041).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use prost::Message;

use super::pbf;
use crate::OsmError;

/// The tags a building's shape and look can come from, counted per building carrying them.
pub const SHAPE_TAGS: [&str; 14] = [
    "height",
    "building:levels",
    "min_height",
    "building:min_level",
    "roof:shape",
    "roof:height",
    "roof:levels",
    "roof:angle",
    "roof:direction",
    "roof:orientation",
    "roof:colour",
    "building:colour",
    "building:material",
    "roof:material",
];

/// A region's buildings and what their tags say of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildingCensus {
    /// Ways mapped as a building (`building=*` but `no`).
    pub buildings: u64,
    /// Multipolygon relations mapped as a building: their outlines are the tiles' alone.
    pub multipolygons: u64,
    /// Ways mapped as a part of a building (`building:part=*` but `no`).
    pub parts: u64,
    /// Building ways carrying each of [`SHAPE_TAGS`], by tag.
    pub tagged: BTreeMap<&'static str, u64>,
    /// Part ways carrying each of [`SHAPE_TAGS`], by tag.
    pub parts_tagged: BTreeMap<&'static str, u64>,
    /// Building ways with a height or a number of levels.
    pub with_height: u64,
    /// Building ways with any of [`SHAPE_TAGS`]: those an extract would keep.
    pub described: u64,
    /// Their nodes, with those of every part: what an extract would store of their outlines.
    pub described_nodes: u64,
    /// The `roof:shape` values of buildings and parts, by value.
    pub roof_shapes: BTreeMap<String, u64>,
    /// The `building=*` values, by value.
    pub kinds: BTreeMap<String, u64>,
}

impl BuildingCensus {
    fn add(&mut self, other: Self) {
        self.buildings += other.buildings;
        self.multipolygons += other.multipolygons;
        self.parts += other.parts;
        self.with_height += other.with_height;
        self.described += other.described;
        self.described_nodes += other.described_nodes;
        for (tag, count) in other.tagged {
            *self.tagged.entry(tag).or_default() += count;
        }
        for (tag, count) in other.parts_tagged {
            *self.parts_tagged.entry(tag).or_default() += count;
        }
        for (value, count) in other.roof_shapes {
            *self.roof_shapes.entry(value).or_default() += count;
        }
        for (value, count) in other.kinds {
            *self.kinds.entry(value).or_default() += count;
        }
    }

    fn read(&mut self, bytes: &[u8]) -> Result<(), OsmError> {
        let block = TaggedBlock::decode(bytes).map_err(pbf::invalid)?;
        for group in &block.primitivegroup {
            for way in &group.ways {
                let tags = Tags::of(&block, &way.keys, &way.vals);
                self.way(&tags, way.refs.len());
            }
            for relation in &group.relations {
                let tags = Tags::of(&block, &relation.keys, &relation.vals);
                if tags.get("type") == Some("multipolygon")
                    && tags.get("building").is_some_and(|value| value != "no")
                {
                    self.multipolygons += 1;
                }
            }
        }
        Ok(())
    }

    fn way(&mut self, tags: &Tags<'_>, nodes: usize) {
        let building = tags.get("building").filter(|&value| value != "no");
        let part = tags.get("building:part").filter(|&value| value != "no");
        if building.is_none() && part.is_none() {
            return;
        }
        let present: Vec<&'static str> = SHAPE_TAGS
            .into_iter()
            .filter(|&tag| tags.get(tag).is_some())
            .collect();
        if let Some(shape) = tags.get("roof:shape") {
            *self.roof_shapes.entry(shape.to_owned()).or_default() += 1;
        }
        if let Some(kind) = building {
            self.buildings += 1;
            *self.kinds.entry(kind.to_owned()).or_default() += 1;
            for &tag in &present {
                *self.tagged.entry(tag).or_default() += 1;
            }
            if present.contains(&"height") || present.contains(&"building:levels") {
                self.with_height += 1;
            }
            if !present.is_empty() {
                self.described += 1;
                self.described_nodes += nodes as u64;
            }
        } else {
            self.parts += 1;
            self.described_nodes += nodes as u64;
            for &tag in &present {
                *self.parts_tagged.entry(tag).or_default() += 1;
            }
        }
    }
}

/// Counts the buildings of the region download `pbf` and the tags describing them, reading
/// its ways and relations once on every core; `progress` hears of bytes read of the file's.
///
/// # Errors
/// [`OsmError::Cache`] if the file cannot be read, [`OsmError::Invalid`] if it is no
/// OpenStreetMap download this can read, [`OsmError::Stopped`] once stopped.
pub fn building_census(
    pbf: &Path,
    progress: &mut dyn FnMut(u64, u64),
    stop: &AtomicBool,
) -> Result<BuildingCensus, OsmError> {
    let (_, parts) = pbf::fold(
        pbf,
        BuildingCensus::default,
        BuildingCensus::read,
        progress,
        stop,
    )?;
    let mut census = BuildingCensus::default();
    for part in parts {
        census.add(part);
    }
    Ok(census)
}

/// A way's or relation's tags, read from its block's string table.
struct Tags<'a>(Vec<(&'a [u8], &'a str)>);

impl<'a> Tags<'a> {
    fn of(block: &'a TaggedBlock, keys: &[u32], vals: &[u32]) -> Self {
        Self(
            keys.iter()
                .zip(vals)
                .map(|(&key, &value)| {
                    (
                        block.text(key),
                        std::str::from_utf8(block.text(value)).unwrap_or_default(),
                    )
                })
                .collect(),
        )
    }

    fn get(&self, key: &str) -> Option<&'a str> {
        self.0
            .iter()
            .find(|(k, _)| *k == key.as_bytes())
            .map(|&(_, value)| value)
    }
}

/// A data block as far as the tags of its ways and relations go.
#[derive(Clone, PartialEq, Message)]
struct TaggedBlock {
    #[prost(message, required, tag = "1")]
    stringtable: pbf::StringTable,
    #[prost(message, repeated, tag = "2")]
    primitivegroup: Vec<TaggedGroup>,
}

impl TaggedBlock {
    fn text(&self, index: u32) -> &[u8] {
        self.stringtable
            .s
            .get(index as usize)
            .map_or(&[][..], Vec::as_slice)
    }
}

#[derive(Clone, PartialEq, Message)]
struct TaggedGroup {
    #[prost(message, repeated, tag = "3")]
    ways: Vec<pbf::Way>,
    #[prost(message, repeated, tag = "4")]
    relations: Vec<TaggedRelation>,
}

#[derive(Clone, PartialEq, Message)]
struct TaggedRelation {
    #[prost(int64, required, tag = "1")]
    id: i64,
    #[prost(uint32, repeated, packed = "true", tag = "2")]
    keys: Vec<u32>,
    #[prost(uint32, repeated, packed = "true", tag = "3")]
    vals: Vec<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn census_of(ways: &[&[(&str, &str)]]) -> BuildingCensus {
        let mut census = BuildingCensus::default();
        for tags in ways {
            let tags = Tags(
                tags.iter()
                    .map(|&(key, value)| (key.as_bytes(), value))
                    .collect(),
            );
            census.way(&tags, 5);
        }
        census
    }

    #[test]
    fn counts_what_buildings_say_of_their_shape() {
        let census = census_of(&[
            &[
                ("building", "house"),
                ("roof:shape", "gabled"),
                ("building:levels", "2"),
            ],
            &[("building", "yes")],
            &[("building", "no"), ("height", "9")],
            &[
                ("building:part", "yes"),
                ("height", "30"),
                ("roof:shape", "dome"),
            ],
            &[("highway", "residential")],
        ]);
        assert_eq!((census.buildings, census.parts), (2, 1));
        assert_eq!(census.with_height, 1);
        assert_eq!(
            census.described, 1,
            "only buildings saying more than their outline"
        );
        assert_eq!(
            census.described_nodes, 10,
            "described buildings and every part"
        );
        assert_eq!(census.tagged.get("roof:shape"), Some(&1));
        assert_eq!(census.parts_tagged.get("height"), Some(&1));
        assert_eq!(census.roof_shapes.get("gabled"), Some(&1));
        assert_eq!(census.roof_shapes.get("dome"), Some(&1));
        assert_eq!(census.kinds.get("house"), Some(&1));
        assert_eq!(census.kinds.get("no"), None);
    }
}
