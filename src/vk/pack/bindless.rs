//! A pack program's resources moved onto the shared bindless set
//! (VULKAN_REWRITE_CHECKLIST.md 6.2: "pack textures and samplers join the
//! bindless set").
//!
//! Every pack stage ends as WGSL (6.1), declaring its resources one by
//! one: `@group(g) @binding(b) var name: texture_2d<f32>;`, samplers, the
//! frame's uniform block, storage buffers. `rewrite` takes those
//! declarations out and points each use at the bindless set instead:
//!
//! - textures index binding 0, samplers binding 1, buffers (uniform blocks
//!   too, read as storage: WGSL lays a type out the same in both) binding
//!   2, each through an array typed for that resource and aliased onto the
//!   binding. Vulkan allows aliasing a binding with different types; naga's
//!   validator does not, so these modules skip its binding check
//!   (`backend::wgsl_to_spirv_aliased`).
//! - The index for each resource comes from a slot table, a buffer of
//!   `u32` the pass fills: `table` and `base` arrive in the push constants
//!   (their first two words), and resource `k` is `table[base + k]`.
//!
//! `Slot`s list the resources in table order, with what each was declared
//! as, so a pass knows what to put where.

use crate::vk::bindless::{MAX_BUFFERS, MAX_SAMPLERS, MAX_TEXTURES};

/// What a slot holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// A sampled image, with its WGSL type (`texture_2d<f32>`,
    /// `texture_depth_2d`, ...).
    Texture(String),
    Sampler,
    Comparison,
    /// A buffer of this WGSL type, written to when `read_write`. Uniform
    /// blocks come here too.
    Buffer { ty: String, read_write: bool },
}

/// One resource the program declared, in slot-table order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub group: u32,
    pub binding: u32,
    pub name: String,
    pub kind: SlotKind,
}

struct Declaration {
    start: usize,
    end: usize,
    slot: Slot,
}

/// Finds `@group(g) @binding(b) var<space> name: type;` declarations.
fn declarations(source: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = source[from..].find("@group(") {
        let start = from + at;
        from = start + 1;
        let rest = &source[start..];
        let Some(end) = rest.find(';') else {
            break;
        };
        let text = &rest[..end];
        let number = |key: &str| -> Option<u32> {
            let at = text.find(key)? + key.len();
            let close = text[at..].find(')')?;
            text[at..at + close].trim().parse().ok()
        };
        let (Some(group), Some(binding)) = (number("@group("), number("@binding(")) else {
            continue;
        };
        let Some(var_at) = text.find("var") else {
            continue;
        };
        let after = &text[var_at + 3..];
        let (space, after) = match after.trim_start().strip_prefix('<') {
            Some(inner) => {
                let close = inner.find('>').unwrap_or(0);
                (inner[..close].trim().to_owned(), &inner[close + 1..])
            }
            None => (String::new(), after),
        };
        let Some((name, ty)) = after.split_once(':') else {
            continue;
        };
        let name = name.trim().to_owned();
        let ty = ty.split_whitespace().collect::<Vec<_>>().join(" ");
        let kind = if ty.starts_with("texture_") {
            SlotKind::Texture(ty)
        } else if ty == "sampler" {
            SlotKind::Sampler
        } else if ty == "sampler_comparison" {
            SlotKind::Comparison
        } else if space.starts_with("uniform") || space.starts_with("storage") {
            SlotKind::Buffer {
                ty,
                read_write: space.contains("read_write"),
            }
        } else {
            continue;
        };
        out.push(Declaration {
            start,
            end: start + end + 1,
            slot: Slot {
                group,
                binding,
                name,
                kind,
            },
        });
        from = start + end + 1;
    }
    out
}

