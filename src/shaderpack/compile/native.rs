//! Native programs: the built-in pack's `<program>.slang`, written in Slang
//! against this engine instead of in GLSL for Iris, and compiled to SPIR-V
//! by build.rs (`compile_torch_pack`) behind the engine's prelude
//! (`builtin/iris`). Nothing is translated or compiled at run time. The
//! file names the uniforms it reads as `frame.<name>`, samples targets by
//! their Iris names (`colortex0`, `depthtex1`, `shadowtex0HW`, ...), and
//! reads a `bufferObject.<n>` through `IRIS_BUFFER_<n>`. The engine scans
//! the file for those as it would a translated program's, so the layout,
//! targets, flips and uniforms are shared; what it finds becomes WGSL
//! declarations (the slot manifest `vk::pack::bindless` reads), and the
//! module's specialization constants take the values: options, uniform
//! offsets, the alpha test, and later each resource's slot.
//!
//! Entry points are `vertex(IrisVertexIn input)` and `fragment(...)` for
//! drawing programs, `compute(...)` for compute ones. A file with a
//! `[numthreads(...)]` entry is a compute program. The prelude gives every
//! drawing program `iris_attributes(input)`, the vertex in the kind's model
//! space (camera-relative for terrain and entities, view space for the
//! hand), `iris_clip` for the clip-space conventions, `iris_alpha_pass` for
//! the alpha test of the layer drawn, and `iris_output` for the final pass.
//! Iris's text directives (`RENDERTARGETS`, `const int colortex0Format`,
//! `const ivec3 workGroups`, ...) are read from the file's comments.

use super::pack::Pack;
use super::program::{texture_sources, undeclared_buffer};
use super::{AlphaTest, Dispatch, PACK_SET, Precompiled, Program, ProgramKind};
use crate::shaderpack::directives;
use crate::shaderpack::pipeline::{self, Layout};
use crate::shaderpack::preprocess::Defines;
use crate::shaderpack::transform::declarations::{PackUniform, std140_size};
use crate::shaderpack::transform::lexer::{self, Kind};
use crate::shaderpack::transform::{Interface, Sampler, StorageBuffer, UNIFORM_BINDING, UNIFORMS};

pub(super) const EXTENSION: &str = ".slang";

pub(super) fn is_compute(text: &str) -> bool {
    text.contains("[numthreads(")
}

#[cfg(feature = "vulkan")]
mod modules {
    include!(concat!(env!("OUT_DIR"), "/torch_pack/modules.rs"));
}

/// Built without the native renderer: no modules, so no native program
/// compiles.
#[cfg(not(feature = "vulkan"))]
mod modules {
    pub(crate) const MODULES: &[(&str, &[u8])] = &[];
}

/// The input a program of `kind` is built for (build.rs names modules by it).
fn input(kind: ProgramKind) -> &'static str {
    match kind {
        ProgramKind::Geometry | ProgramKind::Shadow => "terrain",
        ProgramKind::Entity | ProgramKind::ShadowEntity => "entity",
        ProgramKind::Screen => "screen",
        ProgramKind::Compute => "compute",
    }
}

/// `program`'s module for `input` and `stage`, as built.
fn module(program: &str, input: &str, stage: &str) -> Result<Vec<u32>, String> {
    let key = format!("{program}.{input}.{stage}");
    let (_, bytes) = modules::MODULES
        .iter()
        .find(|(k, _)| *k == key)
        .ok_or_else(|| {
            format!("{key}: not built into this client (only the built-in pack is native)")
        })?;
    crate::shaderpack::spirv::words(bytes).map_err(|e| format!("{key}: {e}"))
}

