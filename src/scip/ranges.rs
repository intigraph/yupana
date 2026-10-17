//! Checked coordinate normalization; no guessed encoding or clamped ranges.
use super::Range;
use anyhow::{bail, ensure, Result};

pub(super) fn range(raw: &[i32], text: &str, encoding: i32) -> Result<Range> {
    let (sl, sc, el, ec) = match *raw {
        [line, start, end] => (line, start, line, end),
        [sl, sc, el, ec] => (sl, sc, el, ec),
        _ => bail!("malformed occurrence range"),
    };
    ensure!([sl, sc, el, ec].iter().all(|n| *n >= 0), "negative range");
    let (sl, sc, el, ec) = (
        usize::try_from(sl)?,
        usize::try_from(sc)?,
        usize::try_from(el)?,
        usize::try_from(ec)?,
    );
    let lines: Vec<_> = text.split('\n').collect();
    let start = byte(
        lines
            .get(sl)
            .ok_or_else(|| anyhow::anyhow!("start line outside source"))?,
        sc,
        encoding,
    )?;
    let end = byte(
        lines
            .get(el)
            .ok_or_else(|| anyhow::anyhow!("end line outside source"))?,
        ec,
        encoding,
    )?;
    ensure!(
        (sl, start) < (el, end),
        "empty or reversed identifier range"
    );
    Ok(Range {
        start_line: sl,
        start_byte: start,
        end_line: el,
        end_byte: end,
    })
}

fn byte(line: &str, offset: usize, encoding: i32) -> Result<usize> {
    if encoding == 1 {
        ensure!(
            offset <= line.len() && line.is_char_boundary(offset),
            "invalid UTF-8 byte offset"
        );
        return Ok(offset);
    }
    ensure!(
        encoding == 2 || encoding == 3,
        "unsupported position encoding"
    );
    let mut units = 0;
    for (at, ch) in line.char_indices() {
        if units == offset {
            return Ok(at);
        }
        units += if encoding == 2 { ch.len_utf16() } else { 1 };
    }
    ensure!(units == offset, "invalid character offset");
    Ok(line.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    // Exhaustive bounded property: all nonempty character-boundary slices in
    // mixed-width strings normalize identically from all three SCIP encodings.
    #[test]
    fn every_character_boundary_normalizes_to_the_same_byte_range() {
        for text in ["a🚀é中z", "", "ASCII", "🚀🚀", "é\r"] {
            let offsets: Vec<_> = text
                .char_indices()
                .map(|(n, _)| n)
                .chain(std::iter::once(text.len()))
                .collect();
            for &start in &offsets {
                for &end in offsets.iter().filter(|&&end| end > start) {
                    for encoding in 1..=3 {
                        let count = |prefix: &str| {
                            i32::try_from(match encoding {
                                1 => prefix.len(),
                                2 => prefix.encode_utf16().count(),
                                _ => prefix.chars().count(),
                            })
                            .unwrap()
                        };
                        let result = range(
                            &[0, count(&text[..start]), count(&text[..end])],
                            text,
                            encoding,
                        )
                        .unwrap();
                        assert_eq!((result.start_byte, result.end_byte), (start, end));
                    }
                }
            }
        }
    }

    #[test]
    fn unicode_encodings_and_invalid_ranges() {
        assert_eq!(range(&[0, 3, 4], "🚀 x", 2).unwrap().start_byte, 5);
        assert_eq!(range(&[0, 2, 3], "🚀 x", 3).unwrap().start_byte, 5);
        assert!(range(&[0, 1, 2], "🚀 x", 2).is_err());
        assert!(range(&[0, 1, 2], "🚀 x", 1).is_err());
        assert!(range(&[0, 0, 1], "x", 0).is_err());
        assert!(range(&[0, 1, 0], "x", 1).is_err());
        assert!(range(&[0, 0], "x", 1).is_err());
    }
}
