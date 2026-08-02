//! dimi-exec fs module — `hostFsService.ts` mirror (M2, slice 2).
//!
//! Synchronous `std::fs` implementations of every `IHostFileSystem` method,
//! with Node `node:fs/promises` parity: `readText` without options decodes
//! utf-8 leniently and KEEPS the BOM (Node `readFile(path, 'utf8')`), with
//! options it goes through `decodeTextWithErrors` (TextDecoder semantics:
//! BOM stripped, `strict` throws). `readLines` streams 64KB chunks for
//! utf-8 and materializes the whole file for other encodings.
//!
//! Errors carry the raw errno so the bridge can emit a symbolic name
//! (`ENOENT`, `EACCES`, …) that the TS adapter maps through the existing
//! `toHostFsError` table.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};

/// Node `READ_CHUNK_SIZE` — 64KB streaming blocks.
const READ_CHUNK_SIZE: usize = 64 * 1024;

/// `HostFsError` boundary error: raw errno + context. The bridge formats
/// `"{ERRNO} {op} failed: {message}"`; the TS adapter re-reads the errno
/// symbol and maps it to an `os.fs.*` code.
#[derive(Debug)]
pub struct FsError {
    pub path: String,
    pub op: &'static str,
    pub errno: Option<i32>,
    pub message: String,
}

impl FsError {
    pub fn from_io(path: &str, op: &'static str, error: std::io::Error) -> Self {
        Self {
            path: path.to_owned(),
            op,
            errno: error.raw_os_error(),
            message: error.to_string(),
        }
    }

    /// Non-errno failure (decode errors, …) — maps to `os.fs.unknown`.
    pub fn unknown(path: &str, op: &'static str, message: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            op,
            errno: None,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} failed: {}",
            errno_name(self.errno),
            self.op,
            self.message
        )
    }
}

impl std::error::Error for FsError {}

/// errno → symbolic name, mirroring the Node `ErrnoException.code` strings
/// the TS `toHostFsError` table understands. Unknown errnos fall back to a
/// numeric string (Node uses `UNKNOWN` there; the adapter maps anything
/// unrecognized to `os.fs.unknown` anyway).
pub fn errno_name(errno: Option<i32>) -> &'static str {
    #[cfg(unix)]
    {
        match errno {
            Some(libc::ENOENT) => "ENOENT",
            Some(libc::EISDIR) => "EISDIR",
            Some(libc::ENOTDIR) => "ENOTDIR",
            Some(libc::EEXIST) => "EEXIST",
            Some(libc::EACCES) => "EACCES",
            Some(libc::EPERM) => "EPERM",
            Some(libc::ENOTEMPTY) => "ENOTEMPTY",
            Some(libc::EINVAL) => "EINVAL",
            Some(libc::EBADF) => "EBADF",
            Some(libc::EMFILE) => "EMFILE",
            Some(libc::ENFILE) => "ENFILE",
            Some(libc::ENOSPC) => "ENOSPC",
            Some(libc::EROFS) => "EROFS",
            Some(libc::ELOOP) => "ELOOP",
            _ => "UNKNOWN",
        }
    }
    #[cfg(not(unix))]
    {
        match errno {
            Some(code) => Box::leak(code.to_string().into_boxed_str()),
            None => "UNKNOWN",
        }
    }
}

/// `TextDecodeErrors` — Python-compatible `errors=` handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeErrors {
    Strict,
    Replace,
    Ignore,
}

impl DecodeErrors {
    pub fn parse(value: &str) -> Self {
        match value {
            "strict" => Self::Strict,
            "ignore" => Self::Ignore,
            _ => Self::Replace,
        }
    }
}

/// `BufferEncoding` — the subset with distinct decode semantics. Anything
/// else falls back to Node `Buffer.toString(encoding)` behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Other(&'static str),
}

impl Encoding {
    pub fn parse(value: &str) -> Self {
        match value {
            "utf-8" | "utf8" => Self::Utf8,
            "utf16le" | "ucs2" | "ucs-2" => Self::Utf16Le,
            other => Self::Other(Box::leak(other.to_owned().into_boxed_str())),
        }
    }

    pub fn is_utf8(&self) -> bool {
        matches!(self, Self::Utf8)
    }
}

