//! Fail the Nix edge build when the shipped binary references an asset id
//! that is not in the bundle manifest installed next to it.
//!
//! Topcoat panics at request time (`failed to resolve asset`) if those drift.
//! `stylesheet!()` hashes `OUT_DIR` (relative to the crate manifest, so it
//! still contains the target triple and build hash) into the id, so a
//! manifest produced from any other cargo invocation will not match.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process::ExitCode;

const PREFIX: &[u8] = b"TOPCOAT_ASSET";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [binary_path, manifest_path] = args.as_slice() else {
        eprintln!("usage: check-asset-ids <binary> <manifest.toml>");
        return ExitCode::from(2);
    };

    let (binary, manifest) = match (fs::read(binary_path), fs::read_to_string(manifest_path)) {
        (Ok(binary), Ok(manifest)) => (binary, manifest),
        (Err(err), _) => {
            eprintln!("read {binary_path}: {err}");
            return ExitCode::from(2);
        }
        (_, Err(err)) => {
            eprintln!("read {manifest_path}: {err}");
            return ExitCode::from(2);
        }
    };

    let embedded = embedded_asset_ids(&binary);
    let bundled = manifest_ids(&manifest);

    if embedded.is_empty() {
        eprintln!("no TOPCOAT_ASSET declarations found in {binary_path}");
        return ExitCode::from(1);
    }

    let missing: Vec<u64> = embedded.difference(&bundled).copied().collect();
    if missing.is_empty() {
        println!(
            "asset ids match: {} embedded, {} in {}",
            embedded.len(),
            bundled.len(),
            manifest_path
        );
        return ExitCode::SUCCESS;
    }

    eprintln!(
        "asset bundle does not match {binary_path}: {} id(s) embedded in the binary are missing from {manifest_path}",
        missing.len()
    );
    for id in missing {
        eprintln!("  missing id {id}");
    }
    eprintln!(
        "stylesheet!() includes OUT_DIR in the asset id. Bundle the same binary that is installed (same --target and profile)."
    );
    ExitCode::from(1)
}

fn manifest_ids(text: &str) -> BTreeSet<u64> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("id = "))
        .filter_map(|id| id.trim().parse().ok())
        .collect()
}

fn embedded_asset_ids(binary: &[u8]) -> BTreeSet<u64> {
    binary
        .windows(PREFIX.len())
        .enumerate()
        .filter(|(_, window)| *window == PREFIX)
        .filter_map(|(i, _)| decode_asset_id(&binary[i..]))
        .collect()
}

/// Decode one embedded `RawAsset` blob (topcoat-asset 0.12 layout:
/// prefix, u64 id, path, then 4 optional strings for rename / extension /
/// checksum / content type). Mirrors `RawAsset::find_in_binary`, which
/// also skips declarations with an empty path (rust-analyzer placeholders).
/// Returns `None` for stray prefix matches that do not decode.
fn decode_asset_id(buf: &[u8]) -> Option<u64> {
    let mut pos = PREFIX.len();
    let id = read_u64(buf, &mut pos)?;
    let path = read_str(buf, &mut pos)?;
    for _ in 0..4 {
        read_str_opt(buf, &mut pos)?;
    }
    if path.is_empty() || path.len() > 4096 {
        return None;
    }
    Some(id)
}

fn read_u64(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let bytes = read_bytes(buf, pos, 8)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

fn read_str<'a>(buf: &'a [u8], pos: &mut usize) -> Option<&'a str> {
    let len = read_u16(buf, pos)? as usize;
    let bytes = read_bytes(buf, pos, len)?;
    std::str::from_utf8(bytes).ok()
}

fn read_str_opt<'a>(buf: &'a [u8], pos: &mut usize) -> Option<Option<&'a str>> {
    match read_bytes(buf, pos, 1)?[0] {
        0 => Some(None),
        1 => read_str(buf, pos).map(Some),
        _ => None,
    }
}

fn read_u16(buf: &[u8], pos: &mut usize) -> Option<u16> {
    let bytes = read_bytes(buf, pos, 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Option<&'a [u8]> {
    let end = pos.checked_add(n)?;
    if end > buf.len() {
        return None;
    }
    let slice = &buf[*pos..end];
    *pos = end;
    Some(slice)
}
