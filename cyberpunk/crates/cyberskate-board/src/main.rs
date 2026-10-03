//! `cyberskate-board <skate-data/assets> <out/board.glb>`
//!
//! The Skate 3 skater the converter writes (`private/skater.glb`) carries the
//! board as primitives skinned to the board's bones, in the skater's bind
//! pose. This takes those primitives, moves them into the frame of the bone
//! most of the board hangs from (the deck), and writes them as one static
//! mesh whose axes are the ones the mod places a board entity with: x right,
//! y nose, z up in Cyberpunk, stored y-up as glTF expects. Textures are the
//! skater file's own images, copied as they are.
use serde_json::{Value, json};
use std::path::Path;

fn index(value: &Value) -> Result<usize, String> {
    value
        .as_u64()
        .map(|v| v as usize)
        .ok_or_else(|| format!("expected an index, found {value}"))
}

struct Glb<'a> {
    json: Value,
    blob: &'a [u8],
}

impl<'a> Glb<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        let word = |at: usize| -> Result<usize, String> {
            bytes
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                .ok_or_else(|| "truncated GLB".to_owned())
        };
        if bytes.get(..4) != Some(b"glTF") {
            return Err("not a GLB file".into());
        }
        let json_len = word(12)?;
        let json =
            serde_json::from_slice(bytes.get(20..20 + json_len).ok_or("truncated GLB JSON")?)
                .map_err(|e| e.to_string())?;
        let blob_len = word(20 + json_len)?;
        let start = 28 + json_len;
        let blob = bytes
            .get(start..start + blob_len)
            .ok_or("truncated GLB binary")?;
        Ok(Self { json, blob })
    }

    fn view(&self, view: usize) -> Result<(&'a [u8], Option<usize>), String> {
        let view = &self.json["bufferViews"][view];
        let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let length = index(&view["byteLength"])?;
        let bytes = self
            .blob
            .get(offset..offset + length)
            .ok_or("buffer view outside the GLB")?;
        Ok((bytes, view["byteStride"].as_u64().map(|s| s as usize)))
    }

    fn components(&self, accessor: usize) -> Result<Vec<f64>, String> {
        let a = &self.json["accessors"][accessor];
        let count = index(&a["count"])?;
        let width = match a["type"].as_str() {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            Some("MAT4") => 16,
            other => return Err(format!("unsupported accessor type {other:?}")),
        };
        let (size, read): (usize, fn(&[u8]) -> f64) = match a["componentType"].as_u64() {
            Some(5121) => (1, |b| f64::from(b[0])),
            Some(5123) => (2, |b| f64::from(u16::from_le_bytes([b[0], b[1]]))),
            Some(5125) => (4, |b| {
                f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }),
            Some(5126) => (4, |b| {
                f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }),
            other => return Err(format!("unsupported component type {other:?}")),
        };
        let (bytes, stride) = self.view(index(&a["bufferView"])?)?;
        let start = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = stride.unwrap_or(size * width);
        let mut out = Vec::with_capacity(count * width);
        for e in 0..count {
            for c in 0..width {
                let at = start + e * stride + c * size;
                out.push(read(
                    bytes
                        .get(at..at + size)
                        .ok_or("accessor outside its view")?,
                ));
            }
        }
        Ok(out)
    }
}

/// Column-major 4x4 times a point (w = 1) or a direction (w = 0).
fn apply(m: &[f64], v: [f64; 3], w: f64) -> [f64; 3] {
    std::array::from_fn(|r| m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2] + m[12 + r] * w)
}

/// A point in the deck bone's frame as glTF stores it for the mod's board.
/// The GLB's bones carry Blender's basis; the native bone frame is right,
/// up, nose. Cyberpunk's board axes are right, nose, up with right = nose ×
/// up, which turns native (a, b, c) into (-a, c, b); glTF is y-up, turning
/// Cyberpunk (x, y, z) into (x, z, -y). Both are rotations, so faces keep
/// their winding.
fn to_board(inverse_bind: &[f64], v: [f64; 3], w: f64) -> [f32; 3] {
    let [x, y, z] = apply(inverse_bind, v, w);
    let native = [x, -z, y];
    let cyberpunk = [-native[0], native[2], native[1]];
    [
        cyberpunk[0] as f32,
        cyberpunk[2] as f32,
        -cyberpunk[1] as f32,
    ]
}

