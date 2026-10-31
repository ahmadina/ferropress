//! Split a page path and walk candidate rows.

use std::collections::HashMap;

use cms_db::{Cell, ResultSet};

use crate::PostId;
use crate::title::sanitize_title_for_query;

const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

/// One `get_page_by_path` candidate row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PathHit {
    id: PostId,
    name: Box<[u8]>,
    parent: i64,
    type_name: Box<[u8]>,
}

impl PathHit {
    pub(crate) fn from_set(set: &ResultSet) -> Vec<Self> {
        set.rows()
            .iter()
            .filter_map(|row| hit_from_row(set, row.cells()))
            .collect()
    }
}

/// Path segments after decode, slash split, and query-title sanitizing.
#[must_use]
pub(crate) fn path_parts(path: &[u8]) -> Vec<Box<[u8]>> {
    let decoded = urldecode(path);
    let encoded = rawurlencode(&decoded);
    let mixed = restore_slash_and_space(&encoded);
    let trimmed = trim_slashes(&mixed);
    split_slash(trimmed)
        .into_iter()
        .map(|part| sanitize_title_for_query(&part))
        .collect()
}

/// The winning id, preferring `want_type` over attachment.
#[must_use]
pub(crate) fn walk(parts: &[Box<[u8]>], hits: &[PathHit], want_type: &[u8]) -> Option<PostId> {
    if parts.is_empty() {
        return None;
    }
    let rev: Vec<&[u8]> = parts.iter().rev().map(AsRef::as_ref).collect();
    let by_id: HashMap<u64, &PathHit> = hits.iter().map(|hit| (hit.id.as_u64(), hit)).collect();
    let mut found = None;
    for hit in hits {
        if hit.name.as_ref() != rev[0] {
            continue;
        }
        let mut count = 0;
        let mut current = hit;
        while current.parent != 0 {
            let Some(parent) = u64::try_from(current.parent)
                .ok()
                .and_then(|id| by_id.get(&id).copied())
            else {
                break;
            };
            count += 1;
            if rev
                .get(count)
                .is_none_or(|name| parent.name.as_ref() != *name)
            {
                break;
            }
            current = parent;
        }
        if current.parent == 0 && rev.len() == count + 1 && current.name.as_ref() == rev[count] {
            found = Some(hit.id);
            if hit.type_name.as_ref() == want_type {
                break;
            }
        }
    }
    found
}

fn hit_from_row(set: &ResultSet, cells: &[Cell]) -> Option<PathHit> {
    let mut id = None;
    let mut name = Vec::new();
    let mut parent = 0;
    let mut type_name = Vec::new();
    for (column, cell) in set.columns().iter().zip(cells.iter()) {
        let bytes = cell_bytes(cell);
        match column.as_bytes() {
            b"ID" => id = PostId::from_bytes(&bytes).ok(),
            b"post_name" => name = bytes,
            b"post_parent" => parent = php_int(&bytes),
            b"post_type" => type_name = bytes,
            _ => {}
        }
    }
    Some(PathHit {
        id: id?,
        name: name.into(),
        parent,
        type_name: type_name.into(),
    })
}

fn cell_bytes(cell: &Cell) -> Vec<u8> {
    match cell {
        Cell::Null => Vec::new(),
        Cell::Bytes(bytes) => bytes.to_vec(),
    }
}

fn php_int(bytes: &[u8]) -> i64 {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| {
            let trimmed = text.trim_start();
            let end = trimmed
                .find(|ch: char| !ch.is_ascii_digit() && ch != '-' && ch != '+')
                .unwrap_or(trimmed.len());
            trimmed[..end].parse().ok()
        })
        .unwrap_or(0)
}

fn urldecode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
                && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit) =>
            {
                let hi = hex_val(bytes[index + 1]);
                let lo = hex_val(bytes[index + 2]);
                out.push((hi << 4) | lo);
                index += 3;
            }
            other => {
                out.push(other);
                index += 1;
            }
        }
    }
    out
}

