//! A built [`World`] as bytes (#237), so a computer can keep the worlds of its courses and ride
//! them without building them again (R101).
//!
//! Little-endian, field after field in the order the types declare them; a list is its length
//! (`u32`) followed by its items. The chunks and the parts built whole go apart, so a keeper can
//! store and read the chunks side by side. The parts built whole begin with the [`FORMAT`] and
//! the [`GENERATOR`](crate::GENERATOR) version: a world of another format is refused, one of
//! another generator only looks as that one built it.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};

use crate::{BuildingCell, Horizon, Label, MeshData, Props, TerrainChunk, Trees, World};

/// What a stored world needs to be read and drawn. Raise it whenever a world stored before
/// would be misread or misdrawn: a field added, removed or reordered (the test of
/// `tests/data/stored-world-<FORMAT>.bin` fails then), or a change in what the app makes of the
/// data, such as a model renamed or removed or another layout of an instance buffer. Worlds of
/// another format are built again.
pub const FORMAT: u32 = 3;

/// The first bytes of a world's parts built whole.
const MAGIC: &[u8; 8] = b"TQWORLD\0";

/// Writes the parts of `world` built whole, everything but its chunks, to `out`, with the
/// version of the generator that built it (usually [`GENERATOR`](crate::GENERATOR)).
///
/// # Errors
/// On write errors, and if a list is longer than `u32::MAX` items.
pub fn write_whole(world: &World, generator: u32, out: &mut impl Write) -> io::Result<()> {
    let World {
        chunks: _,
        road,
        structures,
        railways,
        horizon: Horizon { ground, water },
        fallback_samples,
    } = world;
    let mut out = Writer(out);
    out.0.write_all(MAGIC)?;
    out.0.write_all(&FORMAT.to_le_bytes())?;
    out.0.write_all(&generator.to_le_bytes())?;
    for mesh in [road, structures, railways, ground, water] {
        out.mesh(mesh)?;
    }
    out.len(*fallback_samples)
}

/// Reads what [`write_whole`] wrote: a world without chunks, and the version of the generator
/// that built it.
///
/// # Errors
/// On read errors, and [`io::ErrorKind::InvalidData`] if the bytes are not a stored world of
/// this [`FORMAT`].
pub fn read_whole(input: &mut impl Read) -> io::Result<(World, u32)> {
    let mut input = Reader(input);
    let generator = input.header()?;
    let world = World {
        chunks: Vec::new(),
        road: input.mesh()?,
        structures: input.mesh()?,
        railways: input.mesh()?,
        horizon: Horizon {
            ground: input.mesh()?,
            water: input.mesh()?,
        },
        fallback_samples: input.len()?,
    };
    input.end()?;
    Ok((world, generator))
}

/// The version of the generator that built a world [`write_whole`] wrote, reading only the
/// beginning.
///
/// # Errors
/// On read errors, and [`io::ErrorKind::InvalidData`] if the bytes are not a stored world of
/// this [`FORMAT`].
pub fn read_generator(input: &mut impl Read) -> io::Result<u32> {
    Reader(input).header()
}

/// Writes one chunk of a world to `out`.
///
/// # Errors
/// On write errors, and if a list is longer than `u32::MAX` items.
pub fn write_chunk(chunk: &TerrainChunk, out: &mut impl Write) -> io::Result<()> {
    Writer(out).chunk(chunk)
}

/// Reads a chunk [`write_chunk`] wrote.
///
/// # Errors
/// On read errors, and [`io::ErrorKind::InvalidData`] if the bytes are not a stored chunk.
pub fn read_chunk(input: &mut impl Read) -> io::Result<TerrainChunk> {
    let mut input = Reader(input);
    let chunk = input.chunk()?;
    input.end()?;
    Ok(chunk)
}

/// The centre of a chunk [`write_chunk`] wrote, reading only the beginning: to index a stored
/// world's chunks without reading them (ADR 0040).
///
/// # Errors
/// On read errors, and [`io::ErrorKind::InvalidData`] if the bytes are not a stored chunk.
pub fn read_center(input: &mut impl Read) -> io::Result<[f32; 3]> {
    Reader(input).one()
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

struct Writer<'a, W: Write>(&'a mut W);

