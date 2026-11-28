//! Writing fracture pieces as one binary glTF (`.glb`) file.
//!
//! A `.glb` is a 12-byte header, then a JSON chunk describing the scene and
//! a BIN chunk holding the vertex data the JSON points into. Each piece is a
//! node of its own (`Piece1`, `Piece2`, …), so a `MeshId` can pick one with
//! `#Piece3`, and loading the whole file gives the original shape.

use std::collections::HashMap;
use std::sync::Arc;

use axcenity_assets::{AlphaMode, Image, ModelMaterial, Primitive};
use glam::{Vec2, Vec3};
use serde_json::{Value, json};

use crate::{Mesh, Surface, position_key};

/// glTF's numbers for its data types and buffer kinds.
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const UNSIGNED_SHORT: u32 = 5123;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// A glTF metallic-roughness material for the pieces.
#[derive(Clone, Debug, PartialEq)]
pub struct GlbMaterial {
    pub name: String,
    /// Linear RGBA (glTF's baseColorFactor).
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    /// alphaMode BLEND when true (see-through), OPAQUE otherwise.
    pub blend: bool,
    /// A texture file's URI, relative to the .glb's folder, for
    /// baseColorTexture (uses TEXCOORD_0), or None.
    pub base_color_texture: Option<String>,
}

/// An image encoded as PNG bytes, embedded directly inside a `.glb`'s binary
/// chunk (for procedural built-in materials, embedded source `.glb` textures,
/// or packed metallic-roughness maps).
#[derive(Clone, Debug)]
pub struct EmbeddedImage {
    pub hash: u64,
    pub png: Arc<Vec<u8>>,
}

impl PartialEq for EmbeddedImage {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.png.len() == other.png.len()
    }
}

impl Eq for EmbeddedImage {}

impl std::hash::Hash for EmbeddedImage {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl EmbeddedImage {
    /// Encodes `rgba` (`width × height × 4` bytes) as PNG and hashes the
    /// result deterministically so identical textures share one buffer view.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Self> {
        use image::ImageEncoder;
        if width == 0 || height == 0 || rgba.len() < (width as usize) * (height as usize) * 4 {
            return None;
        }
        let mut png = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png);
        encoder
            .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
            .ok()?;
        let hash = fnv1a(&png);
        Some(Self {
            hash,
            png: Arc::new(png),
        })
    }
}

/// Deterministic 64-bit FNV-1a hash for deduplicating embedded images.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Embedded PNG textures for a material (used when a texture has no external
/// project file, such as a built-in material or an embedded `.glb` image).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EmbeddedMaps {
    pub base_color: Option<EmbeddedImage>,
    pub metal_rough: Option<EmbeddedImage>,
    pub normal: Option<EmbeddedImage>,
    pub occlusion: Option<EmbeddedImage>,
    pub emissive: Option<EmbeddedImage>,
}

impl EmbeddedMaps {
    pub fn is_empty(&self) -> bool {
        self.base_color.is_none()
            && self.metal_rough.is_none()
            && self.normal.is_none()
            && self.occlusion.is_none()
            && self.emissive.is_none()
    }
}

/// The texture files a material links, by role: paths with `/` between
/// folders, relative to the `.glb` when written (Group as Mesh keeps them
/// relative to the project until then).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MaterialMaps {
    pub base_color: Option<String>,
    /// glTF's one texture of both: metalness in blue, roughness in green.
    pub metal_rough: Option<String>,
    pub normal: Option<String>,
    /// Ambient occlusion, in red.
    pub occlusion: Option<String>,
    pub emissive: Option<String>,
}