/// Strip a UTF-8 BOM (`EF BB BF`).
fn strip_utf8_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(data)
}

/// Strip a UTF-16LE BOM (`FF FE`). Node's TextDecoder for utf-16le also
/// strips `FE FF` (big-endian) but decodes it as LE; keep it simple and
/// strip only the LE BOM, matching TextDecoder's observable output.
fn strip_utf16le_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(&[0xff, 0xfe]).unwrap_or(data)
}

/// `decodeUtf8Ignore` (decodeText.ts) — consume valid sequences, skip
/// invalid bytes, never produce U+FFFD.
fn decode_utf8_ignore(data: &[u8]) -> String {
    let mut output = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        if b0 <= 0x7f {
            output.push(b0 as char);
            i += 1;
            continue;
        }
        if (0xc2..=0xdf).contains(&b0) {
            if let Some(&b1) = data.get(i + 1) {
                if (0x80..=0xbf).contains(&b1) {
                    let code = ((b0 & 0x1f) as u32) << 6 | (b1 & 0x3f) as u32;
                    output.push(char::from_u32(code).unwrap());
                    i += 2;
                    continue;
                }
            }
            i += 1;
            continue;
        }
        if (0xe0..=0xef).contains(&b0) {
            if let (Some(&b1), Some(&b2)) = (data.get(i + 1), data.get(i + 2)) {
                let valid_second = (b0 == 0xe0 && (0xa0..=0xbf).contains(&b1))
                    || ((0xe1..=0xec).contains(&b0) && (0x80..=0xbf).contains(&b1))
                    || (b0 == 0xed && (0x80..=0x9f).contains(&b1))
                    || ((0xee..=0xef).contains(&b0) && (0x80..=0xbf).contains(&b1));
                if valid_second && (0x80..=0xbf).contains(&b2) {
                    let code =
                        ((b0 & 0x0f) as u32) << 12 | ((b1 & 0x3f) as u32) << 6 | (b2 & 0x3f) as u32;
                    output.push(char::from_u32(code).unwrap());
                    i += 3;
                    continue;
                }
            }
            i += 1;
            continue;
        }
        if (0xf0..=0xf4).contains(&b0) {
            if let (Some(&b1), Some(&b2), Some(&b3)) =
                (data.get(i + 1), data.get(i + 2), data.get(i + 3))
            {
                let valid_second = (b0 == 0xf0 && (0x90..=0xbf).contains(&b1))
                    || ((0xf1..=0xf3).contains(&b0) && (0x80..=0xbf).contains(&b1))
                    || (b0 == 0xf4 && (0x80..=0x8f).contains(&b1));
                if valid_second && (0x80..=0xbf).contains(&b2) && (0x80..=0xbf).contains(&b3) {
                    let code = ((b0 & 0x07) as u32) << 18
                        | ((b1 & 0x3f) as u32) << 12
                        | ((b2 & 0x3f) as u32) << 6
                        | (b3 & 0x3f) as u32;
                    output.push(char::from_u32(code).unwrap());
                    i += 4;
                    continue;
                }
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    output
}

/// `decodeUtf16LeIgnore` (decodeText.ts) — consume valid surrogate pairs,
/// skip lone surrogates.
fn decode_utf16le_ignore(data: &[u8]) -> String {
    let mut output = String::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let first = data[i];
        let second = data[i + 1];
        let code_unit = (first as u32) | ((second as u32) << 8);
        if (0xd800..=0xdbff).contains(&code_unit) {
            if let (Some(&low_first), Some(&low_second)) = (data.get(i + 2), data.get(i + 3)) {
                let low = (low_first as u32) | ((low_second as u32) << 8);
                if (0xdc00..=0xdfff).contains(&low) {
                    let code_point = 0x10000 + ((code_unit - 0xd800) << 10) + (low - 0xdc00);
                    output.push(char::from_u32(code_point).unwrap_or('\u{fffd}'));
                    i += 4;
                    continue;
                }
            }
            i += 2;
            continue;
        }
        if (0xdc00..=0xdfff).contains(&code_unit) {
            i += 2;
            continue;
        }
        output.push(char::from_u32(code_unit).unwrap_or('\u{fffd}'));
        i += 2;
    }
    output
}

/// Strict UTF-8 decode (TextDecoder fatal semantics).
fn decode_utf8_strict(data: &[u8]) -> Result<String, String> {
    match std::str::from_utf8(data) {
        Ok(text) => Ok(text.to_owned()),
        Err(error) => Err(format!("Invalid UTF-8: {error}")),
    }
}

/// Strict UTF-16LE decode with surrogate-pair validation.
fn decode_utf16le_strict(data: &[u8]) -> Result<String, String> {
    let mut output = String::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let code_unit = (data[i] as u32) | ((data[i + 1] as u32) << 8);
        if (0xd800..=0xdbff).contains(&code_unit) {
            let Some(&low_first) = data.get(i + 2) else {
                return Err("Invalid UTF-16LE: truncated high surrogate".to_owned());
            };
            let Some(&low_second) = data.get(i + 3) else {
                return Err("Invalid UTF-16LE: truncated high surrogate".to_owned());
            };
            let low = (low_first as u32) | ((low_second as u32) << 8);
            if !(0xdc00..=0xdfff).contains(&low) {
                return Err("Invalid UTF-16LE: unpaired high surrogate".to_owned());
            }
            let code_point = 0x10000 + ((code_unit - 0xd800) << 10) + (low - 0xdc00);
            output.push(char::from_u32(code_point).ok_or("Invalid code point")?);
            i += 4;
            continue;
        }
        if (0xdc00..=0xdfff).contains(&code_unit) {
            return Err("Invalid UTF-16LE: unpaired low surrogate".to_owned());
        }
        output.push(char::from_u32(code_unit).ok_or("Invalid code unit")?);
        i += 2;
    }
    if i < data.len() {
        return Err("Invalid UTF-16LE: odd byte length".to_owned());
    }
    Ok(output)
}

