//! Display-name extraction from dropped files.

use crate::model::Kind;
use std::io::Read;
use std::path::Path;

pub fn stem(path: &Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Best display name for a dropped path.
pub fn display_name(path: &Path, kind: Kind) -> String {
    let name = match kind {
        Kind::Exe => crate::platform::exe_description(path),
        Kind::Lnk => lnk_name(path),
        Kind::Html => html_title(path),
        Kind::Bat => bat_title(path),
        Kind::Dir => path.file_name().map(|s| s.to_string_lossy().into_owned()),
        Kind::Url | Kind::File => None,
    };
    name.map(|s| clean(&s))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| stem(path).replace('_', " "))
}

fn clean(s: &str) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(64).collect()
}

fn read_head(path: &Path, max: usize) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::with_capacity(max.min(1 << 16));
    f.by_ref().take(max as u64).read_to_end(&mut buf).ok()?;
    Some(buf)
}

// ------------------------------------------------------------------ HTML

pub fn html_title(path: &Path) -> Option<String> {
    let bytes = read_head(path, 256 * 1024)?;
    let text = String::from_utf8_lossy(&bytes);
    title_from_html(&text)
}

pub fn title_from_html(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    // Skip <title> inside <svg> etc. by just taking the first real <title> tag.
    loop {
        let start = lower[from..].find("<title")? + from;
        let after = &lower[start + 6..];
        // must be "<title>" or "<title " (not "<titles")
        let next = after.chars().next()?;
        if next != '>' && !next.is_whitespace() {
            from = start + 6;
            continue;
        }
        let open_end = lower[start..].find('>')? + start + 1;
        let close = lower[open_end..].find("</title")? + open_end;
        let raw = &text[open_end..close];
        let t = decode_entities(raw.trim());
        return if t.trim().is_empty() { None } else { Some(t) };
    }
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // Search bytes, not a str slice: slicing at a fixed length can split a UTF-8 char.
        let Some(semi) = rest.as_bytes().iter().take(12).position(|&c| c == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..semi];
        let rep = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
            }
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match rep {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------------- BAT

/// Uses a `title Foo` line if the script sets one; otherwise None (-> file name).
pub fn bat_title(path: &Path) -> Option<String> {
    let bytes = read_head(path, 64 * 1024)?;
    title_from_bat(&String::from_utf8_lossy(&bytes))
}

pub fn title_from_bat(text: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim().trim_start_matches('@').trim();
        if l.len() > 6 && l.as_bytes()[..6].eq_ignore_ascii_case(b"title ") {
            // The 6 matched bytes are ASCII, so index 6 is a char boundary.
            let t = l[6..].trim();
            if !t.is_empty() && !t.contains('%') && !t.contains('!') {
                return Some(t.to_string());
            }
        }
    }
    None
}

// ------------------------------------------------------------------- LNK

#[derive(Debug, Default, PartialEq)]
pub struct LnkInfo {
    pub description: Option<String>,
    pub target: Option<String>,
}

/// Prefer the target program's own name; fall back to the shortcut's file name
/// (what Explorer shows), then the shortcut description.
pub fn lnk_name(path: &Path) -> Option<String> {
    let bytes = read_head(path, 1 << 20)?;
    let info = parse_lnk(&bytes)?;
    if let Some(t) = &info.target {
        let t = crate::platform::expand_env(t);
        let tp = Path::new(&t);
        if Kind::of(tp) == Kind::Exe && tp.exists() {
            if let Some(n) = crate::platform::exe_description(tp) {
                return Some(n);
            }
        }
    }
    let s = stem(path);
    if !s.is_empty() {
        return Some(s);
    }
    info.description
}