impl MaterialMaps {
    /// The files of a loaded model's material's textures (relative to its
    /// model's folder). A texture with no file is left out.
    pub fn of(material: &ModelMaterial) -> MaterialMaps {
        let file = |image: Option<&Arc<Image>>| image.and_then(|image| image.file.clone());
        let metal_rough = [&material.metalness_texture, &material.roughness_texture]
            .into_iter()
            .flatten()
            .map(|channel| &channel.image)
            .next();
        MaterialMaps {
            base_color: file(material.base_color_texture.as_ref()),
            metal_rough: file(metal_rough),
            normal: file(material.normal_texture.as_ref()),
            occlusion: file(material.occlusion_texture.as_ref().map(|c| &c.image)),
            emissive: file(material.emissive_texture.as_ref()),
        }
    }

    /// True when it links no texture.
    pub fn is_empty(&self) -> bool {
        self.all().all(|map| map.is_none())
    }

    fn all(&self) -> impl Iterator<Item = &Option<String>> {
        [
            &self.base_color,
            &self.metal_rough,
            &self.normal,
            &self.occlusion,
            &self.emissive,
        ]
        .into_iter()
    }

    /// Every file it links.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.all().filter_map(|map| map.as_deref())
    }

    /// Each file changed by `change`.
    pub fn map(&self, change: impl Fn(&str) -> String) -> MaterialMaps {
        let each = |map: &Option<String>| map.as_deref().map(&change);
        MaterialMaps {
            base_color: each(&self.base_color),
            metal_rough: each(&self.metal_rough),
            normal: each(&self.normal),
            occlusion: each(&self.occlusion),
            emissive: each(&self.emissive),
        }
    }
}

/// A .glb with one node per piece named `Piece1`, `Piece2`, … (in order),
/// each with its own mesh: one primitive per Outer material it uses
/// (`materials[i]`) and one for its Interior faces (`interior`). Positions
/// are where the piece is (nodes have no transform), so loading the whole
/// file gives the original shape. Interior faces get flat normals of their
/// plane and planar uvs.
pub fn write_glb(pieces: &[Mesh], materials: &[GlbMaterial], interior: &GlbMaterial) -> Vec<u8> {
    let mut file = File::default();
    for material in materials {
        file.add_material(material);
    }
    let interior_material = file.add_material(interior);
    // An Outer surface without a material is drawn plain white.
    let mut plain = None;

    let mut nodes = Vec::new();
    let mut meshes = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        let name = format!("Piece{}", index + 1);
        // Outer surfaces in material order, then the inside.
        let mut surfaces: Vec<Surface> = piece.triangles.iter().map(|t| t.surface).collect();
        surfaces.sort_by_key(|surface| match surface {
            Surface::Outer(material) => (0, *material),
            Surface::Interior => (1, 0),
        });
        surfaces.dedup();
        if surfaces.is_empty() {
            // glTF needs a mesh to have a primitive; an empty piece is a
            // node with nothing in it.
            nodes.push(json!({ "name": name }));
            continue;
        }
        let primitives: Vec<Value> = surfaces
            .into_iter()
            .map(|surface| {
                let material = match surface {
                    Surface::Interior => interior_material,
                    Surface::Outer(material) if (material as usize) < materials.len() => {
                        material as usize
                    }
                    Surface::Outer(_) => {
                        *plain.get_or_insert_with(|| file.add_material(&plain_material()))
                    }
                };
                file.add_primitive(&surface_triangles(piece, surface), material)
            })
            .collect();
        nodes.push(json!({ "name": name, "mesh": meshes.len() }));
        meshes.push(json!({ "name": name, "primitives": primitives }));
    }
    let roots = (0..nodes.len()).collect();
    file.finish(nodes, meshes, roots)
}

