//! OpenStreetMap's PBF format (`.osm.pbf`), as far as map detail reads it: the positions of
//! nodes and the tags and nodes of ways (<https://wiki.openstreetmap.org/wiki/PBF_Format>).
//!
//! The messages declare only the fields read; Protocol Buffers skip the others unread, so a
//! pass over the ways never decodes the nodes, which are most of a file.

use std::fmt::Display;
use std::fs::File;
use std::io::{BufReader, Read};
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, PoisonError};
use std::thread;

use flate2::read::ZlibDecoder;
use prost::Message;

use crate::OsmError;

/// The format's limits on the header of a blob and on a blob, packed or not.
const MAX_HEADER: usize = 64 * 1024;
const MAX_BLOB: usize = 32 * 1024 * 1024;
/// Features a file may require that this reader understands.
const UNDERSTOOD: [&str; 2] = ["OsmSchema-V0.6", "DenseNodes"];

#[derive(Clone, PartialEq, Message)]
pub(crate) struct BlobHeader {
    #[prost(string, required, tag = "1")]
    pub kind: String,
    #[prost(int32, required, tag = "3")]
    pub datasize: i32,
}

/// A blob, stored as is or packed with zlib; files using other packings are refused.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct Blob {
    #[prost(bytes = "vec", optional, tag = "1")]
    pub raw: Option<Vec<u8>>,
    #[prost(int32, optional, tag = "2")]
    pub raw_size: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "3")]
    pub zlib_data: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct HeaderBlock {
    #[prost(string, repeated, tag = "4")]
    pub required_features: Vec<String>,
    #[prost(int64, optional, tag = "32")]
    pub osmosis_replication_timestamp: Option<i64>,
}

/// A data block as far as its ways go.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct WayBlock {
    #[prost(message, required, tag = "1")]
    pub stringtable: StringTable,
    #[prost(message, repeated, tag = "2")]
    pub primitivegroup: Vec<WayGroup>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct StringTable {
    #[prost(bytes = "vec", repeated, tag = "1")]
    pub s: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct WayGroup {
    #[prost(message, repeated, tag = "3")]
    pub ways: Vec<Way>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Way {
    #[prost(int64, required, tag = "1")]
    pub id: i64,
    /// Tag keys and values, as indices into the block's string table.
    #[prost(uint32, repeated, packed = "true", tag = "2")]
    pub keys: Vec<u32>,
    #[prost(uint32, repeated, packed = "true", tag = "3")]
    pub vals: Vec<u32>,
    /// Node ids, each but the first as the difference to the one before.
    #[prost(sint64, repeated, packed = "true", tag = "8")]
    pub refs: Vec<i64>,
}

/// A data block as far as its nodes' positions go.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct NodeBlock {
    #[prost(message, repeated, tag = "2")]
    pub primitivegroup: Vec<NodeGroup>,
    /// Nanodegrees per unit of position; 100 unless the file says otherwise.
    #[prost(int32, optional, tag = "17")]
    pub granularity: Option<i32>,
    #[prost(int64, optional, tag = "19")]
    pub lat_offset: Option<i64>,
    #[prost(int64, optional, tag = "20")]
    pub lon_offset: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct NodeGroup {
    #[prost(message, repeated, tag = "1")]
    pub nodes: Vec<Node>,
    #[prost(message, optional, tag = "2")]
    pub dense: Option<DenseNodes>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Node {
    #[prost(sint64, required, tag = "1")]
    pub id: i64,
    #[prost(sint64, required, tag = "8")]
    pub lat: i64,
    #[prost(sint64, required, tag = "9")]
    pub lon: i64,
}

/// Nodes as columns, each id and position but the first as the difference to the one before.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct DenseNodes {
    #[prost(sint64, repeated, packed = "true", tag = "1")]
    pub id: Vec<i64>,
    #[prost(sint64, repeated, packed = "true", tag = "8")]
    pub lat: Vec<i64>,
    #[prost(sint64, repeated, packed = "true", tag = "9")]
    pub lon: Vec<i64>,
}

impl WayBlock {
    /// The string at `index` of the block's table; empty if there is none.
    pub(crate) fn text(&self, index: u32) -> &[u8] {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.stringtable.s.get(i))
            .map_or(&[], Vec::as_slice)
    }
}

impl NodeBlock {
    /// Each node's id and position (latitude, longitude in 1e-7 degrees), in block order.
    /// Positions beyond the map are left out.
    pub(crate) fn each(&self, mut node: impl FnMut(i64, (i32, i32))) {
        let granularity = i64::from(self.granularity.unwrap_or(100));
        let (lat_offset, lon_offset) = (self.lat_offset.unwrap_or(0), self.lon_offset.unwrap_or(0));
        let e7 = |offset: i64, units: i64| {
            i32::try_from((offset + granularity * units + 50).div_euclid(100)).ok()
        };
        let mut found = |id, lat, lon| {
            if let (Some(lat), Some(lon)) = (e7(lat_offset, lat), e7(lon_offset, lon)) {
                node(id, (lat, lon));
            }
        };
        for group in &self.primitivegroup {
            for plain in &group.nodes {
                found(plain.id, plain.lat, plain.lon);
            }
            if let Some(dense) = &group.dense {
                let (mut id, mut lat, mut lon) = (0, 0, 0);
                for ((d_id, d_lat), d_lon) in dense.id.iter().zip(&dense.lat).zip(&dense.lon) {
                    id += d_id;
                    lat += d_lat;
                    lon += d_lon;
                    found(id, lat, lon);
                }
            }
        }
    }
}

/// What a file tells about itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Header {
    /// When its data was current (Unix seconds), if it says.
    pub data_time: Option<i64>,
}