/// The prelude's own resources for a kind, as `iris/terrain.slang` and
/// `iris/entity.slang` read them, so their slots are laid out. A terrain
/// section's pools are read by the bindless index its slot holds, so they
/// take no slot.
fn kind_slots(kind: ProgramKind) -> &'static str {
    match kind {
        ProgramKind::Geometry | ProgramKind::Shadow => {
            "@group(2) @binding(7) var<storage, read> iris_vk_slots: IrisVkSlots;\n\
             @group(2) @binding(12) var<storage, read> iris_vk_anim: IrisVkAnim;\n\
             @group(2) @binding(14) var<storage, read> iris_vk_voxels: IrisVkVoxels;\n"
        }
        ProgramKind::Entity | ProgramKind::ShadowEntity => {
            "@group(2) @binding(10) var<storage, read> iris_vk_inst: IrisVkInstances;\n\
             @group(2) @binding(11) var<storage, read> iris_vk_mesh: IrisVkMeshes;\n\
             @group(2) @binding(12) var<storage, read> iris_vk_vert: IrisVkVertices;\n\
             @group(2) @binding(13) var<storage, read> iris_vk_part: IrisVkParts;\n\
             @group(2) @binding(14) var<storage, read> iris_vk_voxels: IrisVkVoxels;\n"
        }
        ProgramKind::Screen | ProgramKind::Compute => {
            "@group(2) @binding(14) var<storage, read> iris_vk_voxels: IrisVkVoxels;\n"
        }
    }
}

/// What the file reads, declared for it.
struct Declarations {
    interface: Interface,
    header: String,
}

fn wgsl_type(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "float" => "f32",
        "int" | "bool" => "i32",
        "uint" => "u32",
        "vec2" => "vec2<f32>",
        "vec3" => "vec3<f32>",
        "vec4" => "vec4<f32>",
        "ivec2" => "vec2<i32>",
        "ivec3" => "vec3<i32>",
        "ivec4" => "vec4<i32>",
        "uvec2" => "vec2<u32>",
        "uvec3" => "vec3<u32>",
        "uvec4" => "vec4<u32>",
        "mat3" => "mat3x3<f32>",
        "mat4" => "mat4x4<f32>",
        _ => return None,
    })
}

/// A sampler name a native program may read. Iris's legacy aliases (`gcolor`,
/// `composite`, `shadow`, `tex`, ...) are left out: in Slang they are as likely
/// to be a variable's name.
fn sampled(pack: &Pack, program: &str, name: &str) -> Option<&'static str> {
    let numbered = |prefix: &str, count: usize| {
        name.strip_prefix(prefix)
            .and_then(|n| n.parse::<usize>().ok())
            .is_some_and(|n| n < count && name.len() == prefix.len() + n.to_string().len())
    };
    if matches!(name, "shadowtex0HW" | "shadowtex1HW") {
        return Some("sampler2DShadow");
    }
    if let Some(index) = pack.images.lookup(program, name) {
        return Some(if pack.images.textures[usize::from(index)].three_d {
            "sampler3D"
        } else {
            "sampler2D"
        });
    }
    let plain = numbered("colortex", directives::MAX_COLOR_TARGETS)
        || numbered("shadowcolor", directives::MAX_SHADOW_COLORS)
        || matches!(
            name,
            "depthtex0"
                | "depthtex1"
                | "depthtex2"
                | "shadowtex0"
                | "shadowtex1"
                | "noisetex"
                | "gtexture"
                | "lightmap"
                | "normals"
                | "specular"
        );
    plain.then_some("sampler2D")
}