/// `decodeTextWithErrors` (decodeText.ts): web-label encodings go through
/// TextDecoder semantics (BOM stripped unless `ignore_bom`), everything else
/// through Node `Buffer.toString(encoding)`.
pub fn decode_with_errors(
    data: &[u8],
    encoding: &str,
    errors: DecodeErrors,
    ignore_bom: bool,
) -> Result<String, String> {
    match Encoding::parse(encoding) {
        Encoding::Utf8 => match errors {
            DecodeErrors::Strict => {
                let data = if ignore_bom {
                    data
                } else {
                    strip_utf8_bom(data)
                };
                decode_utf8_strict(data)
            }
            DecodeErrors::Replace => {
                let data = if ignore_bom {
                    data
                } else {
                    strip_utf8_bom(data)
                };
                Ok(String::from_utf8_lossy(data).into_owned())
            }
            DecodeErrors::Ignore => Ok(decode_utf8_ignore(data)),
        },
        Encoding::Utf16Le => match errors {
            DecodeErrors::Strict => {
                let data = if ignore_bom {
                    data
                } else {
                    strip_utf16le_bom(data)
                };
                decode_utf16le_strict(data)
            }
            DecodeErrors::Replace => {
                let data = if ignore_bom {
                    data
                } else {
                    strip_utf16le_bom(data)
                };
                Ok(decode_utf16le_replace(data))
            }
            DecodeErrors::Ignore => Ok(decode_utf16le_ignore(data)),
        },
        Encoding::Other(name) => Ok(buffer_to_string(data, name)),
    }
}

/// UTF-16LE replace mode: lone surrogates become U+FFFD.
fn decode_utf16le_replace(data: &[u8]) -> String {
    let mut output = String::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let code_unit = (data[i] as u32) | ((data[i + 1] as u32) << 8);
        if (0xd800..=0xdbff).contains(&code_unit) {
            if let (Some(&low_first), Some(&low_second)) = (data.get(i + 2), data.get(i + 3)) {
                let low = (low_first as u32) | ((low_second as u32) << 8);
                if (0xdc00..=0xdfff).contains(&low) {
                    let code_point = 0x10000 + ((code_unit - 0xd800) << 10) + (low - 0xdc00);
                    output.push(char::from_u32(code_point).unwrap_or('\u{fffd}'));
                    i += 4;
                    continue;
                }
            }
            output.push('\u{fffd}');
            i += 2;
            continue;
        }
        if (0xdc00..=0xdfff).contains(&code_unit) {
            output.push('\u{fffd}');
            i += 2;
            continue;
        }
        output.push(char::from_u32(code_unit).unwrap_or('\u{fffd}'));
        i += 2;
    }
    if i < data.len() {
        output.push('\u{fffd}');
    }
    output
}

