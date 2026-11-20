//! The trailer of a world file's archive, checked before the zip reader allocates for the entries
//! it claims. World files are large, so ZIP64 is allowed, unlike in course files.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use super::{MAX_BYTES, MAX_ENTRIES, WorldError, invalid};

/// Room for the central directory: 200,000 entries with long names and ZIP64 fields.
const MAX_DIRECTORY_BYTES: u64 = 128 * 1024 * 1024;
const END: usize = 22;

fn short(bytes: &[u8], at: usize) -> u64 {
    u64::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

fn word(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32::from_le_bytes(
        bytes[at..at + 4].try_into().expect("four bytes"),
    ))
}

fn long(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

/// The number of entries the archive's trailer declares, after checking that the archive lies
/// on one disk, its directory is of a sane size and sits where the trailer says.
pub(super) fn entries(file: &mut File) -> Result<u64, WorldError> {
    let refused = || invalid("not a world file archive, or one beyond the format's limits");
    let size = file.metadata()?.len();
    if !(END as u64..=MAX_BYTES).contains(&size) {
        return Err(refused());
    }
    let length = usize::try_from(size.min((END + 65_535) as u64)).map_err(|_| refused())?;
    file.seek(SeekFrom::Start(size - length as u64))?;
    let mut tail = vec![0; length];
    file.read_exact(&mut tail)?;
    let offset = (0..=length - END)
        .rev()
        .find(|&i| {
            tail[i..].starts_with(b"PK\x05\x06")
                && i + END + usize::try_from(short(&tail, i + 20)).unwrap_or(usize::MAX) == length
        })
        .ok_or_else(refused)?;
    let end_at = size - length as u64 + offset as u64;
    let end = &tail[offset..];
    let (mut disk, mut directory_disk) = (short(end, 4), short(end, 6));
    let (mut here, mut count) = (short(end, 8), short(end, 10));
    let (mut directory_bytes, mut directory_at) = (word(end, 12), word(end, 16));
    let mut directory_end = end_at;
    // A ZIP64 locator sits just before the trailer; when present its record is authoritative.
    if end_at >= 20 {
        file.seek(SeekFrom::Start(end_at - 20))?;
        let mut locator = [0; 20];
        file.read_exact(&mut locator)?;
        if locator.starts_with(b"PK\x06\x07") {
            let record_at = long(&locator, 8);
            if word(&locator, 4) != 0 || word(&locator, 16) != 1 || record_at + 56 > end_at - 20 {
                return Err(refused());
            }
            file.seek(SeekFrom::Start(record_at))?;
            let mut record = [0; 56];
            file.read_exact(&mut record)?;
            if !record.starts_with(b"PK\x06\x06") {
                return Err(refused());
            }
            disk = word(&record, 16);
            directory_disk = word(&record, 20);
            here = long(&record, 24);
            count = long(&record, 32);
            directory_bytes = long(&record, 40);
            directory_at = long(&record, 48);
            directory_end = record_at;
        }
    }
    if disk != 0
        || directory_disk != 0
        || here != count
        || !(1..=MAX_ENTRIES).contains(&count)
        || directory_bytes > MAX_DIRECTORY_BYTES
        || directory_at.checked_add(directory_bytes) != Some(directory_end)
    {
        return Err(refused());
    }
    file.rewind()?;
    Ok(count)
}
