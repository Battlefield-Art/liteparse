//! Document-level PDF provenance metadata extraction.

use crate::types::{DocumentMetadata, PdfInput};
use pdfium::Document;
use std::io::{Read, Seek, SeekFrom};

const SCAN_BUFFER_BYTES: usize = 1 << 20;
const SCAN_OVERLAP: usize = 64;
const XMP_MAX_BYTES: usize = 64 * 1024;

pub(crate) fn extract(input: &PdfInput, document: &Document<'_>) -> DocumentMetadata {
    let mut metadata = match input {
        #[cfg(not(target_arch = "wasm32"))]
        PdfInput::Path(path) => std::fs::File::open(path)
            .ok()
            .map(|mut file| extract_raw_facts(&mut file))
            .unwrap_or_default(),
        PdfInput::Bytes(bytes) => extract_raw_facts(&mut std::io::Cursor::new(bytes)),
        #[cfg(target_arch = "wasm32")]
        PdfInput::Path(_) => DocumentMetadata::default(),
    };

    metadata.creation_date = document.meta_text("CreationDate");
    metadata.mod_date = document.meta_text("ModDate");
    metadata.file_version = document.file_version();
    let security_revision = document.security_handler_revision();
    metadata.is_encrypted = Some(security_revision != -1);
    if security_revision != -1 {
        metadata.security_handler_revision = Some(security_revision);
        metadata.permissions = Some(document.permissions());
    }
    let signatures = document.signature_summary(metadata.raw_file_size);
    metadata.signature_count = Some(signatures.count);
    metadata.signature_byte_range_reaches_eof = signatures.byte_range_reaches_eof;
    metadata
}

fn extract_raw_facts<R: Read + Seek>(reader: &mut R) -> DocumentMetadata {
    let mut metadata = DocumentMetadata::default();
    let file_size = reader.seek(SeekFrom::End(0)).ok();
    metadata.raw_file_size = file_size;
    if reader.seek(SeekFrom::Start(0)).is_err() {
        return metadata;
    }

    let mut buffer = vec![0u8; SCAN_BUFFER_BYTES];
    let mut carry = 0usize;
    let mut file_offset = 0u64;
    let mut eof_count = 0u32;
    let mut startxref_count = 0u32;
    let mut xmp_start = None;

    loop {
        let got = match reader.read(&mut buffer[carry..]) {
            Ok(got) => got,
            Err(_) => break,
        };
        let window_len = carry + got;
        if window_len == 0 {
            break;
        }
        let countable = if got > 0 && window_len > SCAN_OVERLAP {
            window_len - SCAN_OVERLAP
        } else {
            window_len
        };
        let window = &buffer[..window_len];
        eof_count = eof_count.saturating_add(count_occurrences_before(window, b"%%EOF", countable));
        startxref_count = startxref_count.saturating_add(count_occurrences_before(
            window,
            b"startxref",
            countable,
        ));
        if xmp_start.is_none()
            && let Some(offset) = find_bytes(window, b"<?xpacket begin")
        {
            xmp_start = Some(file_offset + offset as u64);
        }
        if got == 0 {
            break;
        }
        file_offset += countable as u64;
        carry = window_len - countable;
        buffer.copy_within(countable..window_len, 0);
    }

    metadata.eof_section_count = Some(eof_count);
    metadata.startxref_count = Some(startxref_count);

    if let Some(file_size) = file_size {
        let tail_start = file_size.saturating_sub(SCAN_BUFFER_BYTES as u64);
        if reader.seek(SeekFrom::Start(tail_start)).is_ok() {
            let mut tail = vec![0u8; (file_size - tail_start) as usize];
            if let Ok(got) = reader.read(&mut tail) {
                tail.truncate(got);
                metadata.trailer_id_pair_differs = trailer_id_pair_differs(&tail);
            }
        }
    }

    if let Some(xmp_start) = xmp_start
        && reader.seek(SeekFrom::Start(xmp_start)).is_ok()
    {
        let mut xmp = vec![0u8; XMP_MAX_BYTES];
        if let Ok(got) = reader.read(&mut xmp) {
            xmp.truncate(got);
            if let Some(packet_end) = find_bytes(&xmp, b"<?xpacket end") {
                let suffix = &xmp[packet_end..];
                let end = find_bytes(suffix, b"?>")
                    .map(|offset| packet_end + offset + 2)
                    .unwrap_or(packet_end + b"<?xpacket end".len());
                xmp.truncate(end.min(xmp.len()));
            }
            metadata.xmp = Some(String::from_utf8_lossy(&xmp).into_owned());
        }
    }

    metadata
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    (!needle.is_empty() && haystack.len() >= needle.len())
        .then(|| {
            haystack
                .windows(needle.len())
                .position(|part| part == needle)
        })
        .flatten()
}

