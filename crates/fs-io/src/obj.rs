//! OBJ import/export (subset): `v` positions and `f` faces (arbitrary
//! polygons fan-triangulated; `v/vt/vn` index forms accepted, texture and
//! normal indices ignored — documented lossy). Negative (relative)
//! indices supported per spec. Export writes positions with full f64
//! round-trip precision.

use crate::{IoError, MAX_ELEMENTS};
use fs_geom::Point3;
use fs_rep_mesh::Soup;
use std::fmt::Write as _;

/// Import an OBJ subset.
///
/// # Errors
/// [`IoError`] on malformed indices/coordinates or resource bounds.
pub fn read_obj(text: &str) -> Result<Soup, IoError> {
    let mut positions: Vec<Point3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let mut v = [0.0f64; 3];
                for slot in &mut v {
                    let tok = it.next().ok_or(IoError::Malformed {
                        at: ln + 1,
                        what: "v needs three coordinates".to_string(),
                    })?;
                    *slot = tok.parse::<f64>().map_err(|_| IoError::Malformed {
                        at: ln + 1,
                        what: format!("bad coordinate {tok:?}"),
                    })?;
                    if !slot.is_finite() {
                        return Err(IoError::Malformed {
                            at: ln + 1,
                            what: "non-finite coordinate".to_string(),
                        });
                    }
                }
                positions.push(Point3::new(v[0], v[1], v[2]));
                if positions.len() > MAX_ELEMENTS {
                    return Err(IoError::ResourceBound {
                        what: "vertex count exceeds the element cap".to_string(),
                    });
                }
            }
            Some("f") => {
                let mut idx: Vec<u32> = Vec::new();
                for tok in it {
                    let first = tok.split('/').next().unwrap_or("");
                    let signed: i64 = first.parse().map_err(|_| IoError::Malformed {
                        at: ln + 1,
                        what: format!("bad face index {tok:?}"),
                    })?;
                    let resolved: i64 = if signed < 0 {
                        i64::try_from(positions.len()).expect("cap") + signed
                    } else {
                        signed - 1
                    };
                    if resolved < 0 || resolved >= i64::try_from(positions.len()).expect("cap") {
                        return Err(IoError::Malformed {
                            at: ln + 1,
                            what: format!("face index {signed} out of range"),
                        });
                    }
                    idx.push(u32::try_from(resolved).expect("range checked"));
                }
                if idx.len() < 3 {
                    return Err(IoError::Malformed {
                        at: ln + 1,
                        what: "face needs at least three vertices".to_string(),
                    });
                }
                for k in 1..idx.len() - 1 {
                    triangles.push([idx[0], idx[k], idx[k + 1]]);
                    if triangles.len() > MAX_ELEMENTS {
                        return Err(IoError::ResourceBound {
                            what: "triangle count exceeds the element cap".to_string(),
                        });
                    }
                }
            }
            _ => {} // vt/vn/usemtl/o/g/s… ignored (documented subset)
        }
    }
    if triangles.is_empty() {
        return Err(IoError::Malformed {
            at: 0,
            what: "OBJ contains no faces".to_string(),
        });
    }
    Ok(Soup {
        positions,
        triangles,
    })
}

/// Export as OBJ (deterministic; f64 round-trip precision).
#[must_use]
pub fn write_obj(soup: &Soup) -> String {
    let mut out = String::with_capacity(soup.positions.len() * 32);
    for p in &soup.positions {
        let _ = writeln!(out, "v {} {} {}", p.x, p.y, p.z);
    }
    for t in &soup.triangles {
        let _ = writeln!(out, "f {} {} {}", t[0] + 1, t[1] + 1, t[2] + 1);
    }
    out
}
