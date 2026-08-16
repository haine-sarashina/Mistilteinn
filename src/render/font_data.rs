//! Turning a downloaded `@font-face` file into something the shaper can load.
//!
//! The font stack reads sfnt containers — `.ttf`, `.otf` and collections — and
//! nothing else. A WOFF file is an sfnt whose tables have been zlib-compressed
//! and reordered, so it can be unpacked here rather than rejected.
//!
//! WOFF2 is not handled: it re-encodes the glyph outlines with its own
//! transform on top of Brotli, which is a decoder in its own right rather than
//! a container unwrap. Sources declaring `format("woff2")` are skipped, and a
//! page that offers only WOFF2 falls back to its generic family.

use std::io::Read;

/// `wOFF` — the WOFF 1.0 signature.
const WOFF_SIGNATURE: u32 = 0x774F_4646;
/// `wOF2` — WOFF 2.0, recognised so it can be reported rather than misparsed.
const WOFF2_SIGNATURE: u32 = 0x774F_4632;

const WOFF_HEADER_LEN: usize = 44;
const WOFF_TABLE_ENTRY_LEN: usize = 20;
const SFNT_HEADER_LEN: usize = 12;
const SFNT_TABLE_RECORD_LEN: usize = 16;

/// Why a downloaded font could not be used.
#[derive(Debug, PartialEq, Eq)]
pub enum FontDataError {
    /// A WOFF2 file, which needs a decoder we do not have.
    Woff2Unsupported,
    /// Not a font container we recognise at all.
    UnknownFormat,
    /// A WOFF file whose structure did not hold up.
    Malformed(&'static str),
}

impl std::fmt::Display for FontDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Woff2Unsupported => write!(f, "WOFF2 is not supported"),
            Self::UnknownFormat => write!(f, "unrecognised font container"),
            Self::Malformed(why) => write!(f, "malformed WOFF: {why}"),
        }
    }
}

/// Whether a `format(...)` hint names something worth downloading.
///
/// Used to pick between the sources of one `@font-face` rule before spending a
/// request on a file we could not decode anyway.
pub fn is_supported_format(format: Option<&str>) -> bool {
    match format {
        None => true,
        Some(f) => matches!(
            f,
            "truetype" | "opentype" | "woff" | "truetype-variations" | "opentype-variations"
        ),
    }
}

/// Read a big-endian `u32` at `offset`.
fn be_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Read a big-endian `u16` at `offset`.
fn be_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

/// Convert a downloaded font file into an sfnt the shaper can register.
///
/// An sfnt is returned untouched; a WOFF is unpacked into one.
pub fn to_sfnt(data: Vec<u8>) -> Result<Vec<u8>, FontDataError> {
    let signature = be_u32(&data, 0).ok_or(FontDataError::UnknownFormat)?;

    match signature {
        WOFF_SIGNATURE => decode_woff(&data),
        WOFF2_SIGNATURE => Err(FontDataError::Woff2Unsupported),
        // 0x00010000 (TrueType), 'OTTO' (CFF), 'true'/'ttcf' — all sfnt shaped.
        0x0001_0000 | 0x4F54_544F | 0x7472_7565 | 0x7474_6366 => Ok(data),
        _ => Err(FontDataError::UnknownFormat),
    }
}