fn count_occurrences_before(haystack: &[u8], needle: &[u8], start_limit: usize) -> u32 {
    let mut count = 0u32;
    let mut cursor = 0usize;
    while let Some(offset) = find_bytes(&haystack[cursor..], needle) {
        if cursor + offset >= start_limit {
            break;
        }
        count = count.saturating_add(1);
        cursor += offset + needle.len();
    }
    count
}

fn trailer_id_pair_differs(bytes: &[u8]) -> Option<bool> {
    let mut cursor = 0usize;
    let mut last_pair: Option<(&[u8], &[u8])> = None;
    while let Some(offset) = find_bytes(&bytes[cursor..], b"/ID") {
        let id_start = cursor + offset;
        let mut pos = id_start + 3;
        while bytes
            .get(pos)
            .is_some_and(|b| matches!(b, b' ' | b'\r' | b'\n' | b'\t'))
        {
            pos += 1;
        }
        if bytes.get(pos) == Some(&b'[') {
            pos += 1;
            let mut values = Vec::with_capacity(2);
            while pos < bytes.len() && bytes[pos] != b']' && values.len() < 2 {
                if bytes[pos] == b'<' {
                    let start = pos + 1;
                    if let Some(close) = bytes[start..].iter().position(|b| *b == b'>') {
                        values.push(&bytes[start..start + close]);
                        pos = start + close;
                    }
                }
                pos += 1;
            }
            if values.len() == 2 {
                last_pair = Some((values[0], values[1]));
            }
        }
        cursor = id_start + 3;
    }
    last_pair.map(|(first, second)| first != second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_raw_provenance_facts() {
        let pdf = b"%PDF-1.4\n/ID [<aaaa><bbbb>]\nstartxref\n1\n%%EOF\n\
                    update\n/ID [ <cccc> <dddd> ]\nstartxref\n2\n%%EOF\n\
                    <?xpacket begin='x'?><x:xmpmeta>ok</x:xmpmeta><?xpacket end='w'?>tail";
        let metadata = extract_raw_facts(&mut std::io::Cursor::new(pdf));
        assert_eq!(metadata.raw_file_size, Some(pdf.len() as u64));
        assert_eq!(metadata.eof_section_count, Some(2));
        assert_eq!(metadata.startxref_count, Some(2));
        assert_eq!(metadata.trailer_id_pair_differs, Some(true));
        assert_eq!(
            metadata.xmp.as_deref(),
            Some("<?xpacket begin='x'?><x:xmpmeta>ok</x:xmpmeta><?xpacket end='w'?>")
        );
    }

    #[test]
    fn trailer_id_uses_last_valid_pair() {
        assert_eq!(
            trailer_id_pair_differs(b"/ID [<aa><bb>] junk /ID [<cc><cc>]"),
            Some(false)
        );
        assert_eq!(trailer_id_pair_differs(b"no trailer id"), None);
    }

    #[test]
    fn finds_markers_that_cross_scan_chunks_without_double_counting() {
        let mut pdf = vec![b'x'; SCAN_BUFFER_BYTES - 7];
        pdf.extend_from_slice(b"startxref\n%%EOF\n<?xpacket begin='x'?>payload");
        let metadata = extract_raw_facts(&mut std::io::Cursor::new(pdf));
        assert_eq!(metadata.startxref_count, Some(1));
        assert_eq!(metadata.eof_section_count, Some(1));
        assert_eq!(
            metadata.xmp.as_deref(),
            Some("<?xpacket begin='x'?>payload")
        );
    }
}
