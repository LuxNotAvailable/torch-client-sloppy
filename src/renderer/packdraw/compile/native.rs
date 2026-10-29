//! Native programs: a pack's `<program>.wgsl`, written in WGSL against this
//! engine instead of in GLSL for Iris. Nothing is translated. The file names
//! the uniforms it reads as `frame.<name>`, samples targets by their Iris
//! names (`colortex0`, `depthtex1`, `shadowtex0HW`, ...), and binds a
//! `bufferObject.<n>` with `@binding(IRIS_BUFFER_<n>)`; the engine declares
//! those, prepends the prelude for the program's kind, and lays the bindings
//! out exactly as a translated program's, so everything after compiling
//! (pipelines, targets, flips, uniforms) is shared.
//!
//! Entry points are `fn vertex(input: IrisVertexIn)` and `fn fragment(...)`
//! for drawing programs, `fn compute(...)` for compute ones. A file with a
//! `@compute` entry is a compute program. The prelude gives every drawing
//! program `iris_attributes(input)`, the vertex in the kind's model space
//! (camera-relative for terrain and entities, view space for the hand),
//! `iris_clip` for the clip-space conventions, `iris_alpha_pass` for the
//! alpha test of the layer drawn, and `iris_output` for the final pass.
//! Iris's text directives (`RENDERTARGETS`, `const int colortex0Format`,
//! `const ivec3 workGroups`, ...) are read from the file's comments.

use super::pack::Pack;
use super::program::{texture_sources, undeclared_buffer, used_bindings};
use super::{AlphaTest, Dispatch, PACK_SET, Program, ProgramKind, TERRAIN_SET};
use crate::renderer::terrain_pool::ORIGIN_ROW;
use crate::renderer::terrain_pool::draw::SLOT_TABLE_BINDING;
use crate::shaderpack::directives;
use crate::shaderpack::pipeline::{self, Layout};
use crate::shaderpack::transform::declarations::{PackUniform, std140_size};
use crate::shaderpack::transform::lexer::{self, Kind};
use crate::shaderpack::transform::{Interface, Sampler, StorageBuffer, UNIFORM_BINDING, UNIFORMS};

pub(super) const EXTENSION: &str = ".wgsl";