/// Rebuild an sfnt from a WOFF 1.0 container.
///
/// WOFF keeps the same tables as the sfnt it wraps, each optionally
/// zlib-compressed, with its own directory. Reconstructing means writing a
/// fresh sfnt header and laying the decompressed tables out 4-byte aligned.
fn decode_woff(data: &[u8]) -> Result<Vec<u8>, FontDataError> {
    if data.len() < WOFF_HEADER_LEN {
        return Err(FontDataError::Malformed("header truncated"));
    }

    let flavor = be_u32(data, 4).ok_or(FontDataError::Malformed("no flavor"))?;
    let num_tables = be_u16(data, 12).ok_or(FontDataError::Malformed("no table count"))? as usize;
    if num_tables == 0 {
        return Err(FontDataError::Malformed("no tables"));
    }

    let mut tables: Vec<(u32, Vec<u8>)> = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let entry = WOFF_HEADER_LEN + i * WOFF_TABLE_ENTRY_LEN;
        let tag = be_u32(data, entry).ok_or(FontDataError::Malformed("directory truncated"))?;
        let offset = be_u32(data, entry + 4).ok_or(FontDataError::Malformed("no offset"))? as usize;
        let comp_len = be_u32(data, entry + 8)
            .ok_or(FontDataError::Malformed("no compressed length"))?
            as usize;
        let orig_len = be_u32(data, entry + 12)
            .ok_or(FontDataError::Malformed("no original length"))? as usize;

        let compressed = data
            .get(offset..offset.checked_add(comp_len).unwrap_or(usize::MAX))
            .ok_or(FontDataError::Malformed("table data out of range"))?;

        // A table is stored uncompressed when deflating it would not have
        // helped, which the spec signals by the two lengths being equal.
        let table = if comp_len == orig_len {
            compressed.to_vec()
        } else {
            let mut out = Vec::with_capacity(orig_len);
            flate2::read::ZlibDecoder::new(compressed)
                .read_to_end(&mut out)
                .map_err(|_| FontDataError::Malformed("table failed to inflate"))?;
            out
        };
        tables.push((tag, table));
    }

    // sfnt requires the table records in tag order.
    tables.sort_by_key(|(tag, _)| *tag);

    let mut out =
        Vec::with_capacity(SFNT_HEADER_LEN + num_tables * SFNT_TABLE_RECORD_LEN + data.len());

    // The binary search hints are derived from the table count; some rasterizers
    // read them, so they are written properly rather than zeroed.
    let entry_selector = (usize::BITS - 1 - num_tables.leading_zeros()) as u16;
    let search_range = (1u32 << entry_selector) as u16 * 16;
    let range_shift = (num_tables as u16) * 16 - search_range;

    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(num_tables as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    let mut table_offset = SFNT_HEADER_LEN + num_tables * SFNT_TABLE_RECORD_LEN;
    for (tag, table) in &tables {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&sfnt_checksum(table).to_be_bytes());
        out.extend_from_slice(&(table_offset as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        table_offset += table.len().next_multiple_of(4);
    }

    for (_, table) in &tables {
        out.extend_from_slice(table);
        out.resize(out.len().next_multiple_of(4), 0);
    }

    Ok(out)
}

/// The sfnt table checksum: the sum of the table's big-endian `u32` words,
/// with the tail zero-padded to a word boundary.
fn sfnt_checksum(table: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in table.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a minimal but structurally valid sfnt with the given tables.
    fn build_sfnt(tables: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut sorted = tables.to_vec();
        sorted.sort_by_key(|(tag, _)| *tag);
        let n = sorted.len();
        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&(n as u16).to_be_bytes());
        let entry_selector = (usize::BITS - 1 - n.leading_zeros()) as u16;
        let search_range = (1u32 << entry_selector) as u16 * 16;
        out.extend_from_slice(&search_range.to_be_bytes());
        out.extend_from_slice(&entry_selector.to_be_bytes());
        out.extend_from_slice(&((n as u16) * 16 - search_range).to_be_bytes());
        let mut offset = SFNT_HEADER_LEN + n * SFNT_TABLE_RECORD_LEN;
        for (tag, table) in &sorted {
            out.extend_from_slice(&tag.to_be_bytes());
            out.extend_from_slice(&sfnt_checksum(table).to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(table.len() as u32).to_be_bytes());
            offset += table.len().next_multiple_of(4);
        }
        for (_, table) in &sorted {
            out.extend_from_slice(table);
            out.resize(out.len().next_multiple_of(4), 0);
        }
        out
    }

    /// Wrap tables into a WOFF 1.0 container, compressing each one.
    fn build_woff(tables: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let n = tables.len();
        let mut directory = Vec::new();
        let mut body = Vec::new();
        let mut offset = WOFF_HEADER_LEN + n * WOFF_TABLE_ENTRY_LEN;

        for (tag, table) in tables {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(table).unwrap();
            let compressed = encoder.finish().unwrap();
            // Only claim compression when it actually shrank the table, which
            // is the signal the decoder keys off.
            let stored = compressed.len() >= table.len();
            let payload = if stored { table.clone() } else { compressed };

            directory.extend_from_slice(&tag.to_be_bytes());
            directory.extend_from_slice(&(offset as u32).to_be_bytes());
            directory.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            directory.extend_from_slice(&(table.len() as u32).to_be_bytes());
            directory.extend_from_slice(&sfnt_checksum(table).to_be_bytes());

            offset += payload.len().next_multiple_of(4);
            body.extend_from_slice(&payload);
            body.resize(body.len().next_multiple_of(4), 0);
        }

        let mut out = Vec::new();
        out.extend_from_slice(&WOFF_SIGNATURE.to_be_bytes());
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // flavor
        out.extend_from_slice(&0u32.to_be_bytes()); // length, unread
        out.extend_from_slice(&(n as u16).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // reserved
        out.extend_from_slice(&0u32.to_be_bytes()); // totalSfntSize, unread
        out.extend_from_slice(&[0u8; 4]); // major/minor version
        out.extend_from_slice(&[0u8; 12]); // meta block
        out.extend_from_slice(&[0u8; 8]); // private block
        assert_eq!(out.len(), WOFF_HEADER_LEN);
        out.extend_from_slice(&directory);
        out.extend_from_slice(&body);
        out
    }

    fn sample_tables() -> Vec<(u32, Vec<u8>)> {
        vec![
            // 'head' — repetitive so it compresses.
            (0x6865_6164, vec![0xAB; 64]),
            // 'cmap' — short enough that compression would grow it.
            (0x636D_6170, vec![1, 2, 3]),
        ]
    }

    #[test]
    fn an_sfnt_is_passed_through_untouched() {
        let sfnt = build_sfnt(&sample_tables());
        assert_eq!(to_sfnt(sfnt.clone()).unwrap(), sfnt);
    }

    #[test]
    fn a_woff_round_trips_back_to_its_sfnt() {
        let tables = sample_tables();
        let woff = build_woff(&tables);
        let rebuilt = to_sfnt(woff).expect("WOFF should decode");
        assert_eq!(
            rebuilt,
            build_sfnt(&tables),
            "the rebuilt sfnt must match the original byte for byte"
        );
    }

    #[test]
    fn woff_handles_both_stored_and_compressed_tables() {
        // The 3-byte table cannot be compressed profitably, so it is stored;
        // the 64-byte one is deflated. Both must come back intact.
        let tables = sample_tables();
        let rebuilt = to_sfnt(build_woff(&tables)).unwrap();
        assert!(
            rebuilt.windows(64).any(|w| w == [0xAB; 64]),
            "the compressed table was not restored"
        );
        assert!(
            rebuilt.windows(3).any(|w| w == [1, 2, 3]),
            "the stored table was not copied"
        );
    }

    #[test]
    fn woff2_is_reported_rather_than_misparsed() {
        let mut data = WOFF2_SIGNATURE.to_be_bytes().to_vec();
        data.extend_from_slice(&[0u8; 64]);
        assert_eq!(to_sfnt(data), Err(FontDataError::Woff2Unsupported));
    }

    #[test]
    fn junk_is_rejected() {
        assert_eq!(
            to_sfnt(b"not a font at all".to_vec()),
            Err(FontDataError::UnknownFormat)
        );
        assert_eq!(to_sfnt(vec![0, 1]), Err(FontDataError::UnknownFormat));
    }

    #[test]
    fn a_truncated_woff_directory_is_an_error() {
        let mut woff = build_woff(&sample_tables());
        woff.truncate(WOFF_HEADER_LEN + 4);
        assert!(matches!(to_sfnt(woff), Err(FontDataError::Malformed(_))));
    }

    #[test]
    fn format_hints_gate_what_is_worth_downloading() {
        assert!(is_supported_format(None));
        assert!(is_supported_format(Some("truetype")));
        assert!(is_supported_format(Some("woff")));
        assert!(!is_supported_format(Some("woff2")));
        assert!(!is_supported_format(Some("embedded-opentype")));
    }
}