/// Reads the file's data blocks on every core. Each block, unpacked, is folded into the state
/// of the thread that took it (each made by `init`); the states come back in no order.
/// `progress` hears the bytes read of the file's size; once `stop` is set, it stops.
///
/// # Errors
/// [`OsmError::Cache`] if the file cannot be read, [`OsmError::Invalid`] if it is no PBF file
/// this reader understands or `fold` refuses a block, [`OsmError::Stopped`] once stopped.
pub(crate) fn fold<T: Send>(
    path: &Path,
    init: impl Fn() -> T + Sync,
    fold: impl Fn(&mut T, &[u8]) -> Result<(), OsmError> + Sync,
    progress: &mut dyn FnMut(u64, u64),
    stop: &AtomicBool,
) -> Result<(Header, Vec<T>), OsmError> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let workers = thread::available_parallelism().map_or(1, NonZeroUsize::get);
    // Room for a few blobs per worker: reading stays ahead without holding the file in memory.
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(2 * workers);
    let rx = Mutex::new(rx);
    let failed: Mutex<Option<OsmError>> = Mutex::new(None);
    let fail = |error| {
        failed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(error);
    };
    thread::scope(|scope| {
        let threads: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut state = init();
                    // After a failure the rest is taken unread, so the reader is never stuck.
                    loop {
                        // The lock goes as soon as a block is taken; held in a `while let`
                        // scrutinee, it lasted the whole fold and one core did all (#597).
                        let blob = rx.lock().unwrap_or_else(PoisonError::into_inner).recv();
                        let Ok(blob) = blob else { break };
                        let failing = failed
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .is_some();
                        if !failing
                            && let Err(error) = unpack(&blob).and_then(|b| fold(&mut state, &b))
                        {
                            fail(error);
                        }
                    }
                    state
                })
            })
            .collect();
        let header = read_blobs(&mut reader, size, &tx, progress, (&failed, stop));
        drop(tx);
        let mut states = Vec::with_capacity(workers);
        for thread in threads {
            match thread.join() {
                Ok(state) => states.push(state),
                Err(_) => fail(OsmError::Invalid("a block could not be read".to_owned())),
            }
        }
        if let Some(error) = failed.lock().unwrap_or_else(PoisonError::into_inner).take() {
            return Err(error);
        }
        Ok((header?, states))
    })
}

/// Reads the file's blobs in order, handing data blobs on to the workers, until its end, a
/// worker's failure or `stop`.
fn read_blobs(
    reader: &mut impl Read,
    size: u64,
    tx: &SyncSender<Vec<u8>>,
    progress: &mut dyn FnMut(u64, u64),
    (failed, stop): (&Mutex<Option<OsmError>>, &AtomicBool),
) -> Result<Header, OsmError> {
    let mut header = None;
    let mut done = 0u64;
    while let Some(length) = blob_header_length(reader)? {
        if stop.load(Ordering::Relaxed) {
            return Err(OsmError::Stopped);
        }
        if length > MAX_HEADER {
            return Err(invalid("a blob header is too long"));
        }
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes)?;
        let blob_header = BlobHeader::decode(bytes.as_slice()).map_err(invalid)?;
        let datasize = usize::try_from(blob_header.datasize)
            .ok()
            .filter(|&n| n <= MAX_BLOB)
            .ok_or_else(|| invalid("a blob is too large"))?;
        let mut blob = vec![0; datasize];
        reader.read_exact(&mut blob)?;
        done += (4 + length + datasize) as u64;
        match blob_header.kind.as_str() {
            "OSMHeader" => header = Some(read_header(&unpack(&blob)?)?),
            "OSMData" if header.is_none() => return Err(invalid("data before the header")),
            "OSMData" => {
                let failing = failed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_some();
                if failing || tx.send(blob).is_err() {
                    break;
                }
            }
            // Blobs of kinds the format may add later are skipped, as it asks.
            _ => {}
        }
        progress(done, size);
    }
    header.ok_or_else(|| invalid("no header"))
}