fn rawurlencode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &byte in bytes {
        if UNRESERVED.contains(&byte) {
            out.push(byte);
        } else {
            out.extend_from_slice(format!("%{byte:02X}").as_bytes());
        }
    }
    out
}

fn restore_slash_and_space(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"%2F") {
            out.push(b'/');
            index += 3;
        } else if bytes[index..].starts_with(b"%20") {
            out.push(b' ');
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    out
}

fn trim_slashes(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&b| b != b'/').unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|&b| b != b'/')
        .map_or(start, |index| index + 1);
    if start >= end {
        &[]
    } else {
        &bytes[start..end]
    }
}

fn split_slash(bytes: &[u8]) -> Vec<Vec<u8>> {
    if bytes.is_empty() {
        return vec![Vec::new()];
    }
    bytes.split(|&b| b == b'/').map(Vec::from).collect()
}

fn hex_val(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{PathHit, path_parts, walk};
    use crate::PostId;
    use cms_db::{Cell, Column, ResultSet, Row};

    fn hit(id: u64, name: &[u8], parent: i64, type_name: &[u8]) -> Option<PathHit> {
        PostId::new(id).ok().map(|id| PathHit {
            id,
            name: name.into(),
            parent,
            type_name: type_name.into(),
        })
    }

    #[test]
    fn parts_decode_split_and_sanitize() {
        assert_eq!(path_parts(b"about"), vec![Box::<[u8]>::from(*b"about")]);
        assert_eq!(
            path_parts(b"/Parent/Child/"),
            vec![Box::<[u8]>::from(*b"parent"), Box::<[u8]>::from(*b"child")]
        );
        assert_eq!(
            path_parts(b"Hello+World"),
            vec![Box::<[u8]>::from(*b"hello-world")]
        );
        assert_eq!(path_parts(b""), vec![Box::<[u8]>::from(*b"")]);
        assert_eq!(
            path_parts(b"%2Fabout%2F"),
            vec![Box::<[u8]>::from(*b"about")]
        );
    }

    #[test]
    fn walk_prefers_page_and_requires_ancestry() {
        let parent = hit(1, b"parent", 0, b"page");
        let child = hit(2, b"child", 1, b"page");
        let attach = hit(3, b"about", 0, b"attachment");
        let page = hit(4, b"about", 0, b"page");
        assert!(parent.is_some() && child.is_some() && attach.is_some() && page.is_some());
        if let (Some(parent), Some(child), Some(attach), Some(page)) = (parent, child, attach, page)
        {
            let parts = path_parts(b"parent/child");
            let id = walk(&parts, &[parent, child.clone()], b"page");
            assert!(id.is_some_and(|id| id.as_u64() == 2));
            let about = path_parts(b"about");
            let id = walk(&about, &[attach.clone(), page], b"page");
            assert!(id.is_some_and(|id| id.as_u64() == 4));
            let id = walk(&about, &[attach], b"page");
            assert!(id.is_some_and(|id| id.as_u64() == 3));
            assert!(walk(&parts, &[child], b"page").is_none());
            assert!(walk(&[], &[], b"page").is_none());
        }
    }

    #[test]
    fn from_set_reads_named_columns() {
        let set = ResultSet::new(
            [
                Column::new("ID"),
                Column::new("post_name"),
                Column::new("post_parent"),
                Column::new("post_type"),
                Column::new("extra"),
            ],
            [Row::new([
                Cell::bytes("4"),
                Cell::bytes("about"),
                Cell::bytes("0"),
                Cell::bytes("page"),
                Cell::bytes("x"),
            ])],
        );
        let hits = PathHit::from_set(&set);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.as_u64(), 4);
        assert_eq!(hits[0].name.as_ref(), b"about");
        assert_eq!(hits[0].parent, 0);
        assert!(
            PathHit::from_set(&ResultSet::new(
                [Column::new("post_name")],
                [Row::new([Cell::bytes("x")])],
            ))
            .is_empty()
        );
    }
}