pub(super) fn is_compute(text: &str) -> bool {
    text.contains("@compute")
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
/// `composite`, `shadow`, `tex`, ...) are left out: in WGSL they are as likely
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
            let ty = super::super::uniforms::builtin_type(name)
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
        buffers.push(StorageBuffer {
            index,
            binding: next,
            read_only: !line.contains("read_write"),
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

/// `text` with each `IRIS_BUFFER_<n>` replaced by its binding.
fn bind_buffers(text: &str, buffers: &[StorageBuffer]) -> String {
    let mut out = text.to_owned();
    // Longest index first, so `IRIS_BUFFER_1` does not eat `IRIS_BUFFER_12`.
    let mut sorted: Vec<&StorageBuffer> = buffers.iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.index));
    for buffer in sorted {
        out = out.replace(
            &format!("IRIS_BUFFER_{}", buffer.index),
            &buffer.binding.to_string(),
        );
    }
    out
}

#[derive(Clone, Copy)]
struct Variant {
    upright: bool,
    linear: bool,
    alpha: Option<AlphaTest>,
}

const PLAIN: Variant = Variant {
    upright: false,
    linear: false,
    alpha: None,
};

fn common_prelude(variant: Variant) -> String {
    let (reference, test) = match variant.alpha {
        Some(test) => (
            test.wrapper_reference(),
            format!("a {} IRIS_ALPHA_REF", test.function.operator()),
        ),
        None => (-1.0, "true".to_owned()),
    };
    let mut out = format!(
        "// Inserted by the engine for a native program.\n\
         const IRIS_UPRIGHT: bool = {};\n\
         const IRIS_LINEAR_OUTPUT: bool = {};\n\
         const IRIS_ALPHA_REF: f32 = {reference:.6};\n\
         fn iris_alpha_pass(a: f32) -> bool {{ return {test}; }}\n\
         fn iris_clip(p: vec4<f32>) -> vec4<f32> {{\n    \
             var o = p;\n    \
             if (!IRIS_UPRIGHT) {{ o.y = -o.y; }}\n    \
             o.z = (o.z + o.w) * 0.5;\n    \
             return o;\n\
         }}\n\
         fn iris_output(c: vec4<f32>) -> vec4<f32> {{\n    \
             if (!IRIS_LINEAR_OUTPUT) {{ return c; }}\n    \
             let d = clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0));\n    \
             let high = pow((d + 0.055) / 1.055, vec3<f32>(2.4));\n    \
             return vec4<f32>(select(high, d / 12.92, d <= vec3<f32>(0.04045)), c.a);\n\
         }}\n\
         struct IrisAttributes {{\n    \
             position: vec3<f32>,\n    \
             uv: vec2<f32>,\n    \
             light: vec2<f32>,\n    \
             color: vec4<f32>,\n    \
             normal: vec3<f32>,\n    \
             entity: vec4<f32>,\n    \
             mid_block: vec4<f32>,\n    \
             entity_color: vec4<f32>,\n    \
             entity_id: i32,\n    \
             block_entity_id: i32,\n    \
             item_id: i32,\n    \
             render_stage: i32,\n\
         }}\n",
        variant.upright, variant.linear
    );
    for (id, name) in crate::shaderpack::features::RENDER_STAGES
        .iter()
        .enumerate()
    {
        out.push_str(&format!("const MC_RENDER_STAGE_{name}: i32 = {id};\n"));
    }
    out
}

fn terrain_prelude(direct: bool) -> String {
    let origin = if direct {
        format!(
            "@group({TERRAIN_SET}) @binding({SLOT_TABLE_BINDING}) var iris_Origins: texture_2d<i32>;\n\
             fn iris_origin(input: IrisVertexIn) -> vec3<i32> {{\n    \
                 let slot = bitcast<u32>(input.raw_position.w) & 0xFFFFu;\n    \
                 return textureLoad(iris_Origins, vec2<i32>(i32(slot % {ORIGIN_ROW}u), i32(slot / {ORIGIN_ROW}u)), 0).xyz;\n\
             }}\n"
        )
    } else {
        format!(
            "@group({TERRAIN_SET}) @binding({SLOT_TABLE_BINDING}) var<storage, read> iris_SlotWords: array<vec4<i32>>;\n\
             fn iris_origin(input: IrisVertexIn) -> vec3<i32> {{\n    \
                 return iris_SlotWords[input.instance_index * 4u + 2u].xyz;\n\
             }}\n"
        )
    };
    format!(
        "struct IrisVertexIn {{\n    \
             @builtin(vertex_index) vertex_index: u32,\n    \
             @builtin(instance_index) instance_index: u32,\n    \
             @location(0) raw_position: vec4<i32>,\n    \
             @location(1) raw_uv: vec2<f32>,\n    \
             @location(2) raw_light: vec4<f32>,\n    \
             @location(3) raw_color: vec4<f32>,\n\
         }}\n\
         {origin}\
         @group({}) @binding(0) var iris_PackQuads: texture_2d<u32>;\n\
         fn iris_snorm3(v: u32) -> vec3<f32> {{\n    \
             let b = vec3<i32>(bitcast<i32>(v << 24u) >> 24u, bitcast<i32>(v << 16u) >> 24u, bitcast<i32>(v << 8u) >> 24u);\n    \
             return max(vec3<f32>(b) / 127.0, vec3<f32>(-1.0));\n\
         }}\n\
         fn iris_attributes(input: IrisVertexIn) -> IrisAttributes {{\n    \
             let quad_index = input.vertex_index >> 2u;\n    \
             let quad = textureLoad(iris_PackQuads, vec2<i32>(i32(quad_index % {}u), i32(quad_index / {}u)), 0);\n    \
             let camera_block = floor(frame.iris_CameraPosition.xyz);\n    \
             let raw = vec3<f32>(input.raw_position.xyz) / 256.0;\n    \
             var a: IrisAttributes;\n    \
             a.position = vec3<f32>(iris_origin(input) - vec3<i32>(camera_block)) - (frame.iris_CameraPosition.xyz - camera_block) + raw;\n    \
             a.uv = input.raw_uv;\n    \
             a.light = input.raw_light.xy * 255.0;\n    \
             a.color = input.raw_color;\n    \
             let n = iris_snorm3(quad.x);\n    \
             a.normal = select(vec3<f32>(0.0, 1.0, 0.0), normalize(n), dot(n, n) > 0.25);\n    \
             let id = quad.w & 65535u;\n    \
             a.entity = vec4<f32>(select(f32(id), -1.0, id == 65535u), 0.0, 0.0, 1.0);\n    \
             let cell = vec3<f32>(f32((quad.w >> 16u) & 15u), f32((quad.w >> 20u) & 15u), f32((quad.w >> 24u) & 15u));\n    \
             a.mid_block = vec4<f32>((cell + 0.5 - raw) * 64.0, f32(quad.w >> 28u));\n    \
             a.entity_color = vec4<f32>(0.0);\n    \
             a.entity_id = -1;\n    \
             a.block_entity_id = -1;\n    \
             a.item_id = -1;\n    \
             a.render_stage = MC_RENDER_STAGE_TERRAIN_SOLID;\n    \
             return a;\n\
         }}\n",
        super::PACK_QUADS_SET,
        crate::renderer::packvertex::QUAD_ROW,
        crate::renderer::packvertex::QUAD_ROW,
    )
}

const ENTITY_PRELUDE: &str = "struct IrisVertexIn {
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) model0: vec4<f32>,
    @location(5) model1: vec4<f32>,
    @location(6) model2: vec4<f32>,
    @location(7) tint: vec4<f32>,
    @location(8) overlay: vec4<f32>,
    @location(9) light: vec2<f32>,
    @location(10) entity_id: i32,
    @location(11) block_entity_id: i32,
    @location(12) item_id: i32,
    @location(13) render_stage: i32,
    @location(14) flags: u32,
}
fn iris_model(input: IrisVertexIn, v: vec4<f32>) -> vec3<f32> {
    return vec3<f32>(dot(input.model0, v), dot(input.model1, v), dot(input.model2, v));
}
fn iris_attributes(input: IrisVertexIn) -> IrisAttributes {
    var a: IrisAttributes;
    a.position = iris_model(input, vec4<f32>(input.position, 1.0));
    a.uv = input.uv;
    a.light = input.light;
    var color = input.color;
    if ((input.flags & 1u) != 0u) { color.a = 1.0; }
    a.color = color * input.tint;
    let n = iris_model(input, vec4<f32>(input.normal, 0.0));
    a.normal = select(vec3<f32>(0.0, 1.0, 0.0), normalize(n), dot(n, n) > 1e-8);
    a.entity = vec4<f32>(-1.0, 0.0, 0.0, 1.0);
    a.mid_block = vec4<f32>(0.0);
    a.entity_color = input.overlay;
    a.entity_id = input.entity_id;
    a.block_entity_id = input.block_entity_id;
    a.item_id = input.item_id;
    a.render_stage = input.render_stage;
    return a;
}
";