fn declare(pack: &Pack, program: &str, text: &str, outputs: u32) -> Result<Declarations, String> {
    let tokens: Vec<_> = lexer::lex(text)
        .into_iter()
        .filter(|t| !matches!(t.kind, Kind::Comment | Kind::Whitespace))
        .collect();

    let mut uniforms: Vec<PackUniform> = Vec::new();
    let mut samplers: Vec<(String, &'static str)> = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Word {
            continue;
        }
        let member = (token.text == "frame" && tokens.get(at + 1).is_some_and(|t| t.is_punct('.')))
            .then(|| tokens.get(at + 2))
            .flatten()
            .filter(|t| t.kind == Kind::Word);
        if let Some(member) = member {
            let name = member.text.as_str();
            if UNIFORMS.iter().any(|(n, _)| *n == name) || uniforms.iter().any(|u| u.name == name) {
                continue;
            }
            let ty = crate::shaderpack::uniforms::builtin_type(name)
                .ok_or_else(|| format!("frame.{name} is not a uniform this client supplies"))?;
            uniforms.push(PackUniform {
                name: name.to_owned(),
                ty: ty.to_owned(),
                array: None,
                initial: None,
            });
            continue;
        }
        if at > 0 && tokens[at - 1].is_punct('.') {
            continue;
        }
        if let Some(ty) = sampled(pack, program, &token.text)
            && !samplers.iter().any(|(n, _)| *n == token.text)
        {
            samplers.push((token.text.clone(), ty));
        }
    }

    let mut header = String::from("struct IrisFrame {\n");
    for (name, ty) in UNIFORMS {
        let wgsl = wgsl_type(ty).expect("the engine's own members have a WGSL type");
        header.push_str(&format!("    {name}: {wgsl},\n"));
    }
    for uniform in &uniforms {
        let wgsl = wgsl_type(&uniform.ty)
            .ok_or_else(|| format!("frame.{}: no WGSL type for {}", uniform.name, uniform.ty))?;
        let size = std140_size(&uniform.ty).expect("typed above");
        header.push_str(&format!("    @size({size}) {}: {wgsl},\n", uniform.name));
    }
    header.push_str(&format!(
        "}}\n@group({PACK_SET}) @binding({UNIFORM_BINDING}) var<uniform> frame: IrisFrame;\n"
    ));

    let mut interface_samplers = Vec::with_capacity(samplers.len());
    for (index, (name, ty)) in samplers.iter().enumerate() {
        let texture_binding = UNIFORM_BINDING + 1 + 2 * index as u32;
        let sampler_binding = texture_binding + 1;
        let (texture, sampler) = match *ty {
            "sampler2DShadow" => ("texture_depth_2d", "sampler_comparison"),
            "sampler3D" => ("texture_3d<f32>", "sampler"),
            _ => ("texture_2d<f32>", "sampler"),
        };
        header.push_str(&format!(
            "@group({PACK_SET}) @binding({texture_binding}) var {name}: {texture};\n\
             @group({PACK_SET}) @binding({sampler_binding}) var {name}_sampler: {sampler};\n"
        ));
        interface_samplers.push(Sampler {
            name: name.clone(),
            ty: (*ty).to_owned(),
            texture: name.clone(),
            texture_binding,
            sampler_binding,
        });
    }

    let mut buffers = Vec::new();
    let mut next = UNIFORM_BINDING + 1 + 2 * samplers.len() as u32;
    for line in text.lines() {
        let Some(at) = line.find("IRIS_BUFFER_") else {
            continue;
        };
        let digits: String = line[at + "IRIS_BUFFER_".len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let index: u8 = digits
            .parse()
            .map_err(|_| format!("{line}: IRIS_BUFFER_ needs a buffer index"))?;
        if buffers.iter().any(|b: &StorageBuffer| b.index == index) {
            return Err(format!("bufferObject.{index} is bound twice"));
        }
        let read_only = !line.contains("read_write");
        let access = if read_only { "read" } else { "read_write" };
        header.push_str(&format!(
            "@group({PACK_SET}) @binding({next}) var<storage, {access}> iris_buffer_{index}: array<u32>;\n"
        ));
        buffers.push(StorageBuffer {
            index,
            binding: next,
            read_only,
        });
        next += 1;
    }

    Ok(Declarations {
        interface: Interface {
            samplers: interface_samplers,
            attributes: Vec::new(),
            outputs,
            uniforms,
            images: Vec::new(),
            buffers,
        },
        header,
    })
}

/// What a stage's specialization constants take, but its slots (those are
/// the pass's, `vk::pack::bindless::bind_precompiled`).
#[derive(Clone, Copy)]
struct Values<'a> {
    /// The program's defines after preprocessing: its options as chosen,
    /// and the engine's (`MC_RENDER_STAGE_*`, ...).
    defines: &'a Defines,
    layout: &'a Layout,
    upright: bool,
    linear: bool,
    alpha: Option<AlphaTest>,
}

impl Values<'_> {
    fn get(&self, name: &str) -> Option<f64> {
        if let Some(uniform) = name.strip_prefix("IRIS_OFF_") {
            return self.layout.uniforms.offset(uniform).map(f64::from);
        }
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        match name {
            "IRIS_UPRIGHT" => Some(flag(self.upright)),
            "IRIS_LINEAR_OUTPUT" => Some(flag(self.linear)),
            "IRIS_ALPHA_FUNC" => Some(match self.alpha {
                None => 0.0,
                Some(test) => match test.function.operator() {
                    "<" => 1.0,
                    "==" => 2.0,
                    "<=" => 3.0,
                    ">" => 4.0,
                    "!=" => 5.0,
                    _ => 6.0,
                },
            }),
            "IRIS_ALPHA_REF" => Some(
                self.alpha
                    .map_or(-1.0, |t| f64::from(t.wrapper_reference())),
            ),
            _ => self.defines.get(name)?.trim().parse().ok(),
        }
    }

    /// `words` with every constant but the slots set.
    fn apply(&self, mut words: Vec<u32>, label: &str) -> Result<Vec<u32>, String> {
        let missing: Vec<String> =
            crate::shaderpack::spirv::specialize(&mut words, |n| self.get(n))
                .into_iter()
                .filter(|n| !n.starts_with("IRIS_SLOT_"))
                .collect();
        if missing.is_empty() {
            Ok(words)
        } else {
            Err(format!("{label}: no value for {}", missing.join(", ")))
        }
    }
}