/// A WGSL identifier-safe tag for a type.
fn tag(ty: &str) -> String {
    ty.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Replaces each whole-word use of `name` (not a field, `.name`) with
/// `with`.
fn replace_uses(source: &str, name: &str, with: &str) -> String {
    let bytes = source.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = String::with_capacity(source.len());
    let mut last = 0;
    let mut from = 0;
    while let Some(at) = source[from..].find(name) {
        let start = from + at;
        let end = start + name.len();
        from = end;
        let before = start.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(end).copied();
        if before.is_some_and(|c| word(c) || c == b'.') || after.is_some_and(word) {
            continue;
        }
        out.push_str(&source[last..start]);
        out.push_str(with);
        last = end;
    }
    out.push_str(&source[last..]);
    out
}

/// The aliased array for a texture or sampler type, and its declaration.
fn handle_array(ty: &str) -> Option<(String, String)> {
    if ty.starts_with("texture_") {
        let array = format!("iris_textures_{}", tag(ty));
        let decl = format!(
            "@group(0) @binding(0) var {array}: binding_array<{ty}, {MAX_TEXTURES}>;\n"
        );
        Some((array, decl))
    } else if ty == "sampler" || ty == "sampler_comparison" {
        let array = format!("iris_samplers_{ty}");
        let decl = format!(
            "@group(0) @binding(1) var {array}: binding_array<{ty}, {MAX_SAMPLERS}>;\n"
        );
        Some((array, decl))
    } else {
        None
    }
}

/// The index just past the brace matching the `{` at `open`.
fn block_end(source: &str, open: usize) -> usize {
    let mut depth = 0i32;
    for (i, c) in source[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return open + i + 1;
                }
            }
            _ => {}
        }
    }
    source.len()
}

