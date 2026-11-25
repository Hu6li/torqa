//! `instances` files: the copies of a world's assets placed in a section
//! (`docs/world-files.md`, "Assets and instances files").

use std::io::{self, Read};

use super::{WorldError, invalid};

const MAGIC: &[u8; 4] = b"OQXI";
const VERSION: u32 = 1;
/// Floats of a copy's 3 × 4 matrix.
pub const MATRIX: usize = 12;
/// Floats of a copy's tint.
pub const TINT: usize = 4;

/// The copies of one asset in a section.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceGroup {
    /// The asset's index in the description's `assets`.
    pub asset: usize,
    /// Whether each copy carries a tint after its matrix.
    pub tinted: bool,
    /// Per copy, the 3 × 4 matrix row by row, then the tint if [`InstanceGroup::tinted`]: the
    /// layout of Godot's `MultiMesh` buffers with colours.
    pub floats: Vec<f32>,
}

impl InstanceGroup {
    /// How many copies the group places.
    #[must_use]
    pub fn count(&self) -> usize {
        self.floats.len() / (MATRIX + if self.tinted { TINT } else { 0 })
    }
}

fn u32_from(input: &mut impl Read, read: &mut u64) -> Result<u32, WorldError> {
    let mut bytes = [0; 4];
    input
        .read_exact(&mut bytes)
        .map_err(|_| invalid("cut short"))?;
    *read += 4;
    Ok(u32::from_le_bytes(bytes))
}

/// Reads an `instances` file for a world of `assets` assets, keeping the copies if `keep`;
/// returns them and how many bytes it read. Stops after the last group: whatever follows is
/// for the caller to refuse.
///
/// # Errors
/// [`WorldError::Invalid`] naming the rule broken.
pub fn read(
    input: &mut impl Read,
    assets: usize,
    keep: bool,
) -> Result<(Vec<InstanceGroup>, u64), WorldError> {
    let mut read = 0;
    let mut magic = [0; 4];
    input
        .read_exact(&mut magic)
        .map_err(|_| invalid("not an instances file"))?;
    read += 4;
    if &magic != MAGIC || u32_from(input, &mut read)? != VERSION {
        return Err(invalid("not an instances file of version 1"));
    }
    let groups = u32_from(input, &mut read)?;
    let mut kept = Vec::new();
    let mut buffer = vec![0u8; 64 * 1024];
    for _ in 0..groups {
        let asset = u32_from(input, &mut read)? as usize;
        let count = u64::from(u32_from(input, &mut read)?);
        let flags = u32_from(input, &mut read)?;
        if asset >= assets || flags > 1 {
            return Err(invalid("a group of an unknown asset or flags"));
        }
        let tinted = flags == 1;
        let per_copy = (MATRIX + if tinted { TINT } else { 0 }) as u64 * 4;
        let mut left = count * per_copy;
        let mut floats = Vec::new();
        if keep {
            // Grows as data arrives: the count is the file's claim, not a promise.
            floats.reserve(usize::try_from(left.min(1 << 24) / 4).unwrap_or(0));
        }
        while left > 0 {
            let chunk = usize::try_from(left).map_or(buffer.len(), |left| left.min(buffer.len()));
            input
                .read_exact(&mut buffer[..chunk])
                .map_err(|e| match e.kind() {
                    io::ErrorKind::UnexpectedEof => invalid("cut short"),
                    _ => WorldError::Io(e),
                })?;
            for bytes in buffer[..chunk].as_chunks::<4>().0 {
                let value = f32::from_le_bytes(*bytes);
                if !value.is_finite() {
                    return Err(invalid("a copy with a value that is not a number"));
                }
                if keep {
                    floats.push(value);
                }
            }
            left -= chunk as u64;
            read += chunk as u64;
        }
        if keep {
            kept.push(InstanceGroup {
                asset,
                tinted,
                floats,
            });
        }
    }
    Ok((kept, read))
}