pub fn lnk_target(path: &Path) -> Option<String> {
    let bytes = read_head(path, 1 << 20)?;
    parse_lnk(&bytes)?.target.map(|t| crate::platform::expand_env(&t))
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn cstr_at(b: &[u8], o: usize) -> Option<String> {
    let s = b.get(o..)?;
    let end = s.iter().position(|&c| c == 0)?;
    Some(String::from_utf8_lossy(&s[..end]).into_owned())
}
fn wstr_at(b: &[u8], o: usize) -> Option<String> {
    let mut v = Vec::new();
    let mut i = o;
    loop {
        let c = u16_at(b, i)?;
        if c == 0 {
            break;
        }
        v.push(c);
        i += 2;
    }
    Some(String::from_utf16_lossy(&v))
}

/// Minimal MS-SHLLINK reader: description, LinkInfo local path, env-var target.
pub fn parse_lnk(b: &[u8]) -> Option<LnkInfo> {
    if u32_at(b, 0)? != 0x4C {
        return None;
    }
    let flags = u32_at(b, 0x14)?;
    let unicode = flags & 0x80 != 0;
    let mut o = 0x4C;
    if flags & 0x01 != 0 {
        o += 2 + u16_at(b, o)? as usize; // LinkTargetIDList
    }
    let mut out = LnkInfo::default();
    if flags & 0x02 != 0 {
        let size = u32_at(b, o)? as usize;
        let header = u32_at(b, o + 4)?;
        let li_flags = u32_at(b, o + 8)?;
        if li_flags & 1 != 0 {
            let base = if header >= 0x24 {
                u32_at(b, o + 0x1C).and_then(|off| wstr_at(b, o + off as usize))
            } else {
                None
            }
            .or_else(|| u32_at(b, o + 0x10).and_then(|off| cstr_at(b, o + off as usize)));
            let suffix = if header >= 0x24 {
                u32_at(b, o + 0x20).and_then(|off| wstr_at(b, o + off as usize))
            } else {
                None
            }
            .or_else(|| u32_at(b, o + 0x18).and_then(|off| cstr_at(b, o + off as usize)));
            if let Some(mut p) = base.filter(|s| !s.is_empty()) {
                if let Some(sfx) = suffix.filter(|s| !s.is_empty()) {
                    if !p.ends_with('\\') {
                        p.push('\\');
                    }
                    p.push_str(&sfx);
                }
                out.target = Some(p);
            }
        }
        o += size;
    }
    // StringData: NAME, RELATIVE_PATH, WORKING_DIR, ARGUMENTS, ICON_LOCATION
    let mut strings: [Option<String>; 5] = Default::default();
    for (i, bit) in [0x04u32, 0x08, 0x10, 0x20, 0x40].iter().enumerate() {
        if flags & bit != 0 {
            let n = u16_at(b, o)? as usize;
            o += 2;
            let s = if unicode {
                let raw = b.get(o..o + n * 2)?;
                let w: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                o += n * 2;
                String::from_utf16_lossy(&w)
            } else {
                let raw = b.get(o..o + n)?;
                o += n;
                String::from_utf8_lossy(raw).into_owned()
            };
            strings[i] = Some(s);
        }
    }
    out.description = strings[0].take().filter(|s| !s.trim().is_empty());
    // ExtraData blocks: look for EnvironmentVariableDataBlock (0xA0000001).
    if out.target.is_none() {
        loop {
            let size = u32_at(b, o).unwrap_or(0) as usize;
            if size < 4 {
                break;
            }
            if u32_at(b, o + 4) == Some(0xA000_0001) && size >= 0x314 {
                let w = wstr_at(b, o + 8 + 260).filter(|s| !s.is_empty());
                out.target = w.or_else(|| cstr_at(b, o + 8));
                break;
            }
            o += size;
        }
    }
    if out.target.is_none() {
        // Fall back to a relative path if it is all we have.
        out.target = strings[1].take().filter(|s| !s.is_empty());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html() {
        assert_eq!(
            title_from_html("<html><head><TITLE lang=en> Ink &amp; Grid &#8212; v2 </title>"),
            Some("Ink & Grid — v2".into())
        );
        assert_eq!(title_from_html("<titles>x</titles><title>Y</title>"), Some("Y".into()));
        assert_eq!(title_from_html("<title></title>"), None);
        assert_eq!(title_from_html("no title"), None);
    }

    #[test]
    fn bat() {
        assert_eq!(title_from_bat("@echo off\r\n@title Inkgrid Server\r\n"), Some("Inkgrid Server".into()));
        assert_eq!(title_from_bat("title %APP%"), None);
        assert_eq!(title_from_bat("echo hi"), None);
    }

    /// Audit: none of the parsers may panic on arbitrary bytes (release builds abort on panic).
    #[test]
    fn fuzz_no_panic() {
        let mut x: u64 = 0x1234_5678_9ABC_DEF1;
        let mut rnd = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let seeds: [&[u8]; 6] = [
            b"<title>a&amp;b&#x41;&#65;&bogus;&</title>",
            "tïtle é\r\n@tïtlé x\ntitle ñandú ✓\n".as_bytes(),
            "&é;&#é;&#xé;<title é>ü</title>".as_bytes(),
            b"L\0\0\0",
            "<TITLE>\u{1F600}&\u{1F600};</TITLE>".as_bytes(),
            b"title ",
        ];
        for i in 0..200_000u32 {
            let mut b: Vec<u8> = if i % 3 == 0 {
                (0..(rnd() % 400) as usize).map(|_| rnd() as u8).collect()
            } else {
                let mut v = seeds[(rnd() % 6) as usize].to_vec();
                for _ in 0..(rnd() % 6) {
                    if v.is_empty() {
                        break;
                    }
                    let k = (rnd() as usize) % v.len();
                    match rnd() % 3 {
                        0 => v[k] = rnd() as u8,
                        1 => {
                            v.remove(k);
                        }
                        _ => v.insert(k, rnd() as u8),
                    }
                }
                v
            };
            if i % 5 == 0 && b.len() >= 0x18 {
                b[0] = 0x4C;
                b[1] = 0;
                b[2] = 0;
                b[3] = 0;
            }
            let t = String::from_utf8_lossy(&b);
            let _ = title_from_html(&t);
            let _ = title_from_bat(&t);
            let _ = parse_lnk(&b);
        }
    }

    #[test]
    fn lnk_roundtrip() {
        // Build a tiny unicode .lnk: header + LinkInfo(local path) + NAME.
        let mut b = vec![0u8; 0x4C];
        b[0] = 0x4C;
        let flags: u32 = 0x02 | 0x04 | 0x80;
        b[0x14..0x18].copy_from_slice(&flags.to_le_bytes());
        let path = b"C:\\Tools\\thing.exe\0";
        let li_header = 0x1Cu32;
        let li_size = li_header as usize + path.len() + 1;
        let mut li = vec![0u8; li_size];
        li[0..4].copy_from_slice(&(li_size as u32).to_le_bytes());
        li[4..8].copy_from_slice(&li_header.to_le_bytes());
        li[8..12].copy_from_slice(&1u32.to_le_bytes());
        li[0x10..0x14].copy_from_slice(&li_header.to_le_bytes());
        li[li_header as usize..li_header as usize + path.len()].copy_from_slice(path);
        li[0x18..0x1C].copy_from_slice(&((li_size - 1) as u32).to_le_bytes());
        b.extend_from_slice(&li);
        let name: Vec<u16> = "My Tool".encode_utf16().collect();
        b.extend_from_slice(&(name.len() as u16).to_le_bytes());
        for c in name {
            b.extend_from_slice(&c.to_le_bytes());
        }
        b.extend_from_slice(&0u32.to_le_bytes());
        let info = parse_lnk(&b).unwrap();
        assert_eq!(info.target.as_deref(), Some("C:\\Tools\\thing.exe"));
        assert_eq!(info.description.as_deref(), Some("My Tool"));
    }
}