pub(super) fn compile_draw(
    pack: &mut Pack,
    name: &str,
    kind: ProgramKind,
    tests: &[AlphaTest],
    _direct: bool,
) -> Result<(Program, [String; 2]), String> {
    let path = format!("{}{name}{EXTENSION}", pack.folder);
    let (text, defines) = pack.stage_with_defines(&path)?;
    let is_final = name == "final";
    let draw_buffers = if is_final {
        vec![0]
    } else {
        directives::draw_buffers(&text).map_err(|e| format!("{name}: {e}"))?
    };
    if kind.is_shadow()
        && let Some(past) = draw_buffers
            .iter()
            .find(|b| usize::from(**b) >= directives::MAX_SHADOW_COLORS)
    {
        return Err(format!(
            "{name} draws to shadowcolor{past}, past the last shadow colour target"
        ));
    }
    let declared = declare(pack, name, &text, draw_buffers.len() as u32)
        .map_err(|e| format!("{name}: {e}"))?;
    let layout: Layout = pipeline::describe(&declared.interface);
    undeclared_buffer(pack, name, &layout)?;
    let textures = texture_sources(pack, name, kind, &layout)?;
    let manifest = format!("{}{}", declared.header, kind_slots(kind));

    let input = input(kind);
    let vertex_words = module(name, input, "vertex")?;
    let fragment_words = module(name, input, "fragment")?;
    let plain = Values {
        defines: &defines,
        layout: &layout,
        upright: is_final,
        linear: false,
        alpha: None,
    };
    let label = |stage: &str| format!("{name}.{input}.{stage}");
    let vertex = plain.apply(vertex_words, &label("vertex"))?;
    let fragment = plain.apply(fragment_words.clone(), &label("fragment"))?;
    let fragment_linear = if is_final {
        let linear = Values {
            linear: true,
            ..plain
        };
        Some(linear.apply(fragment_words.clone(), &label("fragment"))?)
    } else {
        None
    };
    let mut fragment_tests: Vec<(AlphaTest, Vec<u32>)> = Vec::with_capacity(tests.len());
    for test in tests {
        if fragment_tests.iter().any(|(t, _)| t == test) {
            continue;
        }
        let tested = Values {
            alpha: Some(*test),
            ..plain
        };
        fragment_tests.push((
            *test,
            tested.apply(fragment_words.clone(), &label("fragment"))?,
        ));
    }
    let mipmapped = directives::mipmapped([text.as_str()]);
    let precompiled = Precompiled {
        vertex: Some(vertex),
        fragment: Some(fragment),
        fragment_tests: fragment_tests.iter().map(|(_, w)| w.clone()).collect(),
        fragment_linear,
        compute: None,
    };
    Ok((
        Program {
            name: name.to_owned(),
            kind,
            vertex: manifest.clone(),
            vertex_skinned: None,
            fragment: manifest.clone(),
            fragment_tests: fragment_tests
                .into_iter()
                .map(|(t, _)| (t, manifest.clone()))
                .collect(),
            alpha_override: pack.alpha_tests.get(name).copied(),
            fragment_linear: is_final.then(|| manifest.clone()),
            compute: None,
            compute_used: None,
            compute_parity: None,
            dispatch: Dispatch::Fixed([0; 3]),
            layout,
            draw_buffers,
            textures,
            mipmapped,
            spirv: Some(Box::new(precompiled)),
        },
        [text, String::new()],
    ))
}

/// The local size a `[numthreads(x, y, z)]` entry names.
fn workgroup_size(text: &str) -> [u32; 3] {
    let mut size = [1u32; 3];
    let Some(at) = text.find("[numthreads(") else {
        return size;
    };
    let rest = &text[at + "[numthreads(".len()..];
    let Some(close) = rest.find(')') else {
        return size;
    };
    for (axis, value) in rest[..close].split(',').take(3).enumerate() {
        if let Ok(n) = value.trim().parse::<u32>() {
            size[axis] = n.max(1);
        }
    }
    size
}