const SCREEN_PRELUDE: &str = "struct IrisVertexIn {
    @builtin(vertex_index) vertex_index: u32,
}
fn iris_attributes(input: IrisVertexIn) -> IrisAttributes {
    let corner = vec2<f32>(f32((input.vertex_index << 1u) & 2u), f32(input.vertex_index & 2u));
    var a: IrisAttributes;
    a.position = vec3<f32>(corner, 0.0);
    a.uv = corner;
    a.light = vec2<f32>(240.0);
    a.color = vec4<f32>(1.0);
    a.normal = vec3<f32>(0.0, 0.0, 1.0);
    a.entity = vec4<f32>(-1.0, 0.0, 0.0, 1.0);
    a.mid_block = vec4<f32>(0.0);
    a.entity_color = vec4<f32>(0.0);
    a.entity_id = -1;
    a.block_entity_id = -1;
    a.item_id = -1;
    a.render_stage = MC_RENDER_STAGE_NONE;
    return a;
}
/// The whole screen, for a pass's vertex stage.
fn iris_screen(input: IrisVertexIn) -> vec4<f32> {
    let corner = iris_attributes(input).uv;
    return iris_clip(vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0));
}
";

fn kind_prelude(kind: ProgramKind, direct: bool) -> String {
    match kind {
        ProgramKind::Geometry | ProgramKind::Shadow => terrain_prelude(direct),
        ProgramKind::Entity | ProgramKind::ShadowEntity => ENTITY_PRELUDE.to_owned(),
        ProgramKind::Screen => SCREEN_PRELUDE.to_owned(),
        ProgramKind::Compute => String::new(),
    }
}

