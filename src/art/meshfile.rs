//! Reads the small binary meshes `tools/art/build_meshes.py` writes (`assets/models/*.pnm`):
//! "PNM1", u32 vertex count, u32 index count, f32 positions / normals / uvs, u32 indices.
//! Read synchronously at stage load so the meshes are on the CPU when PCG merges them.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// Parses a `.pnm` file into a mesh with tangents (for normal maps).
pub fn parse(bytes: &[u8]) -> Result<Mesh, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"PNM1" {
        return Err("not a PNM1 mesh".into());
    }
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let (nv, ni) = (u32_at(4) as usize, u32_at(8) as usize);
    let need = 12 + nv * 32 + ni * 4;
    if bytes.len() < need {
        return Err(format!("truncated: {} < {need} bytes", bytes.len()));
    }
    let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let mut o = 12;
    let vec3s = |n: usize, o: &mut usize| -> Vec<[f32; 3]> {
        let v = (0..n).map(|k| [f(*o + k * 12), f(*o + k * 12 + 4), f(*o + k * 12 + 8)]).collect();
        *o += n * 12;
        v
    };
    let pos = vec3s(nv, &mut o);
    let nrm = vec3s(nv, &mut o);
    let uv: Vec<[f32; 2]> = (0..nv).map(|k| [f(o + k * 8), f(o + k * 8 + 4)]).collect();
    o += nv * 8;
    let idx: Vec<u32> = (0..ni).map(|k| u32_at(o + k * 4)).collect();
    if idx.iter().any(|&i| i as usize >= nv) {
        return Err("index out of range".into());
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(Indices::U32(idx));
    m.generate_tangents().map_err(|e| format!("tangents: {e}"))?;
    Ok(m)
}

/// Loads `assets/models/<name>.pnm`.
pub fn load(dir: &std::path::Path, name: &str) -> Result<Mesh, String> {
    let p = dir.join("models").join(format!("{name}.pnm"));
    let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    parse(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_triangle_and_rejects_bad_files() {
        let mut b = b"PNM1".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(3u32.to_le_bytes());
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            b.extend(v.iter().flat_map(|x| x.to_le_bytes()));
        }
        for _ in 0..3 {
            b.extend([0.0f32, 1.0, 0.0].iter().flat_map(|x| x.to_le_bytes()));
        }
        for v in [[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]] {
            b.extend(v.iter().flat_map(|x| x.to_le_bytes()));
        }
        for i in [0u32, 2, 1] {
            b.extend(i.to_le_bytes());
        }
        let m = parse(&b).unwrap();
        assert_eq!(m.count_vertices(), 3);
        assert!(parse(&b[..20]).is_err());
        assert!(parse(b"nope").is_err());
    }

    #[test]
    fn shipped_meshes_load() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for name in crate::art::dressing::MESHES {
            let m = load(&dir, name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(m.count_vertices() > 20, "{name}");
        }
    }
}