/// A .glb of named objects, one node each (no transform) with a primitive
/// per entry, in the model's own units: what a generator makes (a tree's
/// `Tree` with its `Tree_LOD1` and `Tree_LOD2` copies, which the loader
/// takes as levels of detail by material). Each material is written once
/// by name, with its wind and leaf values in its `extras` and the textures
/// linked from `maps`, by material name (files relative to the `.glb`).
pub fn write_objects_glb(
    objects: &[(String, Vec<Primitive>)],
    maps: &[(String, MaterialMaps)],
) -> Vec<u8> {
    let mut file = File::default();
    let mut material_of: HashMap<String, usize> = HashMap::new();
    let mut nodes = Vec::new();
    let mut meshes = Vec::new();
    for (name, primitives) in objects {
        let primitives: Vec<Value> = primitives
            .iter()
            .filter(|primitive| !primitive.indices.is_empty())
            .map(|primitive| {
                let material = *material_of
                    .entry(primitive.material.name.clone())
                    .or_insert_with(|| {
                        let none = MaterialMaps::default();
                        let maps = maps
                            .iter()
                            .find(|(name, _)| *name == primitive.material.name)
                            .map_or(&none, |(_, maps)| maps);
                        file.add_material_with(&primitive.material, maps)
                    });
                let vertices = Vertices {
                    positions: primitive.positions.iter().map(|&p| Vec3::from(p)).collect(),
                    normals: primitive.normals.iter().map(|&n| Vec3::from(n)).collect(),
                    uvs: primitive.uvs.iter().map(|&uv| Vec2::from(uv)).collect(),
                    indices: primitive.indices.clone(),
                    colors: primitive.colors.clone(),
                    ..Vertices::default()
                };
                file.add_primitive(&vertices, material)
            })
            .collect();
        if primitives.is_empty() {
            nodes.push(json!({ "name": name }));
            continue;
        }
        nodes.push(json!({ "name": name, "mesh": meshes.len() }));
        meshes.push(json!({ "name": name, "primitives": primitives }));
    }
    let roots = (0..nodes.len()).collect();
    file.finish(nodes, meshes, roots)
}

/// A primitive's vertices and triangles, ready to write. Without `uvs`,
/// it's written without texture coordinates; `uvs2` (the second set,
/// `TEXCOORD_1`) may be empty, and so may `joints` and `weights` (a
/// skinned mesh's `JOINTS_0` and `WEIGHTS_0`), and `colors` (`COLOR_0`,
/// linear RGBA).
#[derive(Default)]
pub(crate) struct Vertices {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub uvs2: Vec<Vec2>,
    pub indices: Vec<u32>,
    /// The mesh vertex each one is a corner of (not written).
    pub vertex: Vec<u32>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub colors: Vec<[f32; 4]>,
}

/// The triangles of `piece` on `surface`, with only the vertices they use.
/// Interior triangles get their plane's normal and uvs across it.
pub(crate) fn surface_triangles(piece: &Mesh, surface: Surface) -> Vertices {
    let mut vertices = Vertices::default();
    // Vertices already written, by the original index (Outer) or by
    // position and normal (Interior).
    let mut by_index: HashMap<u32, u32> = HashMap::new();
    let mut by_place: HashMap<([u32; 3], [u32; 3]), u32> = HashMap::new();
    for triangle in piece.triangles.iter().filter(|t| t.surface == surface) {
        let [a, b, c] = triangle
            .corners
            .map(|corner| piece.positions[corner as usize]);
        // A cap's triangles carry their plane's normal, exactly the same on
        // each, so their corners are shared. Thin triangles' own normals
        // wobble; theirs is only used when the carried one is missing or
        // plainly wrong.
        let carried = piece.normals[triangle.corners[0] as usize].normalize_or_zero();
        let flat = match (b - a).cross(c - a).try_normalize() {
            Some(own) if carried.dot(own) < 0.9 => own,
            _ => carried.normalize_or(Vec3::Y),
        };
        for corner in triangle.corners {
            let index = match surface {
                Surface::Outer(_) => *by_index.entry(corner).or_insert_with(|| {
                    vertices.push(
                        piece.positions[corner as usize],
                        piece.normals[corner as usize],
                        piece.uvs[corner as usize],
                    )
                }),
                Surface::Interior => {
                    let position = piece.positions[corner as usize];
                    let key = (position_key(position), position_key(flat));
                    *by_place.entry(key).or_insert_with(|| {
                        let u = flat.any_orthonormal_vector();
                        let v = flat.cross(u);
                        let uv = Vec2::new(position.dot(u), position.dot(v));
                        vertices.push(position, flat, uv)
                    })
                }
            };
            vertices.indices.push(index);
        }
    }
    vertices
}

