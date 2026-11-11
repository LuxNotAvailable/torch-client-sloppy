use naga::back::wgsl;
use naga::front::glsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};

pub(crate) use naga::ShaderStage;

pub(crate) fn to_wgsl(
    source: &str,
    stage: ShaderStage,
    defines: &[(&str, &str)],
) -> Result<String, String> {
    let mut options = glsl::Options::from(stage);
    options.defines.extend(
        defines
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    let mut frontend = glsl::Frontend::default();
    let module = frontend.parse(&options, source).map_err(|errors| {
        errors.errors.first().map_or_else(
            || "the shader could not be parsed".to_owned(),
            |e| e.to_string(),
        )
    })?;

    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|e| format!("the shader is not valid: {}", chain(&e)))?;

    wgsl::write_string(&module, &info, wgsl::WriterFlags::empty())
        .map_err(|e| format!("the shader could not be written as WGSL: {e}"))
}

/// A compiled program stage, as WGSL (a GLSL pack's after the transform and
/// naga, or a native WGSL pack's own), to SPIR-V for the native renderer
/// (VULKAN_REWRITE_CHECKLIST.md 6.1). Every pack program passes through
/// WGSL already, so this one step covers both kinds of pack.
#[allow(dead_code, reason = "used by the native renderer's pack path as it lands")]
pub(crate) fn wgsl_to_spirv(source: &str) -> Result<Vec<u32>, String> {
    spirv_with(source, ValidationFlags::all())
}

/// As `wgsl_to_spirv`, for a stage whose resources sit on the bindless set
/// (`vk::pack::bindless`): several typed arrays alias each binding, which
/// Vulkan allows and naga's binding check refuses, so that check is off.
#[allow(dead_code, reason = "used by the native renderer's pack path as it lands")]
pub(crate) fn wgsl_to_spirv_aliased(source: &str) -> Result<Vec<u32>, String> {
    spirv_with(source, ValidationFlags::all() - ValidationFlags::BINDINGS)
}

fn spirv_with(source: &str, flags: ValidationFlags) -> Result<Vec<u32>, String> {
    use naga::back::spv;
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| format!("the WGSL could not be parsed: {}", e.emit_to_string(source)))?;
    let info = Validator::new(flags, Capabilities::all())
        .validate(&module)
        .map_err(|e| format!("the shader is not valid: {}", chain(&e)))?;
    let options = spv::Options {
        lang_version: (1, 3),
        // As wgpu writes for Vulkan: flips clip y, so a normal viewport
        // matches wgpu's (and the pack transform's flips hold).
        flags: spv::WriterFlags::ADJUST_COORDINATE_SPACE,
        ..Default::default()
    };
    let mut words = spv::write_vec(&module, &info, &options, None)
        .map_err(|e| format!("the shader could not be written as SPIR-V: {e}"))?;
    relax_atomics(&mut words);
    Ok(words)
}

/// naga marks every atomic relaxed with a storage-class semantics bit
/// (UniformMemory or WorkgroupMemory), which Vulkan's SPIR-V rules refuse
/// (VUID-StandaloneSpirv-MemorySemantics-10871): a storage-class bit needs
/// an acquire or release order. WGSL's atomics are all relaxed, so the bit
/// says nothing; each atomic whose semantics carry no ordering is pointed
/// at a new constant 0 (plain relaxed) instead.
fn relax_atomics(words: &mut Vec<u32>) {
    const HEADER: usize = 5;
    const OP_TYPE_INT: u32 = 21;
    const OP_CONSTANT: u32 = 43;
    // Acquire, Release, AcquireRelease, SequentiallyConsistent.
    const ORDER_BITS: u32 = 0x2 | 0x4 | 0x8 | 0x10;
    /// Each atomic's opcode and where its semantics operands sit.
    fn semantics_at(opcode: u32) -> &'static [usize] {
        match opcode {
            // OpAtomicStore: pointer, scope, semantics, value.
            228 => &[3],
            // OpAtomicCompareExchange(Weak): two semantics.
            230 | 231 => &[5, 6],
            // Load, exchange, increment, decrement and the arithmetic and
            // bitwise read-modify-writes; the float ones (EXT).
            227 | 229 | 232..=242 | 5614 | 5615 | 6035 => &[5],
            _ => &[],
        }
    }
    if words.len() <= HEADER {
        return;
    }
    // One pass to find the 32-bit unsigned type, the constants' values and
    // the atomics.
    let mut u32_type = None;
    let mut values = std::collections::HashMap::new();
    let mut atomics = Vec::new();
    let mut at = HEADER;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        let opcode = words[at] & 0xFFFF;
        if count == 0 || at + count > words.len() {
            return;
        }
        match opcode {
            OP_TYPE_INT if count == 4 && words[at + 2] == 32 && words[at + 3] == 0 => {
                u32_type.get_or_insert((words[at + 1], at + count));
            }
            OP_CONSTANT if count == 4 => {
                values.insert(words[at + 2], words[at + 3]);
            }
            _ => {
                for &operand in semantics_at(opcode) {
                    atomics.push(at + operand);
                }
            }
        }
        at += count;
    }
    let relaxed: Vec<usize> = atomics
        .into_iter()
        .filter(|&slot| {
            values
                .get(&words[slot])
                .is_some_and(|v| *v != 0 && *v & ORDER_BITS == 0)
        })
        .collect();
    let Some((type_id, after_type)) = u32_type else {
        return;
    };
    if relaxed.is_empty() {
        return;
    }
    // A new constant 0 just after the type, with the next free id.
    let zero = words[3];
    words[3] += 1;
    for slot in relaxed {
        words[slot] = zero;
    }
    words.splice(
        after_type..after_type,
        [(4 << 16) | OP_CONSTANT, type_id, zero, 0],
    );
}

