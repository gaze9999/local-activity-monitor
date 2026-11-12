mod commands;
mod database_path;
mod invocations;
#[cfg(feature = "sqlite-python")]
mod sqlite;

pub use commands::CommandMetadata;
pub use database_path::database_path;
pub use invocations::{nested_invocations, Invocation, Nested, INVOCATION_LIMIT};
#[cfg(feature = "sqlite-python")]
pub use sqlite::{project_diagnostic, SqlDiagnostic, SqlMetadata};

use std::ops::Range;

pub const LINE_LIMIT: usize = 1024 * 1024;

pub struct Forward {
    pub skipped: usize,
    pub lines: Range<usize>,
    pub tail: Range<usize>,
    pub discard: bool,
}

pub fn forward(data: &[u8], mut discard: bool, limit: usize) -> Forward {
    let end = data
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let skipped = if discard && end > 0 {
        discard = false;
        prefix_width(data)
    } else {
        0
    };
    let tail = if data.len() - end > limit {
        discard = true;
        end..end
    } else {
        end..data.len()
    };
    Forward {
        skipped,
        lines: skipped..end,
        tail,
        discard,
    }
}

// An unterminated first fragment has a virtual LF for the backfill cursor.
pub fn prefix_width(data: &[u8]) -> usize {
    data.iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(data.len())
        + 1
}

pub fn next_line<'a>(
    data: &'a [u8],
    remaining: &mut Range<usize>,
    limit: usize,
    reverse: bool,
) -> Option<(usize, &'a [u8])> {
    while remaining.start < remaining.end {
        let chunk = &data[remaining.clone()];
        let (offset, line) = if reverse {
            let width = chunk
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |index| index + 1);
            let offset = remaining.start + width;
            remaining.end = remaining.start + width.saturating_sub(1);
            (offset, &chunk[width..])
        } else {
            let offset = remaining.start;
            let width = chunk
                .iter()
                .position(|byte| *byte == b'\n')
                .unwrap_or(chunk.len());
            remaining.start += width + 1;
            (offset, &chunk[..width])
        };
        if !line.is_empty() && line.len() <= limit {
            return Some((offset, line));
        }
    }
    None
}

/// Framing of one bounded Codex Desktop diagnostic line.
pub struct DesktopLine<'a> {
    pub timestamp: &'a str,
    pub level: &'a str,
    pub module: &'a str,
    pub body: &'a str,
}

/// Split the source framing without retaining or interpreting the diagnostic body.
pub fn desktop_line(line: &str) -> Option<DesktopLine<'_>> {
    if line.chars().count() > 65_536 {
        return None;
    }
    let (timestamp, remainder) = line.split_once(' ')?;
    if !(20..=40).contains(&timestamp.chars().count()) || timestamp.chars().any(char::is_whitespace)
    {
        return None;
    }
    let (level, remainder) = remainder.split_once(' ')?;
    if !(1..=16).contains(&level.len()) || !level.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let (module, body) = remainder.strip_prefix('[')?.split_once("] ")?;
    if !(1..=160).contains(&module.len())
        || !module
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
    {
        return None;
    }
    Some(DesktopLine {
        timestamp,
        level,
        module,
        body,
    })
}

#[cfg(test)]
mod desktop_line_tests {
    use super::desktop_line;

    #[test]
    fn desktop_framing_is_bounded_and_rejects_invalid_headers() {
        let line = desktop_line("2026-11-12T01:02:03.123Z WARN [codex.mcp] private").unwrap();
        assert_eq!(
            (line.level, line.module, line.body),
            ("WARN", "codex.mcp", "private")
        );
        assert!(desktop_line("2026-11-12T01:02:03Z WARN [bad/path] text").is_none());
        assert!(desktop_line(&format!(
            "2026-11-12T01:02:03Z WARN [codex] {}",
            "x".repeat(65_536)
        ))
        .is_none());
        assert!(desktop_line("not-a-date WARN [codex] text").is_none());
    }
}