/// Node `Buffer.toString(encoding)` for the non-web-label encodings.
/// Empirically verified: `ascii` masks to 7 bits, `latin1`/`binary` map
/// bytes to U+0000–U+00FF, `base64`/`base64url`/`hex` are ENCODING outputs.
fn buffer_to_string(data: &[u8], encoding: &str) -> String {
    match encoding {
        "ascii" => data.iter().map(|&b| (b & 0x7f) as char).collect(),
        "latin1" | "binary" => data.iter().map(|&b| b as char).collect(),
        "hex" => data.iter().map(|b| format!("{b:02x}")).collect(),
        "base64" => base64_encode(data, false),
        "base64url" => base64_encode(data, true),
        // Anything else Node does not know falls back to utf8 in
        // `Buffer.toString`; mirror that default.
        _ => String::from_utf8_lossy(data).into_owned(),
    }
}

/// Minimal standard base64 (RFC 4648) encoder — dimi-exec stays std-only.
fn base64_encode(data: &[u8], url_safe: bool) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        output.push(TABLE[(n >> 18) as usize & 0x3f] as char);
        output.push(TABLE[(n >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(n >> 6) as usize & 0x3f] as char);
        } else if !url_safe {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[n as usize & 0x3f] as char);
        } else if !url_safe {
            output.push('=');
        }
    }
    if url_safe {
        output.replace('+', "-").replace('/', "_")
    } else {
        output
    }
}

/// Split on `\n` keeping the terminator (`splitLinesKeepingTerminator`).
fn split_lines_keeping_terminator(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            lines.push(text[start..=i].to_owned());
            start = i + 1;
        }
    }
    if start < text.len() {
        lines.push(text[start..].to_owned());
    }
    lines
}

fn read_file_bytes(path: &str) -> Result<Vec<u8>, FsError> {
    fs::read(path).map_err(|error| FsError::from_io(path, "read", error))
}

/// `readText(path, options?)` — no options: `readFile(path, 'utf8')`
/// (lenient utf-8, BOM kept). With options: `decodeTextWithErrors`.
/// `readText` options — the PRESENCE of the options object selects the
/// `decodeTextWithErrors` path (Node parity: `options === undefined` →
/// `readFile(path, 'utf8')`).
#[derive(Debug, Clone, Default)]
pub struct ReadTextOptions {
    pub encoding: Option<String>,
    pub errors: Option<String>,
}

/// `readText(path, options?)` — no options: `readFile(path, 'utf8')`
/// (lenient utf-8, BOM kept). With options: `decodeTextWithErrors` (defaults
/// `utf-8` / `strict`; BOM stripped; strict throws).
pub fn read_text(path: &str, options: Option<&ReadTextOptions>) -> Result<String, FsError> {
    let data = read_file_bytes(path)?;
    let Some(options) = options else {
        // Node `readFile(path, 'utf8')` — lenient, BOM kept.
        return Ok(String::from_utf8_lossy(&data).into_owned());
    };
    let encoding = options.encoding.as_deref().unwrap_or("utf-8");
    let errors = options.errors.as_deref().unwrap_or("strict");
    decode_with_errors(&data, encoding, DecodeErrors::parse(errors), false)
        .map_err(|message| FsError::unknown(path, "read", message))
}

pub fn write_text(path: &str, data: &str) -> Result<(), FsError> {
    fs::write(path, data).map_err(|error| FsError::from_io(path, "write", error))
}

pub fn append_text(path: &str, data: &str) -> Result<(), FsError> {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|error| FsError::from_io(path, "append", error))?;
    file.write_all(data.as_bytes())
        .map_err(|error| FsError::from_io(path, "append", error))
}

/// `readBytes(path, n?)` — whole file, or up to `n` bytes from offset 0.
pub fn read_bytes(path: &str, n: Option<usize>) -> Result<Vec<u8>, FsError> {
    match n {
        None => read_file_bytes(path),
        Some(n) => {
            let mut file =
                File::open(path).map_err(|error| FsError::from_io(path, "read", error))?;
            let mut buf = vec![0u8; n];
            let mut filled = 0;
            while filled < n {
                match file.read(&mut buf[filled..]) {
                    Ok(0) => break,
                    Ok(count) => filled += count,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(FsError::from_io(path, "read", error)),
                }
            }
            buf.truncate(filled);
            Ok(buf)
        }
    }
}