/// The length of the next blob's header; `None` at the end of the file.
fn blob_header_length(reader: &mut impl Read) -> Result<Option<usize>, OsmError> {
    let mut bytes = [0u8; 4];
    let mut read = 0;
    while read < bytes.len() {
        match reader.read(&mut bytes[read..])? {
            0 if read == 0 => return Ok(None),
            0 => return Err(invalid("the file ends inside a blob")),
            n => read += n,
        }
    }
    Ok(usize::try_from(u32::from_be_bytes(bytes)).ok())
}

fn read_header(bytes: &[u8]) -> Result<Header, OsmError> {
    let header = HeaderBlock::decode(bytes).map_err(invalid)?;
    if let Some(feature) = header
        .required_features
        .iter()
        .find(|f| !UNDERSTOOD.contains(&f.as_str()))
    {
        return Err(invalid(format!("the file needs “{feature}”")));
    }
    Ok(Header {
        data_time: header.osmosis_replication_timestamp,
    })
}

/// A blob's content, unpacked.
fn unpack(blob: &[u8]) -> Result<Vec<u8>, OsmError> {
    let blob = Blob::decode(blob).map_err(invalid)?;
    if let Some(raw) = blob.raw {
        return Ok(raw);
    }
    let Some(packed) = blob.zlib_data else {
        return Err(invalid("a blob is packed other than with zlib"));
    };
    let size = blob
        .raw_size
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0)
        .min(MAX_BLOB);
    let mut data = Vec::with_capacity(size);
    ZlibDecoder::new(packed.as_slice())
        .take(MAX_BLOB as u64 + 1)
        .read_to_end(&mut data)
        .map_err(invalid)?;
    if data.len() > MAX_BLOB {
        return Err(invalid("a blob is too large"));
    }
    Ok(data)
}

pub(crate) fn invalid(error: impl Display) -> OsmError {
    OsmError::Invalid(format!("OpenStreetMap download: {error}"))
}

/// Writing PBF files, for tests: the same messages encoded, in blobs packed with zlib.
#[cfg(test)]
pub(crate) mod write {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use prost::Message;

    use super::{Blob, BlobHeader, HeaderBlock};

    /// A file of `header`, then the blocks, each encoded already.
    pub(crate) fn file(header: &HeaderBlock, blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut file = Vec::new();
        blob(&mut file, "OSMHeader", &header.encode_to_vec());
        for block in blocks {
            blob(&mut file, "OSMData", block);
        }
        file
    }