impl<W: Write> Writer<'_, W> {
    fn len(&mut self, len: usize) -> io::Result<()> {
        let len = u32::try_from(len).map_err(|_| invalid("list too long to store"))?;
        self.0.write_all(&len.to_le_bytes())
    }

    fn floats(&mut self, values: &[f32]) -> io::Result<()> {
        self.len(values.len())?;
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.0.write_all(&bytes)
    }

    fn text(&mut self, text: &str) -> io::Result<()> {
        self.len(text.len())?;
        self.0.write_all(text.as_bytes())
    }

    fn models(&mut self, models: &BTreeMap<String, Vec<f32>>) -> io::Result<()> {
        self.len(models.len())?;
        for (name, buffer) in models {
            self.text(name)?;
            self.floats(buffer)?;
        }
        Ok(())
    }

    fn mesh(&mut self, mesh: &MeshData) -> io::Result<()> {
        let MeshData {
            vertices,
            normals,
            uvs,
            colors,
            indices,
        } = mesh;
        self.floats(vertices.as_flattened())?;
        self.floats(normals.as_flattened())?;
        self.floats(uvs.as_flattened())?;
        self.floats(colors.as_flattened())?;
        self.len(indices.len())?;
        let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        self.0.write_all(&bytes)
    }

    fn chunk(&mut self, chunk: &TerrainChunk) -> io::Result<()> {
        let TerrainChunk {
            center,
            mesh,
            buildings,
            modelled,
            streets,
            tracks,
            urban,
            water,
            trees:
                Trees {
                    models: plants,
                    grass,
                    flowers,
                },
            props:
                Props {
                    models: props,
                    labels,
                    wires,
                },
        } = chunk;
        self.floats(center)?;
        for mesh in [mesh, buildings] {
            self.mesh(mesh)?;
        }
        self.len(modelled.len())?;
        for BuildingCell { models, shells } in modelled {
            self.models(models)?;
            self.mesh(shells)?;
        }
        for mesh in [streets, tracks, urban, water] {
            self.mesh(mesh)?;
        }
        self.models(plants)?;
        self.floats(grass)?;
        self.floats(flowers)?;
        self.models(props)?;
        self.len(labels.len())?;
        for Label { text, transform } in labels {
            self.text(text)?;
            self.floats(transform)?;
        }
        self.mesh(wires)
    }
}

struct Reader<'a, R: Read>(&'a mut R);