pub fn write_bytes(path: &str, data: &[u8]) -> Result<(), FsError> {
    fs::write(path, data).map_err(|error| FsError::from_io(path, "write", error))
}

/// Streaming line reader — `readLines` state machine. utf-8 reads 64KB
/// blocks and stitches partial lines across blocks; other encodings
/// materialize the whole file (Node parity) and split afterwards.
pub struct ReadLines {
    file: File,
    buf: Vec<u8>,
    buf_start: usize,
    buf_end: usize,
    pending: Vec<u8>,
    pending_offset: usize,
    file_offset: usize,
    encoding: Encoding,
    errors: DecodeErrors,
    first_line: bool,
    eof: bool,
    /// Non-utf8 fast path: whole content decoded up front.
    materialized: Option<std::vec::IntoIter<String>>,
}

impl ReadLines {
    pub fn open(path: &str, encoding: Option<&str>, errors: Option<&str>) -> Result<Self, FsError> {
        let encoding = encoding.unwrap_or("utf-8");
        let errors = DecodeErrors::parse(errors.unwrap_or("strict"));
        let file = File::open(path).map_err(|error| FsError::from_io(path, "read", error))?;
        let encoding = Encoding::parse(encoding);
        let mut lines = Self {
            file,
            buf: vec![0u8; READ_CHUNK_SIZE],
            buf_start: 0,
            buf_end: 0,
            pending: Vec::new(),
            pending_offset: 0,
            file_offset: 0,
            encoding,
            errors,
            first_line: true,
            eof: false,
            materialized: None,
        };
        if !encoding.is_utf8() {
            lines.materialize(path)?;
        }
        Ok(lines)
    }

    fn materialize(&mut self, path: &str) -> Result<(), FsError> {
        let mut data = Vec::new();
        self.file
            .read_to_end(&mut data)
            .map_err(|error| FsError::from_io(path, "read", error))?;
        let text = decode_with_errors(&data, encoding_name(self.encoding), self.errors, false)
            .map_err(|message| FsError::unknown(path, "read", message))?;
        self.materialized = Some(split_lines_keeping_terminator(&text).into_iter());
        Ok(())
    }

    /// Next line including its `\n` terminator (`None` = EOF). Matches the
    /// TS `_readUtf8Lines` chunking, pending stitching and
    /// `ignoreBOM = lineOffset !== 0` semantics.
    pub fn next_line(&mut self, path: &str) -> Result<Option<String>, FsError> {
        if let Some(lines) = &mut self.materialized {
            return Ok(lines.next());
        }
        loop {
            // 1. Scan the buffered chunk for a line boundary.
            if let Some(relative) = find_newline(&self.buf[self.buf_start..self.buf_end]) {
                let end = self.buf_start + relative + 1;
                let piece = self.buf[self.buf_start..end].to_vec();
                self.buf_start = end;
                let line = if self.pending.is_empty() {
                    piece
                } else {
                    let mut joined = std::mem::take(&mut self.pending);
                    joined.extend_from_slice(&piece);
                    joined
                };
                return self.decode_line(path, &line);
            }
            // 2. Nothing complete in the chunk: move the tail into pending.
            if self.buf_start < self.buf_end {
                let tail = self.buf[self.buf_start..self.buf_end].to_vec();
                if self.pending.is_empty() {
                    self.pending_offset = self.file_offset + self.buf_start;
                }
                self.pending.extend_from_slice(&tail);
                self.buf_start = self.buf_end;
            }
            // 3. Refill the chunk.
            if self.eof {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                let line = std::mem::take(&mut self.pending);
                return self.decode_line(path, &line);
            }
            let mut filled = 0;
            while filled < self.buf.len() {
                match self.file.read(&mut self.buf[filled..]) {
                    Ok(0) => {
                        self.eof = true;
                        break;
                    }
                    Ok(count) => filled += count,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(FsError::from_io(path, "read", error)),
                }
            }
            self.buf_start = 0;
            self.buf_end = filled;
            self.file_offset += filled;
            if filled == 0 {
                self.eof = true;
            }
        }
    }