pub(super) fn compile_compute(
    pack: &mut Pack,
    name: &str,
    path: &str,
) -> Result<(Program, String), String> {
    let (text, defines) = pack.stage_with_defines(path)?;
    let dispatch = match directives::work_groups(&text) {
        Some(groups) => Dispatch::Fixed(groups),
        None => {
            let [x, y, _] = workgroup_size(&text);
            Dispatch::Screen {
                scale: directives::work_groups_render(&text).unwrap_or([1.0, 1.0]),
                local: [x, y],
            }
        }
    };
    let declared = declare(pack, name, &text, 0).map_err(|e| format!("{name}: {e}"))?;
    let layout = pipeline::describe_program(pipeline::Program::Compute(&declared.interface));
    undeclared_buffer(pack, name, &layout)?;
    let textures = texture_sources(pack, name, ProgramKind::Compute, &layout)?;
    let values = Values {
        defines: &defines,
        layout: &layout,
        upright: false,
        linear: false,
        alpha: None,
    };
    let compute = values.apply(
        module(name, "compute", "compute")?,
        &format!("{name}.compute.compute"),
    )?;
    let mipmapped = directives::mipmapped([text.as_str()]);
    Ok((
        Program {
            name: name.to_owned(),
            kind: ProgramKind::Compute,
            vertex: String::new(),
            vertex_skinned: None,
            fragment: String::new(),
            fragment_tests: Vec::new(),
            alpha_override: None,
            fragment_linear: None,
            compute: Some(format!(
                "{}{}",
                declared.header,
                kind_slots(ProgramKind::Compute)
            )),
            compute_used: None,
            compute_parity: None,
            dispatch,
            layout,
            draw_buffers: Vec::new(),
            textures,
            mipmapped,
            spirv: Some(Box::new(Precompiled {
                compute: Some(compute),
                ..Precompiled::default()
            })),
        },
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workgroup_sizes_read_from_the_attribute() {
        assert_eq!(
            workgroup_size("[numthreads(8, 4, 1)]\nvoid compute() {}"),
            [8, 4, 1]
        );
        assert_eq!(workgroup_size("[numthreads(64, 1, 1)]"), [64, 1, 1]);
        assert_eq!(workgroup_size("void compute() {}"), [1, 1, 1]);
    }

    /// The prelude's `frame` members are typed as the engine supplies them
    /// (`iris/frame.slang`'s trailing comments).
    #[test]
    fn frame_members_match_the_engine_types() {
        let text = include_str!("../builtin/iris/frame.slang");
        let mut members = 0;
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("property ") else {
                continue;
            };
            let mut words = rest.split_whitespace();
            let (_, name) = (words.next(), words.next().expect("a name"));
            let glsl = line.rsplit("// ").next().expect("a type comment").trim();
            let engine = UNIFORMS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, t)| *t)
                .or_else(|| crate::shaderpack::uniforms::builtin_type(name))
                .unwrap_or_else(|| panic!("frame.{name} is not a uniform the engine supplies"));
            assert_eq!(glsl, engine, "frame.{name}");
            members += 1;
        }
        assert!(members > 20, "{members} members");
    }

    /// The manifest's own slots are those the preludes name.
    #[test]
    fn kind_slots_match_the_preludes() {
        let common = include_str!("../builtin/iris/iris.slang");
        let preludes = [
            (
                ProgramKind::Geometry,
                include_str!("../builtin/iris/terrain.slang"),
            ),
            (
                ProgramKind::Entity,
                include_str!("../builtin/iris/entity.slang"),
            ),
            (ProgramKind::Screen, ""),
            (ProgramKind::Compute, ""),
        ];
        for (kind, prelude) in preludes {
            let prelude = format!("{prelude}{common}");
            for line in kind_slots(kind).lines() {
                let name = line
                    .split("var<storage, read> ")
                    .nth(1)
                    .and_then(|r| r.split(':').next())
                    .expect("a declaration");
                assert!(
                    prelude.contains(&format!("IRIS_SLOT_{name} ")),
                    "{kind:?}: {name}"
                );
            }
        }
    }
}