impl<R: Read> Reader<'_, R> {
    /// The beginning of a world's parts built whole: its generator's version.
    fn header(&mut self) -> io::Result<u32> {
        let mut magic = [0; MAGIC.len()];
        self.0.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(invalid("not a stored world"));
        }
        let mut word = [0; 4];
        self.0.read_exact(&mut word)?;
        if u32::from_le_bytes(word) != FORMAT {
            return Err(invalid("a world stored in another format"));
        }
        self.0.read_exact(&mut word)?;
        Ok(u32::from_le_bytes(word))
    }

    /// Fails unless everything was read: bytes left over mean another layout.
    fn end(&mut self) -> io::Result<()> {
        if self.0.read(&mut [0])? == 0 {
            Ok(())
        } else {
            Err(invalid("bytes after the end"))
        }
    }

    fn len(&mut self) -> io::Result<usize> {
        let mut bytes = [0; 4];
        self.0.read_exact(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes) as usize)
    }

    /// `count` items of four bytes. Read as they come rather than allocated at once, so that a
    /// damaged length ends in an error, not in a request for gigabytes.
    fn words(&mut self, count: usize) -> io::Result<Vec<[u8; 4]>> {
        let mut bytes = Vec::new();
        let wanted = count * 4;
        self.0.take(wanted as u64).read_to_end(&mut bytes)?;
        if bytes.len() != wanted {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(bytes.as_chunks::<4>().0.to_vec())
    }

    fn floats(&mut self) -> io::Result<Vec<f32>> {
        let count = self.len()?;
        Ok(self
            .words(count)?
            .into_iter()
            .map(f32::from_le_bytes)
            .collect())
    }

    /// Floats in groups of `N`, as vertices and transforms are kept.
    fn groups<const N: usize>(&mut self) -> io::Result<Vec<[f32; N]>> {
        let floats = self.floats()?;
        let (groups, rest) = floats.as_chunks::<N>();
        if !rest.is_empty() {
            return Err(invalid("floats do not make whole groups"));
        }
        Ok(groups.to_vec())
    }

    fn one<const N: usize>(&mut self) -> io::Result<[f32; N]> {
        let [group] = self.groups::<N>()?[..] else {
            return Err(invalid("expected one group of floats"));
        };
        Ok(group)
    }

    fn text(&mut self) -> io::Result<String> {
        let len = self.len()?;
        let mut bytes = Vec::new();
        self.0.take(len as u64).read_to_end(&mut bytes)?;
        if bytes.len() != len {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        String::from_utf8(bytes).map_err(|_| invalid("text is not UTF-8"))
    }

    fn models(&mut self) -> io::Result<BTreeMap<String, Vec<f32>>> {
        let count = self.len()?;
        let mut models = BTreeMap::new();
        for _ in 0..count {
            let name = self.text()?;
            models.insert(name, self.floats()?);
        }
        Ok(models)
    }

    fn mesh(&mut self) -> io::Result<MeshData> {
        Ok(MeshData {
            vertices: self.groups()?,
            normals: self.groups()?,
            uvs: self.groups()?,
            colors: self.groups()?,
            indices: {
                let count = self.len()?;
                self.words(count)?
                    .into_iter()
                    .map(u32::from_le_bytes)
                    .collect()
            },
        })
    }

    fn chunk(&mut self) -> io::Result<TerrainChunk> {
        let center = self.one()?;
        let mesh = self.mesh()?;
        let buildings = self.mesh()?;
        let cells = self.len()?;
        let mut modelled = Vec::new();
        for _ in 0..cells {
            modelled.push(BuildingCell {
                models: self.models()?,
                shells: self.mesh()?,
            });
        }
        let streets = self.mesh()?;
        let tracks = self.mesh()?;
        let urban = self.mesh()?;
        let water = self.mesh()?;
        let trees = Trees {
            models: self.models()?,
            grass: self.floats()?,
            flowers: self.floats()?,
        };
        let models = self.models()?;
        let count = self.len()?;
        let mut labels = Vec::new();
        for _ in 0..count {
            labels.push(Label {
                text: self.text()?,
                transform: self.one()?,
            });
        }
        Ok(TerrainChunk {
            center,
            mesh,
            buildings,
            modelled,
            streets,
            tracks,
            urban,
            water,
            trees,
            props: Props {
                models,
                labels,
                wires: self.mesh()?,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(seed: f32) -> MeshData {
        MeshData {
            vertices: vec![[seed, 1.0, 2.0], [3.0, seed, 5.0]],
            normals: vec![[0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
            uvs: vec![[0.5, seed]],
            colors: vec![[0.1, 0.2, 0.3, 1.0]],
            indices: vec![0, 1, 0, u32::MAX],
        }
    }

    /// A world with every field set apart from its default, so a field the format forgot
    /// comes back different.
    fn sample() -> World {
        let models = |name: &str| BTreeMap::from([(name.to_owned(), vec![1.5, -2.5, f32::MAX])]);
        let chunk = |seed: f32| TerrainChunk {
            center: [seed, 0.0, -seed],
            mesh: mesh(seed),
            buildings: mesh(seed + 1.0),
            modelled: vec![BuildingCell {
                models: models("house"),
                shells: mesh(seed + 2.0),
            }],
            streets: mesh(seed + 3.0),
            tracks: mesh(seed + 4.0),
            urban: mesh(seed + 9.0),
            water: mesh(seed + 5.0),
            trees: Trees {
                models: models("pine"),
                grass: vec![seed; 12],
                flowers: vec![-seed; 12],
            },
            props: Props {
                models: models("guard_rail"),
                labels: vec![Label {
                    text: "Prêles 12 %".to_owned(),
                    transform: [seed; 12],
                }],
                wires: mesh(seed + 6.0),
            },
        };
        World {
            chunks: vec![chunk(1.0), chunk(20.0)],
            road: mesh(30.0),
            structures: mesh(31.0),
            railways: mesh(32.0),
            horizon: Horizon {
                ground: mesh(33.0),
                water: mesh(34.0),
            },
            fallback_samples: 7,
        }
    }

    /// The generator version the samples are stored with.
    const BUILT_BY: u32 = 7;

    fn stored(world: &World) -> (Vec<u8>, Vec<Vec<u8>>) {
        let mut whole = Vec::new();
        write_whole(world, BUILT_BY, &mut whole).expect("written");
        let chunks = world
            .chunks
            .iter()
            .map(|chunk| {
                let mut bytes = Vec::new();
                write_chunk(chunk, &mut bytes).expect("written");
                bytes
            })
            .collect();
        (whole, chunks)
    }

    fn read_back((whole, chunks): &(Vec<u8>, Vec<Vec<u8>>)) -> World {
        let (mut world, generator) = read_whole(&mut whole.as_slice()).expect("read");
        assert_eq!(generator, BUILT_BY);
        for bytes in chunks {
            world
                .chunks
                .push(read_chunk(&mut bytes.as_slice()).expect("read"));
        }
        world
    }

    #[test]
    fn a_stored_world_reads_back_the_same_with_its_generator() {
        let world = sample();
        let stored = stored(&world);
        assert_eq!(read_back(&stored), world);
        assert_eq!(read_generator(&mut stored.0.as_slice()).unwrap(), BUILT_BY);
        let empty = World::default();
        assert_eq!(read_back(&self::stored(&empty)), empty);
    }

    #[test]
    fn a_stored_chunk_tells_its_centre_from_its_beginning() {
        let world = sample();
        for (chunk, bytes) in world.chunks.iter().zip(stored(&world).1) {
            // Its first bytes are enough: what follows is never read.
            let mut beginning = &bytes[..16];
            assert_eq!(read_center(&mut beginning).unwrap(), chunk.center);
        }
        assert!(read_center(&mut [3, 0, 0, 0].as_slice()).is_err());
    }

    /// The sample as this format stores it, kept in the repository: a world stored by an
    /// earlier build must still read back, until the format is raised.
    #[test]
    fn worlds_stored_before_read_back_until_the_format_is_raised() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/data/stored-world-{FORMAT}.bin"));
        let (whole, chunks) = stored(&sample());
        if std::env::var_os("ORQWA_WRITE_STORED_WORLD").is_some() {
            let mut bytes = Vec::new();
            for part in std::iter::once(&whole).chain(&chunks) {
                let len = u32::try_from(part.len()).unwrap();
                bytes.extend(len.to_le_bytes());
                bytes.extend(part);
            }
            std::fs::write(&file, bytes).unwrap();
        }
        let raise = "the stored layout changed: raise stored::FORMAT, then write the new sample \
            with ORQWA_WRITE_STORED_WORLD=1 cargo test -p orqwa-world stored";
        let bytes = std::fs::read(&file).unwrap_or_else(|_| panic!("{}: {raise}", file.display()));
        let mut parts = Vec::new();
        let mut rest = bytes.as_slice();
        while let [a, b, c, d, tail @ ..] = rest {
            let len = u32::from_le_bytes([*a, *b, *c, *d]) as usize;
            let (part, tail) = tail.split_at(len.min(tail.len()));
            parts.push(part.to_vec());
            rest = tail;
        }
        let read = || -> io::Result<World> {
            let (whole, chunks) = parts.split_first().ok_or(io::ErrorKind::UnexpectedEof)?;
            let (mut world, _) = read_whole(&mut whole.as_slice())?;
            for chunk in chunks {
                world.chunks.push(read_chunk(&mut chunk.as_slice())?);
            }
            Ok(world)
        };
        assert!(read().is_ok_and(|world| world == sample()), "{raise}");
    }

    #[test]
    fn a_cut_off_or_foreign_file_is_refused() {
        let (whole, chunks) = stored(&sample());
        let chunk = &chunks[0];
        for end in [
            0,
            5,
            MAGIC.len() + 2,
            MAGIC.len() + 6,
            whole.len() / 2,
            whole.len() - 1,
        ] {
            assert!(read_whole(&mut &whole[..end]).is_err(), "read {end} bytes");
        }
        for end in [0, 3, chunk.len() / 2, chunk.len() - 1] {
            assert!(read_chunk(&mut &chunk[..end]).is_err(), "read {end} bytes");
        }
        let longer = |bytes: &[u8]| [bytes, &[0]].concat();
        assert!(read_whole(&mut longer(&whole).as_slice()).is_err());
        assert!(read_chunk(&mut longer(chunk).as_slice()).is_err());
        let mut foreign = whole.clone();
        foreign[0] = b'X';
        let error = read_whole(&mut foreign.as_slice()).expect_err("refused");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        // Another format is refused before anything is read as this one.
        for format in [FORMAT - 1, FORMAT + 1] {
            let mut other = whole.clone();
            other[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&format.to_le_bytes());
            for error in [
                read_whole(&mut other.as_slice()).expect_err("refused"),
                read_generator(&mut other.as_slice()).expect_err("refused"),
            ] {
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            }
        }
    }

    #[test]
    fn a_damaged_length_fails_without_allocating_it() {
        // The chunk's centre claims four billion floats.
        let mut bytes = u32::MAX.to_le_bytes().to_vec();
        bytes.extend([0; 16]);
        assert!(read_chunk(&mut bytes.as_slice()).is_err());
    }
}