impl Vertices {
    fn push(&mut self, position: Vec3, normal: Vec3, uv: Vec2) -> u32 {
        self.positions.push(position);
        self.normals.push(normal);
        self.uvs.push(uv);
        (self.positions.len() - 1) as u32
    }
}

/// A plain white surface, for an Outer material index with no material.
pub(crate) fn plain_material() -> GlbMaterial {
    GlbMaterial {
        name: "Plain".to_owned(),
        base_color: [1.0; 4],
        metallic: 0.0,
        roughness: 0.5,
        emissive: [0.0; 3],
        blend: false,
        base_color_texture: None,
    }
}

/// The glTF document being built, and its binary data.
#[derive(Default)]
pub(crate) struct File {
    bin: Vec<u8>,
    buffer_views: Vec<Value>,
    accessors: Vec<Value>,
    materials: Vec<Value>,
    images: Vec<Value>,
    textures: Vec<Value>,
    /// The texture already made for each image URI.
    texture_of: HashMap<String, usize>,
    /// The texture already made for each embedded PNG (by hash).
    embedded_of: HashMap<u64, usize>,
    /// Some texture is placed with `KHR_texture_transform`.
    transforms: bool,
}

impl File {
    /// The texture showing the image file at `uri`, made the first time.
    fn texture(&mut self, uri: &str) -> usize {
        if let Some(&texture) = self.texture_of.get(uri) {
            return texture;
        }
        self.images.push(json!({ "uri": encode_uri(uri) }));
        self.textures
            .push(json!({ "source": self.images.len() - 1 }));
        let texture = self.textures.len() - 1;
        self.texture_of.insert(uri.to_owned(), texture);
        texture
    }