    fn decode_line(&mut self, path: &str, line: &[u8]) -> Result<Option<String>, FsError> {
        // `ignoreBOM = lineOffset !== 0` (decodeText.ts): only the first
        // line may carry the file's BOM.
        let ignore_bom = !self.first_line;
        self.first_line = false;
        let text = decode_with_errors(line, encoding_name(self.encoding), self.errors, ignore_bom)
            .map_err(|message| FsError::unknown(path, "read", message))?;
        Ok(Some(text))
    }
}

fn encoding_name(encoding: Encoding) -> &'static str {
    match encoding {
        Encoding::Utf8 => "utf-8",
        Encoding::Utf16Le => "utf-16le",
        Encoding::Other(name) => name,
    }
}

fn find_newline(chunk: &[u8]) -> Option<usize> {
    chunk.iter().position(|&b| b == 0x0a)
}

/// `createExclusive` — O_EXCL create + write + fsync; EEXIST → `false`.
pub fn create_exclusive(path: &str, data: &[u8]) -> Result<bool, FsError> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            let write_result = file
                .write_all(data)
                .and_then(|_| file.sync_all())
                .map_err(|error| FsError::from_io(path, "create", error));
            drop(file);
            write_result.map(|_| true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(FsError::from_io(path, "create", error)),
    }
}

/// `HostFileStat` mirror.
#[derive(Debug, Clone)]
pub struct FileStat {
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
    pub size: u64,
    pub mtime_ms: Option<f64>,
    pub ino: Option<u64>,
}

fn to_stat(meta: &fs::Metadata) -> FileStat {
    FileStat {
        is_file: meta.is_file(),
        is_directory: meta.is_dir(),
        is_symbolic_link: meta.file_type().is_symlink(),
        size: meta.len(),
        mtime_ms: meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs_f64() * 1000.0),
        ino: stat_ino(meta),
    }
}

#[cfg(unix)]
fn stat_ino(meta: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.ino())
}

#[cfg(not(unix))]
fn stat_ino(_meta: &fs::Metadata) -> Option<u64> {
    None
}

/// `stat` — follows symlinks.
pub fn stat(path: &str) -> Result<FileStat, FsError> {
    fs::metadata(path)
        .map(|meta| to_stat(&meta))
        .map_err(|error| FsError::from_io(path, "stat", error))
}

/// `lstat` — stats the entry itself.
pub fn lstat(path: &str) -> Result<FileStat, FsError> {
    fs::symlink_metadata(path)
        .map(|meta| to_stat(&meta))
        .map_err(|error| FsError::from_io(path, "lstat", error))
}

/// `HostDirEntry` mirror.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
}

/// `readdir` — `withFileTypes: true` semantics (entries are not followed).
pub fn readdir(path: &str) -> Result<Vec<DirEntry>, FsError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path).map_err(|error| FsError::from_io(path, "readdir", error))? {
        let entry = entry.map_err(|error| FsError::from_io(path, "readdir", error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| FsError::from_io(path, "readdir", error))?;
        entries.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_file: file_type.is_file(),
            is_directory: file_type.is_dir(),
            is_symbolic_link: file_type.is_symlink(),
        });
    }
    Ok(entries)
}

/// `mkdir(path, { recursive })` — recursive defaults to false. Non-recursive
/// uses `create_dir` (single level: missing parent → ENOENT, exists →
/// EEXIST, exactly like Node); recursive uses `create_dir_all` (existing
/// dirs are Ok).
pub fn mkdir(path: &str, recursive: bool) -> Result<(), FsError> {
    let result = if recursive {
        fs::create_dir_all(path)
    } else {
        fs::create_dir(path)
    };
    result.map_err(|error| FsError::from_io(path, "mkdir", error))
}

/// `remove` — `rm(path, { recursive: true, force: true })`.
pub fn remove(path: &str) -> Result<(), FsError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => {
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(FsError::from_io(path, "remove", error)),
            }
        }
        Err(error) => Err(FsError::from_io(path, "remove", error)),
    }
}