/// `text` with the entry point `from` renamed `main`, the name pipelines use.
fn entry(text: &str, from: &str) -> Result<String, String> {
    let needle = format!("fn {from}(");
    if text.matches(&needle).count() != 1 {
        return Err(format!("needs exactly one `{needle}...)` entry point"));
    }
    Ok(text.replacen(&needle, "fn main(", 1))
}

fn validate(name: &str, wgsl: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(wgsl)
        .map_err(|e| format!("{name}: {}", e.emit_to_string(wgsl)))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| format!("{name}: {}", e.emit_to_string(wgsl)))?;
    Ok(())
}

struct Source<'a> {
    name: &'a str,
    kind: ProgramKind,
    direct: bool,
    header: &'a str,
    body: &'a str,
}

impl Source<'_> {
    fn module(&self, stage: &str, variant: Variant) -> Result<String, String> {
        let wgsl = format!(
            "{}{}{}\n{}",
            common_prelude(variant),
            self.header,
            kind_prelude(self.kind, self.direct),
            entry(self.body, stage).map_err(|e| format!("{}: {e}", self.name))?
        );
        validate(&format!("{}.wgsl ({stage})", self.name), &wgsl)?;
        Ok(wgsl)
    }
}

pub(super) fn compile_draw(
    pack: &mut Pack,
    name: &str,
    kind: ProgramKind,
    tests: &[AlphaTest],
    direct: bool,
) -> Result<(Program, [String; 2]), String> {
    let path = format!("{}{name}{EXTENSION}", pack.folder);
    let text = pack.stage_at(&path)?;
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
    let body = bind_buffers(&text, &declared.interface.buffers);
    let source = Source {
        name,
        kind,
        direct,
        header: &declared.header,
        body: &body,
    };
    let plain = Variant {
        upright: is_final,
        ..PLAIN
    };
    let vertex = source.module("vertex", plain)?;
    let fragment = source.module("fragment", plain)?;
    let fragment_linear = if is_final {
        Some(source.module(
            "fragment",
            Variant {
                linear: true,
                ..plain
            },
        )?)
    } else {
        None
    };
    let mut fragment_tests: Vec<(AlphaTest, String)> = Vec::with_capacity(tests.len());
    for test in tests {
        if fragment_tests.iter().any(|(t, _)| t == test) {
            continue;
        }
        let wgsl = source.module(
            "fragment",
            Variant {
                alpha: Some(*test),
                ..plain
            },
        )?;
        fragment_tests.push((*test, wgsl));
    }
    let mipmapped = directives::mipmapped([text.as_str()]);
    Ok((
        Program {
            name: name.to_owned(),
            kind,
            vertex,
            fragment,
            fragment_tests,
            alpha_override: pack.alpha_tests.get(name).copied(),
            fragment_linear,
            compute: None,
            compute_used: None,
            compute_parity: None,
            dispatch: Dispatch::Fixed([0; 3]),
            layout,
            draw_buffers,
            textures,
            mipmapped,
        },
        [text, String::new()],
    ))
}

