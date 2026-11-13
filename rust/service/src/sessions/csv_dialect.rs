//! Bounded CSV dialect selection compatible with the local Python source.
use std::collections::BTreeMap;

pub(super) struct Dialect {
    pub delimiter: u8,
    pub quote: u8,
    pub double_quote: bool,
    pub skip_initial_space: bool,
}
#[derive(Clone, Copy)]
struct Found {
    quote: char,
    delimiter: Option<char>,
    space: bool,
}
fn delimiter_candidate(ch: char) -> bool {
    !ch.is_alphanumeric() && !matches!(ch, '_' | '\n' | '\'' | '"')
}
fn closes(bytes: &[char], quote: char) -> Vec<Option<usize>> {
    let mut result = vec![None; bytes.len() + 2];
    for index in (0..bytes.len()).rev() {
        result[index] = if bytes[index] == quote {
            if bytes.get(index + 1) == Some(&quote) {
                result[index + 2]
            } else {
                Some(index)
            }
        } else {
            result[index + 1]
        };
    }
    result
}
fn quoted_matches(
    bytes: &[char],
    pattern: usize,
    single: &[Option<usize>],
    double: &[Option<usize>],
) -> Vec<Found> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let line_start = at == 0 || bytes[at - 1] == '\n';
        let mut delimiter = None;
        let mut space = false;
        let quote_at;
        if matches!(pattern, 0 | 2) {
            if !delimiter_candidate(bytes[at]) {
                at += 1;
                continue;
            }
            delimiter = Some(bytes[at]);
            let mut next = at + 1;
            if bytes.get(next) == Some(&' ') {
                space = true;
                next += 1;
            }
            quote_at = next;
        } else {
            if !line_start {
                at += 1;
                continue;
            }
            quote_at = at;
        }
        let Some(&quote @ ('\'' | '"')) = bytes.get(quote_at) else {
            at += 1;
            continue;
        };
        let close = if quote == '\'' {
            single[quote_at + 1]
        } else {
            double[quote_at + 1]
        };
        let Some(close) = close else {
            at += 1;
            continue;
        };
        let after = close + 1;
        let matched = match pattern {
            0 => bytes.get(after).copied() == delimiter,
            1 => {
                if bytes.get(after).copied().is_some_and(delimiter_candidate) {
                    delimiter = bytes.get(after).copied();
                    space = bytes.get(after + 1) == Some(&' ');
                    true
                } else {
                    false
                }
            }
            2 | 3 => after == bytes.len() || bytes.get(after) == Some(&'\n'),
            _ => false,
        };
        if matched {
            found.push(Found {
                quote,
                delimiter,
                space,
            });
            at = match pattern {
                0 => after + 1,
                1 => after + 1 + usize::from(space),
                _ => after,
            };
        } else {
            at += 1;
        }
    }
    found
}
fn most_frequent(values: impl Iterator<Item = char>) -> Option<char> {
    let mut counts = Vec::<(char, usize)>::new();
    for value in values {
        if let Some((_, count)) = counts.iter_mut().find(|(key, _)| *key == value) {
            *count += 1;
        } else {
            counts.push((value, 1));
        }
    }
    counts
        .into_iter()
        .fold(None, |best, item| {
            if best.is_none_or(|(_, count)| item.1 > count) {
                Some(item)
            } else {
                best
            }
        })
        .map(|(value, _)| value)
}
fn guess_delimiter(sample: &str) -> Option<(u8, bool)> {
    let lines: Vec<_> = sample.split('\n').filter(|line| !line.is_empty()).collect();
    if lines.is_empty() {
        return None;
    }
    let chunk = lines.len().min(10);
    let mut frequencies = [Vec::<(usize, usize)>::new(), Vec::<(usize, usize)>::new()];
    let mut delims = BTreeMap::<u8, ()>::new();
    let mut start = 0;
    let mut iteration = 0;
    while start < lines.len() {
        iteration += 1;
        let end = (start + chunk).min(lines.len());
        for line in &lines[start..end] {
            for (index, delimiter) in [b',', b'\t'].into_iter().enumerate() {
                let frequency = line.bytes().filter(|byte| *byte == delimiter).count();
                if let Some((_, count)) = frequencies[index]
                    .iter_mut()
                    .find(|(value, _)| *value == frequency)
                {
                    *count += 1;
                } else {
                    frequencies[index].push((frequency, 1));
                }
            }
        }
        let total = (chunk * iteration).min(lines.len()) as f64;
        let modes: Vec<_> = frequencies
            .iter()
            .map(|values| {
                let mode = values.iter().copied().fold(None, |best, item| {
                    if best.is_none_or(|(_, count)| item.1 > count) {
                        Some(item)
                    } else {
                        best
                    }
                });
                mode.map(|(frequency, count)| (frequency, 2 * count as isize - total as isize))
            })
            .collect();
        let mut threshold = 1.0;
        while delims.is_empty() && threshold >= 0.9 {
            for (index, mode) in modes.iter().enumerate() {
                if mode.is_some_and(|(frequency, count)| {
                    frequency > 0 && count > 0 && count as f64 / total >= threshold
                }) {
                    delims.insert([b',', b'\t'][index], ());
                }
            }
            threshold -= 0.01;
        }
        if delims.len() == 1 {
            let delimiter = *delims.keys().next()?;
            return Some((delimiter, skip_space(lines[0], delimiter)));
        }
        start = end;
    }
    let delimiter = [b',', b'\t']
        .into_iter()
        .find(|delimiter| delims.contains_key(delimiter))?;
    Some((delimiter, skip_space(lines[0], delimiter)))
}
fn skip_space(first: &str, delimiter: u8) -> bool {
    let bytes = first.as_bytes();
    let count = bytes.iter().filter(|byte| **byte == delimiter).count();
    count
        == bytes
            .windows(2)
            .filter(|pair| pair[0] == delimiter && pair[1] == b' ')
            .count()
}
fn has_doubled_field(
    bytes: &[char],
    delimiter: char,
    quote: char,
    skip: bool,
    closing: &[Option<usize>],
) -> bool {
    for start in 0..bytes.len() {
        if start != 0 && bytes[start - 1] != delimiter && bytes[start - 1] != '\n' {
            continue;
        }
        let mut first = start;
        if skip {
            while bytes.get(first) == Some(&' ') {
                first += 1;
            }
        }
        if bytes.get(first) != Some(&quote) {
            continue;
        }
        let Some(end) = closing[first + 1] else {
            continue;
        };
        if end + 1 != bytes.len() && bytes[end + 1] != delimiter && bytes[end + 1] != '\n' {
            continue;
        }
        if bytes[first + 1..end]
            .windows(2)
            .any(|pair| pair == [quote, quote])
        {
            return true;
        }
    }
    false
}
pub(super) fn sniff(source: &str) -> Option<Dialect> {
    let sample: String = source
        .chars()
        .take(4096)
        .collect::<String>()
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let chars: Vec<_> = sample.chars().collect();
    let single = closes(&chars, '\'');
    let double = closes(&chars, '"');
    let found = (0..4)
        .map(|pattern| quoted_matches(&chars, pattern, &single, &double))
        .find(|matches| !matches.is_empty())
        .unwrap_or_default();
    let quote = most_frequent(found.iter().map(|item| item.quote)).unwrap_or('"');
    let chosen = most_frequent(found.iter().filter_map(|item| {
        item.delimiter
            .filter(|delimiter| matches!(*delimiter, ',' | '\t'))
    }));
    let quoted_delimiter = chosen.is_some();
    let (delimiter, skip_initial_space) = if let Some(delimiter) = chosen {
        (
            delimiter,
            found
                .iter()
                .filter(|item| item.delimiter == Some(delimiter))
                .count()
                == found.iter().filter(|item| item.space).count(),
        )
    } else {
        let (delimiter, space) = guess_delimiter(&sample)?;
        (delimiter as char, space)
    };
    let double_quote = if !quoted_delimiter {
        false
    } else {
        has_doubled_field(
            &chars,
            delimiter,
            quote,
            skip_initial_space,
            if quote == '\'' { &single } else { &double },
        )
    };
    Some(Dialect {
        delimiter: delimiter as u8,
        quote: quote as u8,
        double_quote,
        skip_initial_space,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_python_sniffer_for_quotes_spacing_and_delimiters() {
        let cases = [
            ("a,b\n1,2", b',', b'"', false, false),
            ("a,b\n'v', x", b',', b'\'', false, true),
            ("a,b\n\"x\", 'y'", b',', b'"', false, true),
            ("\"a\",\"b\"\n\"x\",\"y\"", b',', b'"', false, false),
            ("a,b\n\"x\"\"y\",z", b',', b'"', true, false),
            ("a,b\n1, 2", b',', b'"', false, false),
            ("a\tb\n\"x\ty\"\tz", b'\t', b'"', false, false),
            ("a,b\nO'Reilly,ok", b',', b'"', false, false),
            ("a,b\n;\"x\"\"y\";,z", b',', b'"', false, false),
            ("  a, b\n  'x', 'y'", b',', b'\'', false, true),
        ];
        for (input, delimiter, quote, double, space) in cases {
            let dialect = sniff(input).unwrap();
            assert_eq!(
                (
                    dialect.delimiter,
                    dialect.quote,
                    dialect.double_quote,
                    dialect.skip_initial_space
                ),
                (delimiter, quote, double, space),
                "{input}"
            );
        }
        assert!(sniff("\"a\"\n\"b\"").is_none());
    }
}