struct Part {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    image: usize,
    name: String,
}

pub fn export(skater: &[u8]) -> Result<(Vec<u8>, String), String> {
    let glb = Glb::parse(skater)?;
    let skin = &glb.json["skins"][0];
    let joints = skin["joints"].as_array().ok_or("skater has no skin")?;
    let inverse_binds = glb.components(index(&skin["inverseBindMatrices"])?)?;
    let primitives = glb.json["meshes"][0]["primitives"]
        .as_array()
        .ok_or("skater has no mesh")?;
    let board: Vec<&Value> = primitives
        .iter()
        .filter(|p| {
            let m = &glb.json["materials"][p["material"].as_u64().unwrap_or(u64::MAX) as usize];
            m["name"].as_str().is_some_and(|n| n.contains("Skate"))
        })
        .collect();
    if board.is_empty() {
        return Err("the skater has no skateboard surfaces".into());
    }
    // The deck: the bone carrying most of the board's skin weight.
    let mut weight = vec![0f64; joints.len()];
    for p in &board {
        let ids = glb.components(index(&p["attributes"]["JOINTS_0"])?)?;
        let ws = glb.components(index(&p["attributes"]["WEIGHTS_0"])?)?;
        for (j, w) in ids.iter().zip(&ws) {
            if let Some(slot) = weight.get_mut(*j as usize) {
                *slot += w;
            }
        }
    }
    let deck = (0..joints.len())
        .max_by(|a, b| weight[*a].total_cmp(&weight[*b]))
        .ok_or("no joints")?;
    let deck_name = glb.json["nodes"][index(&joints[deck])?]["name"]
        .as_str()
        .unwrap_or("?")
        .to_owned();
    let inverse_bind = &inverse_binds[deck * 16..deck * 16 + 16];

    let mut parts = Vec::new();
    for p in &board {
        let a = &p["attributes"];
        let pos = glb.components(index(&a["POSITION"])?)?;
        let nrm = glb.components(index(&a["NORMAL"])?)?;
        let uv = glb.components(index(&a["TEXCOORD_0"])?)?;
        let material = &glb.json["materials"][index(&p["material"])?];
        let texture = index(&material["pbrMetallicRoughness"]["baseColorTexture"]["index"])?;
        let image = index(&glb.json["textures"][texture]["source"])?;
        let normal = |c: &[f64]| {
            let n = to_board(inverse_bind, [c[0], c[1], c[2]], 0.);
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
            n.map(|v| v / l)
        };
        parts.push(Part {
            positions: pos
                .chunks(3)
                .map(|c| to_board(inverse_bind, [c[0], c[1], c[2]], 1.))
                .collect(),
            normals: nrm.chunks(3).map(normal).collect(),
            uvs: uv.chunks(2).map(|c| [c[0] as f32, c[1] as f32]).collect(),
            indices: glb
                .components(index(&p["indices"])?)?
                .iter()
                .map(|&i| i as u32)
                .collect(),
            image,
            name: material["name"].as_str().unwrap_or("board").to_owned(),
        });
    }
    upright(&mut parts);
    Ok((write(&glb, &parts)?, deck_name))
}

/// The deck bone's up may point through the grip tape or through the
/// wheels depending on how the rig was authored; the trucks and wheels must
/// hang below the deck, so if they sit above it the board is turned half a
/// turn about its nose (glTF y is up, z is the nose).
fn upright(parts: &mut [Part]) {
    let mean_height =
        |p: &Part| p.positions.iter().map(|v| v[1]).sum::<f32>() / p.positions.len().max(1) as f32;
    let (deck, rest): (Vec<&Part>, Vec<&Part>) =
        parts.iter().partition(|p| p.name.contains("Board"));
    let (Some(deck), false) = (deck.first(), rest.is_empty()) else {
        return;
    };
    let below = rest.iter().map(|p| mean_height(p)).sum::<f32>() / rest.len() as f32;
    if below > mean_height(deck) {
        for part in parts.iter_mut() {
            for v in part.positions.iter_mut().chain(part.normals.iter_mut()) {
                v[0] = -v[0];
                v[1] = -v[1];
            }
        }
    }
}