fn workgroup_size(text: &str) -> [u32; 3] {
    let mut size = [1u32; 3];
    let Some(at) = text.find("@workgroup_size(") else {
        return size;
    };
    let rest = &text[at + "@workgroup_size(".len()..];
    let Some(close) = rest.find(')') else {
        return size;
    };
    for (axis, value) in rest[..close].split(',').take(3).enumerate() {
        if let Ok(n) = value.trim().trim_end_matches('u').parse::<u32>() {
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
    let text = pack.stage_at(path)?;
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
    let body = bind_buffers(&text, &declared.interface.buffers);
    let source = Source {
        name,
        kind: ProgramKind::Compute,
        direct: false,
        header: &declared.header,
        body: &body,
    };
    let wgsl = source.module("compute", PLAIN)?;
    let used = used_bindings(&wgsl);
    let mipmapped = directives::mipmapped([text.as_str()]);
    Ok((
        Program {
            name: name.to_owned(),
            kind: ProgramKind::Compute,
            vertex: String::new(),
            fragment: String::new(),
            fragment_tests: Vec::new(),
            alpha_override: None,
            fragment_linear: None,
            compute: Some(wgsl),
            compute_used: used,
            compute_parity: None,
            dispatch,
            layout,
            draw_buffers: Vec::new(),
            textures,
            mipmapped,
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
            workgroup_size("@compute @workgroup_size(8, 4)\nfn compute() {}"),
            [8, 4, 1]
        );
        assert_eq!(workgroup_size("@compute @workgroup_size(64u)"), [64, 1, 1]);
    }

    #[test]
    fn buffers_bind_by_index_without_prefix_clashes() {
        let buffers = [
            StorageBuffer {
                index: 1,
                binding: 7,
                read_only: true,
            },
            StorageBuffer {
                index: 12,
                binding: 8,
                read_only: false,
            },
        ];
        let out = bind_buffers(
            "@binding(IRIS_BUFFER_12) x; @binding(IRIS_BUFFER_1) y;",
            &buffers,
        );
        assert_eq!(out, "@binding(8) x; @binding(7) y;");
    }

    #[test]
    fn the_entry_must_be_unique() {
        assert!(entry("fn vertex() {} fn vertex() {}", "vertex").is_err());
        assert_eq!(
            entry("@vertex fn vertex() {}", "vertex").as_deref(),
            Ok("@vertex fn main() {}")
        );
    }

    #[test]
    fn every_prelude_validates() {
        let header = "struct IrisFrame {\n".to_owned()
            + &UNIFORMS
                .iter()
                .map(|(n, t)| format!("    {n}: {},\n", wgsl_type(t).unwrap()))
                .collect::<String>()
            + "}\n@group(0) @binding(0) var<uniform> frame: IrisFrame;\n";
        let body = "@vertex fn main(input: IrisVertexIn) -> @builtin(position) vec4<f32> {\n    \
                    let a = iris_attributes(input);\n    \
                    return iris_clip(frame.iris_ModelViewProjectionMatrix * vec4<f32>(a.position, 1.0));\n}\n";
        for kind in [
            ProgramKind::Geometry,
            ProgramKind::Entity,
            ProgramKind::Screen,
        ] {
            for direct in [false, true] {
                let wgsl = format!(
                    "{}{header}{}{body}",
                    common_prelude(PLAIN),
                    kind_prelude(kind, direct)
                );
                validate(&format!("{kind:?}"), &wgsl).unwrap_or_else(|e| panic!("{e}"));
            }
        }
    }
}