/// `realpath` — Node `fs.realpath` semantics (resolve every symlink).
pub fn realpath(path: &str) -> Result<String, FsError> {
    fs::canonicalize(path)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| FsError::from_io(path, "realpath", error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dimi-exec-fs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_path(name: &str) -> String {
        temp_dir().join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn read_write_roundtrip() {
        let path = temp_path("roundtrip.txt");
        write_text(&path, "hello").unwrap();
        assert_eq!(read_text(&path, None).unwrap(), "hello");
        append_text(&path, " world").unwrap();
        assert_eq!(read_text(&path, None).unwrap(), "hello world");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn read_bytes_partial_and_full() {
        let path = temp_path("bytes.bin");
        fs::write(&path, b"0123456789").unwrap();
        assert_eq!(read_bytes(&path, Some(4)).unwrap(), b"0123");
        assert_eq!(read_bytes(&path, Some(100)).unwrap(), b"0123456789");
        assert_eq!(read_bytes(&path, None).unwrap(), b"0123456789");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn missing_file_reports_errno() {
        let error = read_text("/definitely/missing", None).unwrap_err();
        assert_eq!(error.errno, Some(libc::ENOENT));
        assert_eq!(errno_name(error.errno), "ENOENT");
    }

    #[test]
    fn readlines_small_and_cross_chunk() {
        let path = temp_path("lines.txt");
        // One line of 200KB with many newlines → forces cross-chunk stitc
        // plus a final unterminated line.
        let mut content = String::new();
        for i in 0..100_000 {
            content.push_str(&format!("line{i}\n"));
        }
        content.push_str("tail-no-newline");
        fs::write(&path, content.as_bytes()).unwrap();

        let mut lines = ReadLines::open(&path, None, None).unwrap();
        let mut count = 0;
        while let Some(line) = lines.next_line(&path).unwrap() {
            if count < 3 {
                assert_eq!(line, format!("line{count}\n"));
            }
            count += 1;
        }
        assert_eq!(count, 100_001);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn readlines_utf8_bom_stripped_on_first_line() {
        let path = temp_path("bom.txt");
        fs::write(&path, [0xef, 0xbb, 0xbf, b'A', b'\n', b'B']).unwrap();
        let mut lines = ReadLines::open(&path, None, None).unwrap();
        assert_eq!(lines.next_line(&path).unwrap().unwrap(), "A\n");
        assert_eq!(lines.next_line(&path).unwrap().unwrap(), "B");
        assert_eq!(lines.next_line(&path).unwrap(), None);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn readlines_strict_rejects_invalid_utf8() {
        let path = temp_path("bad-utf8.txt");
        fs::write(&path, [0x80, b'\n']).unwrap();
        let mut lines = ReadLines::open(&path, None, Some("strict")).unwrap();
        assert!(lines.next_line(&path).is_err());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn readtext_utf8_lenient_keeps_bom_like_readfile() {
        let path = temp_path("bom-read.txt");
        fs::write(&path, [0xef, 0xbb, 0xbf, b'A', 0x80]).unwrap();
        // No options = readFile(path, 'utf8'): BOM kept, invalid replaced.
        let text = read_text(&path, None).unwrap();
        assert_eq!(text, "\u{feff}A\u{fffd}");
        // With options = decodeTextWithErrors: BOM stripped, strict throws.
        assert!(
            read_text(
                &path,
                Some(&ReadTextOptions {
                    encoding: Some("utf-8".to_owned()),
                    errors: Some("strict".to_owned())
                })
            )
            .is_err()
        );
        let replaced = read_text(
            &path,
            Some(&ReadTextOptions {
                encoding: Some("utf-8".to_owned()),
                errors: Some("replace".to_owned()),
            }),
        )
        .unwrap();
        assert_eq!(replaced, "A\u{fffd}");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn create_exclusive_semantics() {
        let path = temp_path("exclusive.txt");
        assert!(create_exclusive(&path, b"data").unwrap());
        assert!(!create_exclusive(&path, b"other").unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"data");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn stat_lstat_symlink() {
        let dir = temp_dir();
        let target = dir.join("target.txt");
        fs::write(&target, "hello").unwrap();
        let link = dir.join("link.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        {
            // symlink may require privileges; skip gracefully
            return;
        }

        let st = stat(&link.to_string_lossy()).unwrap();
        assert!(st.is_file);
        assert!(!st.is_symbolic_link);
        let lst = lstat(&link.to_string_lossy()).unwrap();
        assert!(lst.is_symbolic_link);
        assert!(!lst.is_file);

        let entries = readdir(&dir.to_string_lossy()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"link.txt"));
        let link_entry = entries.iter().find(|e| e.name == "link.txt").unwrap();
        assert!(link_entry.is_symbolic_link);
        fs::remove_file(&link).unwrap();
        fs::remove_file(&target).unwrap();
    }

    #[test]
    fn mkdir_recursive_and_nonrecursive() {
        let dir = temp_dir();
        let nested = dir.join("a").join("b");
        assert!(mkdir(&nested.to_string_lossy(), false).is_err()); // parent missing
        mkdir(&nested.to_string_lossy(), true).unwrap();
        assert!(mkdir(&nested.to_string_lossy(), false).is_err()); // exists → EEXIST
        mkdir(&nested.to_string_lossy(), true).unwrap(); // recursive on existing → Ok
        fs::remove_dir_all(dir.join("a")).unwrap();
    }

    #[test]
    fn remove_file_dir_and_missing() {
        let dir = temp_dir();
        let file = dir.join("f.txt");
        fs::write(&file, "x").unwrap();
        remove(&file.to_string_lossy()).unwrap();
        assert!(!file.exists());

        let sub = dir.join("sub");
        fs::create_dir_all(sub.join("deep")).unwrap();
        fs::write(sub.join("deep").join("x"), "y").unwrap();
        remove(&sub.to_string_lossy()).unwrap();
        assert!(!sub.exists());

        remove("/definitely/missing").unwrap(); // force
    }

    #[test]
    fn realpath_resolves_symlinks() {
        let dir = temp_dir();
        let target = dir.join("real.txt");
        fs::write(&target, "x").unwrap();
        #[cfg(unix)]
        let link = dir.join("alias.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(not(unix))]
        let link = target.clone();

        let resolved = realpath(&link.to_string_lossy()).unwrap();
        assert!(resolved.ends_with("real.txt"), "{resolved}");
        #[cfg(unix)]
        fs::remove_file(&link).unwrap();
        fs::remove_file(&target).unwrap();
    }

    #[test]
    fn encodings_match_node() {
        let data = [0x41u8, 0x80, 0xff, 0xe4, 0xb8, 0xad];
        assert_eq!(
            decode_with_errors(&data, "ascii", DecodeErrors::Strict, false).unwrap(),
            "A\u{0}\u{7f}d8-"
        );
        assert_eq!(
            decode_with_errors(&data, "latin1", DecodeErrors::Strict, false).unwrap(),
            "A\u{80}\u{ff}\u{e4}\u{b8}\u{ad}"
        );
        assert_eq!(
            decode_with_errors(&data, "base64", DecodeErrors::Strict, false).unwrap(),
            "QYD/5Lit"
        );
        assert_eq!(
            decode_with_errors(&data, "hex", DecodeErrors::Strict, false).unwrap(),
            "4180ffe4b8ad"
        );
    }

    #[test]
    fn utf16le_modes() {
        let good = [0x41u8, 0x00, 0x42, 0x00];
        assert_eq!(
            decode_with_errors(&good, "utf16le", DecodeErrors::Strict, false).unwrap(),
            "AB"
        );
        // Lone low surrogate → strict errs, replace emits U+FFFD, ignore skips.
        let bad = [0x41u8, 0x00, 0x00, 0xdc];
        assert!(decode_with_errors(&bad, "utf16le", DecodeErrors::Strict, false).is_err());
        assert_eq!(
            decode_with_errors(&bad, "utf16le", DecodeErrors::Replace, false).unwrap(),
            "A\u{fffd}"
        );
        assert_eq!(
            decode_with_errors(&bad, "utf16le", DecodeErrors::Ignore, false).unwrap(),
            "A"
        );
    }

    #[test]
    fn utf8_ignore_mode_skips_invalid() {
        let data = [0x41u8, 0x80, 0x42, 0xe4, 0xb8];
        assert_eq!(
            decode_with_errors(&data, "utf-8", DecodeErrors::Ignore, false).unwrap(),
            "AB"
        );
    }
}