fn write(source: &Glb<'_>, parts: &[Part]) -> Result<Vec<u8>, String> {
    let mut blob = Vec::<u8>::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut push = |bytes: Vec<u8>, blob: &mut Vec<u8>| {
        while blob.len() % 4 != 0 {
            blob.push(0);
        }
        views.push(json!({"buffer": 0, "byteOffset": blob.len(), "byteLength": bytes.len()}));
        blob.extend(bytes);
        views.len() - 1
    };
    let floats = |v: &[f32]| v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>();
    let mut images = Vec::new();
    let mut image_of = std::collections::HashMap::new();
    let mut materials = Vec::new();
    let mut prims = Vec::new();
    for part in parts {
        let image = match image_of.get(&part.image) {
            Some(&i) => i,
            None => {
                let src = &source.json["images"][part.image];
                let (bytes, _) = source.view(index(&src["bufferView"])?)?;
                let view = push(bytes.to_vec(), &mut blob);
                images.push(json!({"bufferView": view, "mimeType": src["mimeType"].as_str().unwrap_or("image/png")}));
                image_of.insert(part.image, images.len() - 1);
                images.len() - 1
            }
        };
        materials.push(json!({
            "name": part.name,
            "pbrMetallicRoughness": {"baseColorTexture": {"index": image}, "metallicFactor": 0.0, "roughnessFactor": 0.8}
        }));
        let flat = |v: &[[f32; 3]]| v.iter().flatten().copied().collect::<Vec<f32>>();
        let (lo, hi) = part
            .positions
            .iter()
            .fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
                (
                    std::array::from_fn(|i| lo[i].min(p[i])),
                    std::array::from_fn(|i| hi[i].max(p[i])),
                )
            });
        let pv = push(floats(&flat(&part.positions)), &mut blob);
        accessors.push(json!({"bufferView": pv, "componentType": 5126, "count": part.positions.len(), "type": "VEC3", "min": lo, "max": hi}));
        let nv = push(floats(&flat(&part.normals)), &mut blob);
        accessors.push(json!({"bufferView": nv, "componentType": 5126, "count": part.normals.len(), "type": "VEC3"}));
        let uv_flat: Vec<f32> = part.uvs.iter().flatten().copied().collect();
        let uvv = push(floats(&uv_flat), &mut blob);
        accessors.push(json!({"bufferView": uvv, "componentType": 5126, "count": part.uvs.len(), "type": "VEC2"}));
        let iv = push(
            part.indices.iter().flat_map(|i| i.to_le_bytes()).collect(),
            &mut blob,
        );
        accessors.push(json!({"bufferView": iv, "componentType": 5125, "count": part.indices.len(), "type": "SCALAR"}));
        let base = accessors.len() - 4;
        prims.push(json!({
            "attributes": {"POSITION": base, "NORMAL": base + 1, "TEXCOORD_0": base + 2},
            "indices": base + 3,
            "material": materials.len() - 1
        }));
    }
    while blob.len() % 4 != 0 {
        blob.push(0);
    }
    let textures: Vec<Value> = (0..images.len()).map(|i| json!({"source": i})).collect();
    let doc = json!({
        "asset": {"version": "2.0", "generator": "cyberskate-board"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "skateboard", "mesh": 0}],
        "meshes": [{"name": "skateboard", "primitives": prims}],
        "materials": materials,
        "textures": textures,
        "images": images,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": blob.len()}]
    });
    let mut text = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + blob.len();
    let mut out = Vec::with_capacity(total);
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((text.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(text);
    out.extend((blob.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(blob);
    Ok(out)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: cyberskate-board <skate-data/assets> <out/board.glb>");
        std::process::exit(2);
    }
    let skater = Path::new(&args[1]).join("private").join("skater.glb");
    let result = std::fs::read(&skater)
        .map_err(|e| format!("cannot read {}: {e}", skater.display()))
        .and_then(|bytes| export(&bytes))
        .and_then(|(glb, deck)| {
            std::fs::write(&args[2], glb).map_err(|e| format!("cannot write {}: {e}", args[2]))?;
            Ok(deck)
        });
    match result {
        Ok(deck) => println!("Board written to {} (deck bone: {deck})", args[2]),
        Err(e) => {
            println!("ERROR: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheels_end_up_under_the_deck() {
        let part = |name: &str, y: f32| Part {
            positions: vec![[0.1, y, 0.4], [-0.1, y, -0.4]],
            normals: vec![[0., 1., 0.]; 2],
            uvs: vec![[0., 0.]; 2],
            indices: vec![],
            image: 0,
            name: name.into(),
        };
        let mut parts = vec![
            part("Retail_SkateBoard", 0.),
            part("Retail_SkateWheel", 0.06),
        ];
        upright(&mut parts);
        assert!(parts[1].positions[0][1] < parts[0].positions[0][1]);
        assert_eq!(parts[0].normals[0], [0., -1., 0.]);
        assert_eq!(parts[1].positions[0], [-0.1, -0.06, 0.4]);
        upright(&mut parts);
        assert!(parts[1].positions[0][1] < 0.);
    }

    /// A skater with one board quad skinned to bone 1 (bind at y = 2) and
    /// one body triangle on bone 0, through `write` and back.
    #[test]
    fn exports_only_the_board_in_the_deck_frame() {
        let mut blob = Vec::new();
        let mut views = Vec::new();
        let mut put = |bytes: Vec<u8>| {
            views.push(json!({"buffer": 0, "byteOffset": blob.len(), "byteLength": bytes.len()}));
            blob.extend(bytes);
            views.len() - 1
        };
        let f = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let mut ib = [0f32; 32];
        for k in 0..2 {
            for d in 0..4 {
                ib[k * 16 + d * 5] = 1.;
            }
        }
        ib[16 + 13] = -2.; // bone 1's inverse bind moves y = 2 to the origin
        let views_ib = put(f(&ib));
        let quad = put(f(&[0., 2., 0., 1., 2., 0., 1., 2., 1., 0., 2., 1.]));
        let normals = put(f(&[0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0.]));
        let uvs = put(f(&[0.; 8]));
        let joints = put(vec![1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
        let weights = put(f(&[
            1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.,
        ]));
        let idx = put([0u32, 1, 2, 0, 2, 3]
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect());
        let png = put(b"\x89PNG fake".to_vec());
        let acc = |view: usize, count: usize, ty: &str, ct: u32| json!({"bufferView": view, "count": count, "type": ty, "componentType": ct});
        let attrs =
            json!({"POSITION": 1, "NORMAL": 2, "TEXCOORD_0": 3, "JOINTS_0": 4, "WEIGHTS_0": 5});
        let doc = json!({
            "skins": [{"joints": [0, 1], "inverseBindMatrices": 0}],
            "nodes": [{"name": "body"}, {"name": "deck"}],
            "accessors": [acc(views_ib, 2, "MAT4", 5126), acc(quad, 4, "VEC3", 5126), acc(normals, 4, "VEC3", 5126),
                acc(uvs, 4, "VEC2", 5126), acc(joints, 4, "VEC4", 5121), acc(weights, 4, "VEC4", 5126), acc(idx, 6, "SCALAR", 5125)],
            "meshes": [{"primitives": [
                {"attributes": attrs, "indices": 6, "material": 0},
                {"attributes": attrs, "indices": 6, "material": 1}]}],
            "materials": [{"name": "Skateboard", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}, {"name": "Shirt"}],
            "textures": [{"source": 0}],
            "images": [{"bufferView": png, "mimeType": "image/png"}],
            "bufferViews": views,
        });
        let source = Glb {
            json: doc,
            blob: &blob,
        };
        let mut bytes = Vec::new();
        let text = serde_json::to_vec(&source.json).unwrap();
        bytes.extend(b"glTF");
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend((text.len() as u32).to_le_bytes());
        bytes.extend(b"JSON");
        bytes.extend(&text);
        bytes.extend((blob.len() as u32).to_le_bytes());
        bytes.extend(b"BIN\0");
        bytes.extend(&blob);

        let (out, deck) = export(&bytes).unwrap();
        assert_eq!(deck, "deck");
        let back = Glb::parse(&out).unwrap();
        assert_eq!(
            back.json["meshes"][0]["primitives"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let positions = back.components(0).unwrap();
        // The quad sat at y = 2 in the bind pose; in the deck frame it is at
        // the origin, every point finite, and the PNG travelled unchanged.
        assert!(
            positions
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1. + 1e-6)
        );
        assert!(
            positions
                .chunks(3)
                .any(|p| p.iter().all(|v| v.abs() < 1e-6))
        );
        let (png_out, _) = back
            .view(index(&back.json["images"][0]["bufferView"]).unwrap())
            .unwrap();
        assert_eq!(png_out, b"\x89PNG fake");
    }
}