/// The index of the paren matching the `(` at `open`.
fn paren_end(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in source[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits an argument or parameter list at its top-level commas.
fn split_args(args: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0i32, 0);
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if !args[start..].trim().is_empty() {
        out.push(&args[start..]);
    }
    out
}

/// Functions taking textures or samplers take their slot (`u32`) instead:
/// naga's SPIR-V writer passes such parameters by pointer and so cannot
/// pass an element of a binding array. Inside, each such parameter reads
/// its array at that slot; each call passes the slot (`pass_slots`).
/// Returns, per rewritten function, the parameter positions and arrays.
fn slot_parameters(
    body: &mut String,
    header: &mut String,
    declared: &mut Vec<String>,
) -> Vec<(String, Vec<(usize, String)>)> {
    let mut changed = Vec::new();
    let mut from = 0;
    while let Some(at) = body[from..].find("fn ") {
        let start = from + at;
        from = start + 3;
        if start > 0 {
            let before = body.as_bytes()[start - 1];
            if before.is_ascii_alphanumeric() || before == b'_' {
                continue;
            }
        }
        let Some(open) = body[start..].find('(').map(|o| start + o) else {
            break;
        };
        let name = body[start + 3..open].trim().to_owned();
        let Some(close) = paren_end(body, open) else {
            break;
        };
        let params: Vec<String> = split_args(&body[open + 1..close])
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut converted = Vec::new();
        let mut new_params = Vec::new();
        for (position, param) in params.iter().enumerate() {
            let Some((pname, ty)) = param.split_once(':') else {
                new_params.push(param.clone());
                continue;
            };
            match handle_array(ty.trim()) {
                Some((array, decl)) => {
                    if !declared.contains(&array) {
                        declared.push(array.clone());
                        header.push_str(&decl);
                    }
                    new_params.push(format!("{}: u32", pname.trim()));
                    converted.push((position, pname.trim().to_owned(), array));
                }
                None => new_params.push(param.clone()),
            }
        }
        if converted.is_empty() {
            continue;
        }
        let Some(brace) = body[close..].find('{').map(|b| close + b) else {
            break;
        };
        let end = block_end(body, brace);
        let mut function = body[brace..end].to_owned();
        for (_, pname, array) in &converted {
            function = replace_uses(&function, pname, &format!("{array}[iris_slot({pname})]"));
        }
        let rebuilt = format!(
            "{}({}){}{function}",
            &body[start..open],
            new_params.join(", "),
            &body[close + 1..brace]
        );
        body.replace_range(start..end, &rebuilt);
        from = start + rebuilt.len();
        changed.push((
            name,
            converted.into_iter().map(|(p, _, a)| (p, a)).collect(),
        ));
    }
    changed
}

/// At each call to a function `slot_parameters` rewrote, passes the slot
/// where the argument reads an array at one.
fn pass_slots(body: &mut String, changed: &[(String, Vec<(usize, String)>)]) {
    for (name, positions) in changed {
        let pattern = format!("{name}(");
        let mut from = 0;
        while let Some(at) = body[from..].find(&pattern) {
            let start = from + at;
            from = start + pattern.len();
            let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
            if body[..start].chars().next_back().is_some_and(word)
                || body[..start].trim_end().ends_with("fn")
            {
                continue;
            }
            let open = start + name.len();
            let Some(close) = paren_end(body, open) else {
                break;
            };
            let mut args: Vec<String> = split_args(&body[open + 1..close])
                .into_iter()
                .map(|a| a.trim().to_owned())
                .collect();
            for (position, array) in positions {
                let Some(arg) = args.get_mut(*position) else {
                    continue;
                };
                let prefix = format!("{array}[iris_slot(");
                if let Some(inner) = arg
                    .strip_prefix(prefix.as_str())
                    .and_then(|r| r.strip_suffix(")]"))
                {
                    *arg = inner.to_owned();
                }
            }
            let call = format!("({})", args.join(", "));
            body.replace_range(open..=close, &call);
            from = open + call.len();
        }
    }
}

/// `source` with its declared resources on the bindless set, and the slots
/// in table order.
pub fn rewrite(source: &str) -> (String, Vec<Slot>) {
    let found = declarations(source);
    if found.is_empty() {
        return (source.to_owned(), Vec::new());
    }
    // Drop the declarations (last first, so earlier offsets hold).
    let mut body = source.to_owned();
    for d in found.iter().rev() {
        body.replace_range(d.start..d.end, "");
    }
    let slots: Vec<Slot> = found.into_iter().map(|d| d.slot).collect();

    let mut header = format!(
        "struct IrisBindlessPush {{ table: u32, base: u32 }}\n\
         var<push_constant> iris_bindless: IrisBindlessPush;\n\
         struct IrisBindlessSlots {{ s: array<u32> }}\n\
         @group(0) @binding(2) var<storage, read> iris_slot_tables: binding_array<IrisBindlessSlots, {MAX_BUFFERS}>;\n\
         fn iris_slot(k: u32) -> u32 {{ return iris_slot_tables[iris_bindless.table].s[iris_bindless.base + k]; }}\n"
    );
    let mut declared: Vec<String> = Vec::new();
    let changed = slot_parameters(&mut body, &mut header, &mut declared);
    for (k, slot) in slots.iter().enumerate() {
        let index = format!("iris_slot({k}u)");
        let (array, decl, access) = match &slot.kind {
            SlotKind::Texture(ty) => {
                let array = format!("iris_textures_{}", tag(ty));
                let decl = format!(
                    "@group(0) @binding(0) var {array}: binding_array<{ty}, {MAX_TEXTURES}>;\n"
                );
                (array.clone(), decl, format!("{array}[{index}]"))
            }
            SlotKind::Sampler | SlotKind::Comparison => {
                let ty = if slot.kind == SlotKind::Sampler {
                    "sampler"
                } else {
                    "sampler_comparison"
                };
                let array = format!("iris_samplers_{ty}");
                let decl = format!(
                    "@group(0) @binding(1) var {array}: binding_array<{ty}, {MAX_SAMPLERS}>;\n"
                );
                (array.clone(), decl, format!("{array}[{index}]"))
            }
            SlotKind::Buffer { ty, read_write } => {
                // Bindless storage buffers hold structs; a bare array is
                // wrapped.
                let access_mode = if *read_write { "read_write" } else { "read" };
                let bare = ty.starts_with("array<");
                let inner = if bare {
                    format!("IrisBindlessWrap_{}", tag(ty))
                } else {
                    ty.clone()
                };
                let array = format!("iris_buffers_{}_{access_mode}", tag(&inner));
                let mut decl = String::new();
                if bare {
                    decl.push_str(&format!("struct {inner} {{ v: {ty} }}\n"));
                }
                decl.push_str(&format!(
                    "@group(0) @binding(2) var<storage, {access_mode}> {array}: binding_array<{inner}, {MAX_BUFFERS}>;\n"
                ));
                let access = if bare {
                    format!("{array}[{index}].v")
                } else {
                    format!("{array}[{index}]")
                };
                (array, decl, access)
            }
        };
        if !declared.contains(&array) {
            declared.push(array);
            header.push_str(&decl);
        }
        body = replace_uses(&body, &slot.name, &access);
    }
    pass_slots(&mut body, &changed);
    (format!("{header}{body}"), slots)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stage of every installed pack, rewritten onto the bindless set,
    /// compiles and (with `SPIRV_VAL` naming the SDK's spirv-val) is valid
    /// SPIR-V for Vulkan 1.2.
    #[test]
    #[ignore = "needs shader packs (MC_SHADERS_DIR)"]
    fn every_installed_stage_goes_bindless() {
        let (mut stages, mut failures) = (0usize, Vec::new());
        for pack in crate::shaderpack::discover::list() {
            let Ok(installed) = crate::vk::pack::load(&pack.name, false) else {
                failures.push(format!("{}: does not compile", pack.name));
                continue;
            };
            for program in &installed.programs {
                let modules = std::iter::once(&program.vertex)
                    .chain(program.vertex_skinned.iter())
                    .chain(std::iter::once(&program.fragment))
                    .chain(program.fragment_tests.iter().map(|(_, m)| m))
                    .chain(program.fragment_linear.iter())
                    .chain(program.compute.iter())
                    .filter(|m| !m.wgsl.is_empty());
                for module in modules {
                    stages += 1;
                    let (wgsl, _) = rewrite(&module.wgsl);
                    match crate::shaderpack::backend::wgsl_to_spirv_aliased(&wgsl) {
                        Err(e) => failures.push(format!(
                            "{}: {}",
                            module.label,
                            e.lines().take(4).collect::<Vec<_>>().join(" | ")
                        )),
                        Ok(words) => {
                            if let Some(problem) = spirv_val(&words) {
                                failures.push(format!("{}: spirv-val: {problem}", module.label));
                            }
                        }
                    }
                }
            }
        }
        println!("{stages} stages, {} failed", failures.len());
        for f in failures.iter().take(30) {
            println!("FAIL  {f}");
        }
        assert!(stages > 0);
        assert!(failures.is_empty());
    }

    fn spirv_val(words: &[u32]) -> Option<String> {
        let tool = std::env::var_os("SPIRV_VAL")?;
        let path = std::env::temp_dir().join(format!("torch-bindless-{}.spv", std::process::id()));
        std::fs::write(&path, bytemuck::cast_slice::<u32, u8>(words)).ok()?;
        let out = std::process::Command::new(tool)
            .args(["--target-env", "vulkan1.2"])
            .arg(&path)
            .output()
            .ok()?;
        let _ = std::fs::remove_file(&path);
        (!out.status.success()).then(|| {
            String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("failed").to_owned()
        })
    }

    #[test]
    fn declarations_move_onto_the_bindless_set() {
        let source = "struct IrisFrame { time: f32 }\n\
            @group(0) @binding(0)\nvar gtexture: texture_2d<f32>;\n\
            @group(0) @binding(1)\nvar gtexture_sampler: sampler;\n\
            @group(0) @binding(2)\nvar<uniform> frame: IrisFrame;\n\
            @group(2) @binding(0) var<storage, read> quads: array<vec4<i32>>;\n\
            @fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {\n\
            let q = quads[0];\n\
            return textureSample(gtexture, gtexture_sampler, uv) * frame.time + vec4<f32>(q);\n}\n";
        let (out, slots) = rewrite(source);
        assert_eq!(slots.len(), 4);
        assert_eq!(slots[0].kind, SlotKind::Texture("texture_2d<f32>".into()));
        assert_eq!(slots[1].kind, SlotKind::Sampler);
        assert!(matches!(slots[2].kind, SlotKind::Buffer { read_write: false, .. }));
        assert!(!out.contains("var gtexture:"), "{out}");
        assert!(out.contains("iris_textures_texture_2d_f32_[iris_slot(0u)]"), "{out}");
        assert!(out.contains("iris_buffers_IrisFrame_read[iris_slot(2u)].time"), "{out}");
        assert!(out.contains("[iris_slot(3u)].v[0]"), "{out}");
        crate::shaderpack::backend::wgsl_to_spirv_aliased(&out).expect("compiles");
    }
}