fn chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(next) = source {
        out.push_str(": ");
        out.push_str(&next.to_string());
        source = next.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every atomic in a module written for Vulkan carries plain relaxed
    /// semantics (0), not naga's relaxed storage-class bit.
    #[test]
    fn atomics_come_out_plain_relaxed() {
        let wgsl = "struct S { n: atomic<u32>, k: array<atomic<u32>, 4> }
                    @group(0) @binding(0) var<storage, read_write> s: S;
                    @compute @workgroup_size(1) fn main() {
                    atomicStore(&s.n, 0u); let a = atomicLoad(&s.k[1]);
                    let b = atomicAdd(&s.n, a); atomicStore(&s.k[0], b); }
";
        let words = wgsl_to_spirv(wgsl).expect("compiles");
        let mut constants = std::collections::HashMap::new();
        let mut semantics = Vec::new();
        let mut at = 5;
        while at < words.len() {
            let (count, opcode) = ((words[at] >> 16) as usize, words[at] & 0xFFFF);
            match opcode {
                43 => {
                    constants.insert(words[at + 2], words[at + 3]);
                }
                228 => semantics.push(words[at + 3]),
                227 | 234 => semantics.push(words[at + 5]),
                _ => {}
            }
            at += count;
        }
        assert_eq!(semantics.len(), 4, "two stores, a load and an add");
        for id in semantics {
            assert_eq!(constants.get(&id), Some(&0), "semantics constant {id}");
        }
    }

    #[test]
    fn which_declaration_forms_naga_accepts() {
        let cases = [
            ("empty, no profile", "#version 450\nvoid main() {}\n".to_owned()),
            ("empty, core profile", "#version 450 core\nvoid main() {}\n".to_owned()),
            (
                "one output",
                "#version 450\nlayout(location = 0) out vec4 c;\nvoid main() { c = vec4(1.0); }\n".to_owned(),
            ),
            (
                "one input",
                "#version 450\nlayout(location = 0) in vec2 uv;\nlayout(location = 0) out vec4 c;\nvoid main() { c = vec4(uv, 0.0, 1.0); }\n".to_owned(),
            ),
            (
                "bare in/out, no location",
                "#version 450\nin vec2 uv;\nout vec4 c;\nvoid main() { c = vec4(uv, 0.0, 1.0); }\n".to_owned(),
            ),
            (
                "loose sampler uniform",
                "#version 450\nlayout(location = 0) out vec4 c;\nuniform sampler2D tex;\nvoid main() { c = texture(tex, vec2(0.0)); }\n".to_owned(),
            ),
            (
                "sampler with binding",
                "#version 450\nlayout(location = 0) out vec4 c;\nlayout(set = 0, binding = 0) uniform sampler2D tex;\nvoid main() { c = texture(tex, vec2(0.0)); }\n".to_owned(),
            ),
            (
                "loose scalar uniform",
                "#version 450\nlayout(location = 0) out vec4 c;\nuniform float k;\nvoid main() { c = vec4(k); }\n".to_owned(),
            ),
            (
                "separated texture and sampler",
                "#version 450\nlayout(location = 0) out vec4 c;\nlayout(set = 0, binding = 0) uniform texture2D tex;\nlayout(set = 0, binding = 1) uniform sampler samp;\nvoid main() { c = texture(sampler2D(tex, samp), vec2(0.0)); }\n".to_owned(),
            ),
            (
                "scalars in a bound block",
                "#version 450\nlayout(location = 0) out vec4 c;\nlayout(set = 0, binding = 0) uniform U { float k; mat4 m; } u;\nvoid main() { c = u.m * vec4(u.k); }\n".to_owned(),
            ),
            (
                "anonymous uniform block",
                "#version 450\nlayout(location = 0) out vec4 c;\nlayout(set = 0, binding = 0) uniform U { float k; mat4 m; };\nvoid main() { c = m * vec4(k); }\n".to_owned(),
            ),
            (
                "sampler2D as a function parameter",
                "#version 450\nlayout(location = 0) out vec4 c;\nlayout(set = 0, binding = 0) uniform texture2D tex;\nlayout(set = 0, binding = 1) uniform sampler samp;\nvec4 fetch(sampler2D s) { return texture(s, vec2(0.0)); }\nvoid main() { c = fetch(sampler2D(tex, samp)); }\n".to_owned(),
            ),
            (
                "3D texture and sampler",
                "#version 450\nlayout(location = 0) out vec4 c;\nlayout(set = 0, binding = 0) uniform texture3D vol;\nlayout(set = 0, binding = 1) uniform sampler samp;\nvoid main() { c = texture(sampler3D(vol, samp), vec3(0.0)); }\n".to_owned(),
            ),
        ];

        let mut any = false;
        for (name, source) in &cases {
            match to_wgsl(source, ShaderStage::Fragment, &[]) {
                Ok(wgsl) => {
                    any = true;
                    println!("{name}: ok");
                    assert!(wgsl.contains("fn main"), "{wgsl}");
                }
                Err(e) => println!("{name}: {e}"),
            }
        }
        assert!(any, "naga accepted none of the forms tried");
    }

    #[test]
    fn the_version_a_pack_ships_is_refused() {
        let glsl = "#version 130\nvoid main() {}\n";
        let error = to_wgsl(glsl, ShaderStage::Fragment, &[]).unwrap_err();
        assert!(error.to_lowercase().contains("version"), "{error}");
    }

    #[test]
    fn a_compatibility_profile_builtin_is_refused() {
        let glsl = "#version 450 core\nvoid main() { gl_FragData[0] = vec4(1.0); }\n";
        assert!(to_wgsl(glsl, ShaderStage::Fragment, &[]).is_err());
    }

    #[test]
    fn the_terrain_prelude_forms_are_accepted() {
        let cases = [
            (
                "an integer vertex input",
                "#version 450\nlayout(location = 0) in ivec4 p;\nvoid main() { gl_Position = vec4(p) / 256.0; }\n",
            ),
            (
                "a storage block indexed by gl_InstanceIndex",
                "#version 450\nlayout(std430, set = 2, binding = 5) readonly buffer Slots { ivec4 words[]; };\n\
                 void main() { gl_Position = vec4(words[gl_InstanceIndex * 4 + 2]); }\n",
            ),
            (
                "texelFetch from an integer texture",
                "#version 450\nlayout(set = 2, binding = 5) uniform itexture2D origins;\n\
                 void main() { gl_Position = vec4(texelFetch(origins, ivec2(1, 2), 0)); }\n",
            ),
        ];
        for (name, source) in cases {
            let wgsl =
                to_wgsl(source, ShaderStage::Vertex, &[]).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(wgsl.contains("@vertex"), "{name}\n{wgsl}");
        }
    }

    #[test]
    fn a_define_reaches_the_preprocessor() {
        let glsl = "#version 450\nlayout(location = 0) out vec4 c;\nvoid main() {\n#ifdef CUT\n    discard;\n#endif\n    c = vec4(CUT_VALUE);\n}\n";
        let wgsl = to_wgsl(
            glsl,
            ShaderStage::Fragment,
            &[("CUT", "1"), ("CUT_VALUE", "0.5")],
        )
        .expect("should compile");
        assert!(wgsl.contains("discard"), "{wgsl}");
        assert!(
            to_wgsl(glsl, ShaderStage::Fragment, &[("CUT_VALUE", "0.5")])
                .is_ok_and(|w| !w.contains("discard"))
        );
    }
}