    fn blob(file: &mut Vec<u8>, kind: &str, content: &[u8]) {
        let mut packer = ZlibEncoder::new(Vec::new(), Compression::fast());
        packer.write_all(content).unwrap();
        let blob = Blob {
            raw: None,
            raw_size: Some(i32::try_from(content.len()).unwrap()),
            zlib_data: Some(packer.finish().unwrap()),
        }
        .encode_to_vec();
        let header = BlobHeader {
            kind: kind.to_owned(),
            datasize: i32::try_from(blob.len()).unwrap(),
        }
        .encode_to_vec();
        file.extend_from_slice(&u32::try_from(header.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&header);
        file.extend_from_slice(&blob);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("orqwa-pbf-{name}-{}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn nodes(ids: &[i64], positions: &[(i64, i64)]) -> Vec<u8> {
        let delta = |values: Vec<i64>| {
            let mut last = 0;
            values
                .into_iter()
                .map(|v| {
                    let d = v - last;
                    last = v;
                    d
                })
                .collect()
        };
        NodeBlock {
            primitivegroup: vec![NodeGroup {
                nodes: Vec::new(),
                dense: Some(DenseNodes {
                    id: delta(ids.to_vec()),
                    lat: delta(positions.iter().map(|p| p.0).collect()),
                    lon: delta(positions.iter().map(|p| p.1).collect()),
                }),
            }],
            granularity: None,
            lat_offset: None,
            lon_offset: None,
        }
        .encode_to_vec()
    }

    fn header() -> HeaderBlock {
        HeaderBlock {
            required_features: vec!["OsmSchema-V0.6".to_owned(), "DenseNodes".to_owned()],
            osmosis_replication_timestamp: Some(1_793_000_000),
        }
    }

    #[test]
    fn blocks_are_folded_on_more_than_one_thread_at_a_time() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        if thread::available_parallelism().map_or(1, NonZeroUsize::get) < 2 {
            return;
        }
        let blocks: Vec<Vec<u8>> = (0..16).map(|b| nodes(&[b + 1], &[(1, 1)])).collect();
        let path = temp_file("parallel", &write::file(&header(), &blocks));
        let (busy, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
        fold(
            &path,
            || (),
            |(), _bytes| {
                let now = busy.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                thread::sleep(std::time::Duration::from_millis(50));
                busy.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            },
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        std::fs::remove_file(&path).unwrap();
        let most = most.load(Ordering::SeqCst);
        assert!(most >= 2, "most at once: {most}");
    }

    #[test]
    fn reads_every_node_of_every_block_with_the_files_date() {
        let blocks: Vec<Vec<u8>> = (0..20)
            .map(|b| {
                let ids: Vec<i64> = (0..100).map(|n| b * 100 + n + 1).collect();
                let positions: Vec<(i64, i64)> = ids
                    .iter()
                    .map(|&id| (469_000_000 + id, 74_000_000 - id))
                    .collect();
                nodes(&ids, &positions)
            })
            .collect();
        let path = temp_file("nodes", &write::file(&header(), &blocks));
        let mut last = (0, 0);
        let (header, states) = fold(
            &path,
            Vec::new,
            |found: &mut Vec<(i64, (i32, i32))>, bytes| {
                NodeBlock::decode(bytes)
                    .map_err(invalid)?
                    .each(|id, at| found.push((id, at)));
                Ok(())
            },
            &mut |done, size| last = (done, size),
            &AtomicBool::new(false),
        )
        .unwrap();
        std::fs::remove_file(&path).unwrap();
        let mut found: Vec<_> = states.into_iter().flatten().collect();
        found.sort_unstable();
        assert_eq!(found.len(), 2000);
        // Granularity 100 nanodegrees: the units are 1e-7 degrees already.
        assert_eq!(found[0], (1, (469_000_001, 73_999_999)));
        assert_eq!(found[1999], (2000, (469_002_000, 73_998_000)));
        assert_eq!(header.data_time, Some(1_793_000_000));
        assert_eq!(last.0, last.1);
    }

    #[test]
    fn positions_follow_the_blocks_granularity_and_offsets() {
        let block = NodeBlock {
            primitivegroup: vec![NodeGroup {
                nodes: vec![Node {
                    id: 7,
                    lat: 46_900_000,
                    lon: 7_400_000,
                }],
                dense: None,
            }],
            granularity: Some(1000),
            lat_offset: Some(500),
            lon_offset: Some(-500),
        };
        let mut found = Vec::new();
        block.each(|id, at| found.push((id, at)));
        // 46.9° and 7.4° in micro-degrees, shifted by half a 1e-7 degree each way.
        assert_eq!(found, vec![(7, (469_000_005, 73_999_995))]);
    }

    #[test]
    fn files_needing_what_it_cannot_read_are_refused() {
        let mut needs = header();
        needs
            .required_features
            .push("HistoricalInformation".to_owned());
        let path = temp_file("history", &write::file(&needs, &[]));
        let read = fold(
            &path,
            || (),
            |(), _| Ok(()),
            &mut |_, _| {},
            &AtomicBool::new(false),
        );
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(read, Err(OsmError::Invalid(m)) if m.contains("HistoricalInformation")));
    }

    #[test]
    fn a_cut_off_file_is_an_error_not_its_beginning() {
        let blocks = vec![nodes(&[1, 2], &[(1, 1), (2, 2)])];
        let whole = write::file(&header(), &blocks);
        let path = temp_file("cut", &whole[..whole.len() - 3]);
        let read = fold(
            &path,
            || (),
            |(), _| Ok(()),
            &mut |_, _| {},
            &AtomicBool::new(false),
        );
        std::fs::remove_file(&path).unwrap();
        assert!(read.is_err());
    }

    #[test]
    fn a_read_stops_when_asked() {
        let blocks: Vec<Vec<u8>> = (0..50).map(|n| nodes(&[n + 1], &[(n, n)])).collect();
        let path = temp_file("stopped", &write::file(&header(), &blocks));
        let stop = AtomicBool::new(false);
        let mut blobs = 0;
        let read = fold(
            &path,
            || (),
            |(), _| Ok(()),
            &mut |_, _| {
                blobs += 1;
                if blobs == 10 {
                    stop.store(true, Ordering::Relaxed);
                }
            },
            &stop,
        );
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(read, Err(OsmError::Stopped)));
        assert_eq!(blobs, 10, "no blob read after");
    }

    #[test]
    fn a_refused_block_fails_the_whole_read() {
        let blocks: Vec<Vec<u8>> = (0..50).map(|n| nodes(&[n + 1], &[(n, n)])).collect();
        let path = temp_file("refused", &write::file(&header(), &blocks));
        let read = fold(
            &path,
            || (),
            |(), _| Err(OsmError::Invalid("refused".to_owned())),
            &mut |_, _| {},
            &AtomicBool::new(false),
        );
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(read, Err(OsmError::Invalid(m)) if m == "refused"));
    }
}