/// Incremental framing for a single Desktop log file.
#[derive(Default)]
pub struct DesktopStream {
    offset: u64,
    tail: Vec<u8>,
    discard: bool,
    skipped: usize,
    oversized: usize,
}

impl DesktopStream {
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Reset after truncation or file replacement.
    pub fn reset(&mut self, offset: u64) {
        self.offset = offset;
        self.tail.clear();
        self.discard = offset != 0;
        self.skipped = 0;
        self.oversized = 0;
    }

    pub fn oversized(&self) -> usize {
        self.oversized
    }

    pub fn pending_bytes(&self) -> usize {
        self.tail.len()
    }

    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Process a bounded read and return complete lines with source byte offsets.
    pub fn push(&mut self, bytes: &[u8]) -> Option<Vec<(u64, String)>> {
        if bytes.len() > 1024 * 1024 {
            return None;
        }
        let next_offset = self.offset.checked_add(bytes.len() as u64)?;
        let base = self.offset.saturating_sub(self.tail.len() as u64);
        self.offset = next_offset;
        self.tail.extend_from_slice(bytes);
        let was_discarding = self.discard;
        let framing = forward(&self.tail, self.discard, 65_536);
        self.oversized = usize::from(
            self.tail.len() - framing.lines.end > 65_536
                && (!was_discarding || framing.skipped > 0),
        );
        self.discard = framing.discard;
        self.skipped = framing.skipped;
        let mut lines = Vec::new();
        let mut position = framing.lines.start;
        for line in self.tail[framing.lines.clone()].split_inclusive(|byte| *byte == b'\n') {
            let value = line.strip_suffix(b"\n").unwrap_or(line);
            if value.len() > 65_536 {
                self.oversized += 1;
            } else {
                lines.push((
                    base + position as u64,
                    String::from_utf8_lossy(value).into_owned(),
                ));
            }
            position += line.len();
        }
        self.tail = self.tail[framing.tail].to_vec();
        Some(lines)
    }
}

#[cfg(test)]
mod desktop_stream_tests {
    use super::DesktopStream;

    #[test]
    fn oversized_read_is_rejected_without_advancing_state() {
        let mut stream = DesktopStream::default();
        assert!(stream.push(&vec![b'x'; 1024 * 1024 + 1]).is_none());
        assert_eq!(stream.offset(), 0);
        assert_eq!(stream.push(b"ok\n").unwrap(), vec![(0, "ok".into())]);
    }

    #[test]
    fn oversized_line_is_counted_once_across_chunks() {
        let mut stream = DesktopStream::default();
        stream.push(&vec![b'x'; 65_537]).unwrap();
        assert_eq!(stream.oversized(), 1);
        stream.push(&vec![b'x'; 65_537]).unwrap();
        assert_eq!(stream.oversized(), 0);
        stream.push(b"end\n\n").unwrap();
        assert_eq!(stream.oversized(), 0);
    }

    #[test]
    fn replacement_characters_preserve_line_boundary_and_offsets() {
        let mut stream = DesktopStream::default();
        assert!(stream.push(b"a\xff").unwrap().is_empty());
        let lines = stream.push(b"b\nnext\n").unwrap();
        assert_eq!(lines, vec![(0, "a\u{fffd}b".into()), (4, "next".into())]);
    }

    #[test]
    fn incremental_lines_keep_offsets_and_skip_partial_replacement() {
        let mut stream = DesktopStream::default();
        assert!(stream.push(b"first").unwrap().is_empty());
        assert_eq!(
            stream.push(b" line\nnext\n").unwrap(),
            vec![(0, "first line".into()), (11, "next".into())]
        );
        stream.reset(100);
        assert_eq!(
            stream.push(b"partial\ncomplete\n").unwrap(),
            vec![(108, "complete".into())]
        );
        assert_eq!(stream.offset(), 117);
        stream.reset(0);
        assert!(stream.push(&vec![b'x'; 65_537]).unwrap().is_empty());
        assert_eq!(
            stream.push(b"tail\nok\n").unwrap(),
            vec![(65_542, "ok".into())]
        );
    }
}