    /// The texture showing an embedded PNG image, stored in the `.glb`'s
    /// binary chunk and deduplicated by its content hash.
    fn texture_embedded(&mut self, image: &EmbeddedImage) -> usize {
        if let Some(&texture) = self.embedded_of.get(&image.hash) {
            return texture;
        }
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let start = self.bin.len();
        self.bin.extend_from_slice(&image.png);
        let len = image.png.len();
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        self.buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": start,
            "byteLength": len,
        }));
        let view = self.buffer_views.len() - 1;
        self.images.push(json!({
            "bufferView": view,
            "mimeType": "image/png",
        }));
        self.textures
            .push(json!({ "source": self.images.len() - 1 }));
        let texture = self.textures.len() - 1;
        self.embedded_of.insert(image.hash, texture);
        texture
    }

    /// Adds `material` and returns its index.
    fn add_material(&mut self, material: &GlbMaterial) -> usize {
        let mut pbr = json!({
            "baseColorFactor": material.base_color.map(finite),
            "metallicFactor": finite(material.metallic).clamp(0.0, 1.0),
            "roughnessFactor": finite(material.roughness).clamp(0.0, 1.0),
        });
        if let Some(uri) = &material.base_color_texture {
            pbr["baseColorTexture"] = json!({ "index": self.texture(uri) });
        }
        self.materials.push(json!({
            "name": material.name,
            "pbrMetallicRoughness": pbr,
            "emissiveFactor": material.emissive.map(|c| finite(c).clamp(0.0, 1.0)),
            "alphaMode": if material.blend { "BLEND" } else { "OPAQUE" },
        }));
        self.materials.len() - 1
    }

    /// Adds a loaded model's `material` and returns its index: its colors,
    /// alpha and sides, and its textures linked by their files (a texture
    /// with no file is left out). Metalness and roughness are written as
    /// glTF's one texture of both.
    pub(crate) fn add_model_material(&mut self, material: &ModelMaterial) -> usize {
        self.add_material_with(material, &MaterialMaps::of(material))
    }

    /// Adds `material`'s values (its own textures aren't looked at) with
    /// the texture files `maps` names, and returns its index. How it moves
    /// in the wind is kept in its `extras`, as the loader reads it.
    pub(crate) fn add_material_with(
        &mut self,
        material: &ModelMaterial,
        maps: &MaterialMaps,
    ) -> usize {
        self.add_material_with_embedded(material, maps, &EmbeddedMaps::default())
    }

    /// [`add_material_with`](Self::add_material_with), also accepting embedded
    /// PNG textures when a slot has no external URI.
    pub(crate) fn add_material_with_embedded(
        &mut self,
        material: &ModelMaterial,
        maps: &MaterialMaps,
        embedded: &EmbeddedMaps,
    ) -> usize {
        let has_any_map = !maps.is_empty() || !embedded.is_empty();
        let transform = (material.uv_scale != [1.0, 1.0]
            || material.uv_offset != [0.0, 0.0]
            || material.uv_rotation != 0.0)
            .then(|| {
                json!({
                    "offset": material.uv_offset.map(finite),
                    "rotation": finite(material.uv_rotation),
                    "scale": material.uv_scale.map(finite),
                })
            });
        self.transforms |= transform.is_some() && has_any_map;
        let info = |file: &mut Self, uri: &Option<String>, embed: &Option<EmbeddedImage>| {
            let index = if let Some(uri) = uri.as_deref() {
                file.texture(uri)
            } else {
                let img = embed.as_ref()?;
                file.texture_embedded(img)
            };
            let mut info = json!({ "index": index });
            if let Some(transform) = &transform {
                info["extensions"] = json!({ "KHR_texture_transform": transform });
            }
            Some(info)
        };
        let mut pbr = json!({
            "baseColorFactor": material.base_color.map(finite),
            "metallicFactor": finite(material.metalness).clamp(0.0, 1.0),
            "roughnessFactor": finite(material.roughness).clamp(0.0, 1.0),
        });
        if let Some(texture) = info(self, &maps.base_color, &embedded.base_color) {
            pbr["baseColorTexture"] = texture;
        }
        if let Some(texture) = info(self, &maps.metal_rough, &embedded.metal_rough) {
            pbr["metallicRoughnessTexture"] = texture;
        }
        let mut json = json!({
            "name": material.name,
            "pbrMetallicRoughness": pbr,
            "emissiveFactor": material.emissive.map(|c| finite(c).clamp(0.0, 1.0)),
            "doubleSided": material.double_sided,
        });
        match material.alpha_mode {
            AlphaMode::Opaque => json["alphaMode"] = json!("OPAQUE"),
            AlphaMode::Mask { cutoff } => {
                json["alphaMode"] = json!("MASK");
                json["alphaCutoff"] = json!(finite(cutoff));
            }
            AlphaMode::Blend => json["alphaMode"] = json!("BLEND"),
        }
        if let Some(mut texture) = info(self, &maps.normal, &embedded.normal) {
            texture["scale"] = json!(finite(material.normal_scale));
            json["normalTexture"] = texture;
        }
        if let Some(mut texture) = info(self, &maps.occlusion, &embedded.occlusion) {
            texture["strength"] = json!(finite(material.occlusion_strength));
            json["occlusionTexture"] = texture;
        }
        if let Some(texture) = info(self, &maps.emissive, &embedded.emissive) {
            json["emissiveTexture"] = texture;
        }
        // A leaf (one that flutters) is see-through to light unless told
        // otherwise, so its translucency is written whenever it flutters.
        if material.sway > 0.0 || material.flutter > 0.0 || material.translucency > 0.0 {
            let mut extras = serde_json::Map::new();
            if material.sway > 0.0 {
                extras.insert("axcenity_sway".into(), json!(finite(material.sway)));
            }
            if material.flutter > 0.0 {
                extras.insert("axcenity_flutter".into(), json!(finite(material.flutter)));
            }
            if material.flutter > 0.0 || material.translucency > 0.0 {
                extras.insert(
                    "axcenity_translucency".into(),
                    json!(finite(material.translucency)),
                );
            }
            json["extras"] = Value::Object(extras);
        }
        self.materials.push(json);
        self.materials.len() - 1
    }

    /// Writes a primitive's data and returns its JSON.
    pub(crate) fn add_primitive(&mut self, vertices: &Vertices, material: usize) -> Value {
        let (min, max) = vertices
            .positions
            .iter()
            .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), &p| {
                (min.min(p), max.max(p))
            });
        let positions: Vec<f32> = vertices.positions.iter().flat_map(Vec3::to_array).collect();
        let normals: Vec<f32> = vertices.normals.iter().flat_map(Vec3::to_array).collect();
        let uvs: Vec<f32> = vertices.uvs.iter().flat_map(Vec2::to_array).collect();
        let count = vertices.positions.len();
        let position = self.add_accessor(&positions, count, "VEC3", ARRAY_BUFFER);
        self.accessors[position]["min"] = json!(min.to_array().map(finite));
        self.accessors[position]["max"] = json!(max.to_array().map(finite));
        let normal = self.add_accessor(&normals, count, "VEC3", ARRAY_BUFFER);
        let uv = (!uvs.is_empty()).then(|| self.add_accessor(&uvs, count, "VEC2", ARRAY_BUFFER));
        let uvs2: Vec<f32> = vertices.uvs2.iter().flat_map(Vec2::to_array).collect();
        let uv2 = (uvs2.len() == count * 2)
            .then(|| self.add_accessor(&uvs2, count, "VEC2", ARRAY_BUFFER));
        let skinned = vertices.joints.len() == count && vertices.weights.len() == count;
        let joints = skinned.then(|| {
            let view = self.add_view(
                vertices
                    .joints
                    .iter()
                    .flatten()
                    .flat_map(|joint| joint.to_le_bytes()),
                ARRAY_BUFFER,
            );
            self.accessors.push(json!({
                "bufferView": view,
                "componentType": UNSIGNED_SHORT,
                "count": count,
                "type": "VEC4",
            }));
            self.accessors.len() - 1
        });
        let weights = skinned.then(|| {
            let flat: Vec<f32> = vertices.weights.iter().flatten().copied().collect();
            self.add_accessor(&flat, count, "VEC4", ARRAY_BUFFER)
        });
        let colors = (vertices.colors.len() == count).then(|| {
            let flat: Vec<f32> = vertices.colors.iter().flatten().copied().collect();
            self.add_accessor(&flat, count, "VEC4", ARRAY_BUFFER)
        });
        let indices = self.add_view(
            vertices.indices.iter().flat_map(|i| i.to_le_bytes()),
            ELEMENT_ARRAY_BUFFER,
        );
        self.accessors.push(json!({
            "bufferView": indices,
            "componentType": UNSIGNED_INT,
            "count": vertices.indices.len(),
            "type": "SCALAR",
        }));
        let mut attributes = json!({ "POSITION": position, "NORMAL": normal });
        if let Some(uv) = uv {
            attributes["TEXCOORD_0"] = json!(uv);
        }
        if let Some(uv2) = uv2 {
            attributes["TEXCOORD_1"] = json!(uv2);
        }
        if let (Some(joints), Some(weights)) = (joints, weights) {
            attributes["JOINTS_0"] = json!(joints);
            attributes["WEIGHTS_0"] = json!(weights);
        }
        if let Some(colors) = colors {
            attributes["COLOR_0"] = json!(colors);
        }
        json!({
            "attributes": attributes,
            "indices": self.accessors.len() - 1,
            "material": material,
        })
    }

    /// Writes `values` (`count` items of `kind`, such as "VEC3") and
    /// returns the accessor that reads them.
    pub(crate) fn add_accessor(
        &mut self,
        values: &[f32],
        count: usize,
        kind: &str,
        target: u32,
    ) -> usize {
        let view = self.add_view(
            values.iter().flat_map(|value| finite(*value).to_le_bytes()),
            target,
        );
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": FLOAT,
            "count": count,
            "type": kind,
        }));
        self.accessors.len() - 1
    }

    /// Adds `bytes` to the binary data and returns the view onto them.
    /// Every value is 4 bytes, so views stay 4-byte aligned.
    fn add_view(&mut self, bytes: impl Iterator<Item = u8>, target: u32) -> usize {
        let start = self.bin.len();
        self.bin.extend(bytes);
        self.buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": start,
            "byteLength": self.bin.len() - start,
            "target": target,
        }));
        self.buffer_views.len() - 1
    }

    /// Writes `matrices` (column by column) as inverse bind matrices,
    /// and returns their accessor. They aren't vertex data, so their view
    /// has no target.
    pub(crate) fn add_matrices(&mut self, matrices: &[glam::Mat4]) -> usize {
        let start = self.bin.len();
        let values = matrices.iter().flat_map(|m| m.to_cols_array());
        self.bin
            .extend(values.flat_map(|value| finite(value).to_le_bytes()));
        self.buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": start,
            "byteLength": self.bin.len() - start,
        }));
        self.accessors.push(json!({
            "bufferView": self.buffer_views.len() - 1,
            "componentType": FLOAT,
            "count": matrices.len(),
            "type": "MAT4",
        }));
        self.accessors.len() - 1
    }

    /// The finished file: header, JSON chunk and BIN chunk. `roots` are
    /// the nodes at the top of the scene; the rest are their `children`.
    pub(crate) fn finish(
        self,
        nodes: Vec<Value>,
        meshes: Vec<Value>,
        roots: Vec<usize>,
    ) -> Vec<u8> {
        self.finish_with(nodes, meshes, roots, Vec::new())
    }

    /// [`finish`](Self::finish), with `skins`.
    pub(crate) fn finish_with(
        self,
        nodes: Vec<Value>,
        meshes: Vec<Value>,
        roots: Vec<usize>,
        skins: Vec<Value>,
    ) -> Vec<u8> {
        let mut root = json!({
            "asset": { "version": "2.0", "generator": "Axcenity Engine" },
            "scene": 0,
            "scenes": [{ "nodes": roots }],
            "nodes": nodes,
            "meshes": meshes,
            "materials": self.materials,
            "accessors": self.accessors,
            "bufferViews": self.buffer_views,
        });
        if !self.bin.is_empty() {
            root["buffers"] = json!([{ "byteLength": self.bin.len() }]);
        }
        if !self.images.is_empty() {
            root["images"] = json!(self.images);
            root["textures"] = json!(self.textures);
        }
        if self.transforms {
            root["extensionsUsed"] = json!(["KHR_texture_transform"]);
        }
        if !skins.is_empty() {
            root["skins"] = json!(skins);
        }
        let mut json = serde_json::to_vec(&root).expect("glTF JSON is plain data");
        // Chunks are padded to 4 bytes: JSON with spaces, BIN with zeros.
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut bin = self.bin;
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let mut total = 12 + 8 + json.len();
        if !bin.is_empty() {
            total += 8 + bin.len();
        }

        let mut glb = Vec::with_capacity(total);
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&json);
        if !bin.is_empty() {
            glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
            glb.extend_from_slice(b"BIN\0");
            glb.extend_from_slice(&bin);
        }
        glb
    }
}

/// `value`, or 0 if it isn't a number JSON can hold.
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// `path` as a URI: anything but letters, digits, `-`, `_`, `.`, `~` and
/// `/` written as `%XX`, so a file name with spaces still works.
fn encode_uri(path: &str) -> String {
    let mut uri = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}
