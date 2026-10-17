use std::collections::HashMap;

use super::super::literal;
use super::lexer::{self, Kind, Token};
use super::{Interface, Stage};

const LIGHTMAP_TEXTURE_MATRIX: &str = "mat4(vec4(0.00390625, 0.0, 0.0, 0.0), vec4(0.0, 0.00390625, 0.0, 0.0), \
     vec4(0.0, 0.0, 0.00390625, 0.0), vec4(0.03125, 0.03125, 0.03125, 1.0))";

#[derive(Debug, Default)]
pub(super) struct Used {
    pub(super) ftransform: bool,
    pub(super) dot: bool,
    pub(super) fog: bool,
    pub(super) front_color: bool,
    pub(super) modf: bool,
    pub(super) isnan: bool,
    pub(super) isinf: bool,
}

pub(super) fn builtins(tokens: &mut Vec<Token>, stage: Stage) -> Used {
    let mut used = Used::default();

    texture_matrix(tokens);
    shadow_lookups(tokens);
    const_parameters(tokens);
    vector_modulo(tokens);
    switch_returns(tokens);
    missing_returns(tokens);
    component_out_arguments(tokens);

    for token in tokens.iter_mut() {
        match token.kind {
            Kind::Word => {
                if let Some(replacement) = builtin(&token.text, stage, &mut used) {
                    token.text = replacement.to_owned();
                }
            }
            Kind::Directive => lexer::in_define(token, |word| {
                if let Some(replacement) = builtin(&word.text, stage, &mut used) {
                    word.text = replacement.to_owned();
                }
            }),
            _ => {}
        }
    }
    used
}

fn builtin(word: &str, stage: Stage, used: &mut Used) -> Option<&'static str> {
    Some(match word {
        "ftransform" => {
            used.ftransform = true;
            "iris_ftransform"
        }
        "dot" => {
            used.dot = true;
            "iris_dot"
        }
        "modf" => {
            used.modf = true;
            "iris_modf"
        }
        "isnan" => {
            used.isnan = true;
            "iris_isnan"
        }
        "isinf" => {
            used.isinf = true;
            "iris_isinf"
        }
        "gl_Fog" => {
            used.fog = true;
            "iris_gl_Fog()"
        }
        "gl_FrontColor" if stage == Stage::Vertex => {
            used.front_color = true;
            "iris_FrontColor"
        }
        "gl_ModelViewMatrixInverse" => "iris_ModelViewMatrixInverse",
        "gl_ProjectionMatrixInverse" => "iris_ProjectionMatrixInverse",
        "gl_Vertex" if stage == Stage::Vertex => "vec4(iris_position(), 1.0)",
        "gl_Normal" if stage == Stage::Vertex => "iris_normal()",
        "gl_VertexID" if stage == Stage::Vertex => "gl_VertexIndex",
        "gl_InstanceID" if stage == Stage::Vertex => "gl_InstanceIndex",
        "gl_Color" if stage == Stage::Vertex => "iris_color()",
        "gl_MultiTexCoord0" if stage == Stage::Vertex => "vec4(iris_uv0(), 0.0, 1.0)",
        "gl_MultiTexCoord1" | "gl_MultiTexCoord2" if stage == Stage::Vertex => {
            "vec4(iris_light(), 0.0, 1.0)"
        }
        "gl_NormalMatrix" => "mat3(iris_ModelViewMatrix)",
        "gl_ModelViewMatrix" => "iris_ModelViewMatrix",
        "gl_ProjectionMatrix" => "iris_ProjectionMatrix",
        "gl_ModelViewProjectionMatrix" => "iris_ModelViewProjectionMatrix",
        "texture2D" | "texture3D" | "textureCube" => "texture",
        "texture2DLod" | "texture3DLod" | "texture2DLodARB" => "textureLod",
        "texture2DGrad" | "texture2DGradARB" | "texture3DGrad" | "texture3DGradARB" => {
            "textureGrad"
        }
        "texture2DProj" | "texture3DProj" => "textureProj",
        "texelFetch2D" | "texelFetch3D" => "texelFetch",
        "textureSize2D" => "textureSize",
        _ => return None,
    })
}

fn const_parameters(tokens: &mut Vec<Token>) {
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at].is_word("const")
            && let Some(next) = lexer::next_solid(tokens, at + 1)
            && ["in", "out", "inout"]
                .iter()
                .any(|q| tokens[next].is_word(q))
        {
            tokens.drain(at..next);
        }
        at += 1;
    }
}

fn vector_modulo(tokens: &mut Vec<Token>) {
    let mut at = 0;
    while at < tokens.len() {
        if !tokens[at].is_punct('%') {
            at += 1;
            continue;
        }
        let vector = lexer::prev_solid(tokens, at)
            .filter(|close| tokens[*close].is_punct(')'))
            .and_then(|close| {
                (0..close).rev().find(|open| {
                    tokens[*open].is_punct('(') && lexer::matching(tokens, *open) == Some(close)
                })
            })
            .and_then(|open| lexer::prev_solid(tokens, open))
            .map(|name| tokens[name].text.clone())
            .filter(|name| {
                name.strip_prefix(['i', 'u'])
                    .unwrap_or(name)
                    .strip_prefix("vec")
                    .is_some_and(|n| matches!(n, "2" | "3" | "4"))
            });
        let right = lexer::next_solid(tokens, at + 1).filter(|r| {
            matches!(tokens[*r].kind, Kind::Word | Kind::Number)
                && lexer::next_solid(tokens, r + 1).is_none_or(|n| {
                    !tokens[n].is_punct('(') && !tokens[n].is_punct('.') && !tokens[n].is_punct('[')
                })
        });
        if let (Some(vector), Some(right)) = (vector, right) {
            let text = format!("{vector}({})", tokens[right].text);
            tokens[right] = Token::raw(&text);
        }
        at += 1;
    }
}

fn component_out_arguments(tokens: &mut Vec<Token>) {
    let mut outs: HashMap<String, Vec<Option<String>>> = HashMap::new();
    let depths = lexer::brace_depths(tokens);
    for open in 0..tokens.len() {
        if !tokens[open].is_punct('(') || depths[open] != 0 {
            continue;
        }
        let Some(name) = lexer::prev_solid(tokens, open).filter(|n| tokens[*n].kind == Kind::Word)
        else {
            continue;
        };
        let Some((args, close)) = lexer::arguments(tokens, open) else {
            continue;
        };
        if !lexer::next_solid(tokens, close + 1).is_some_and(|b| tokens[b].is_punct('{')) {
            continue;
        }
        let params: Vec<Option<String>> = args
            .iter()
            .map(|(from, to)| {
                let words: Vec<&str> = (*from..*to)
                    .filter(|i| tokens[*i].kind == Kind::Word)
                    .map(|i| tokens[i].text.as_str())
                    .collect();
                let writes = words.iter().any(|w| matches!(*w, "out" | "inout"));
                let scalar = words
                    .iter()
                    .find(|w| matches!(**w, "float" | "int" | "uint" | "bool"));
                scalar.filter(|_| writes).map(|w| (*w).to_owned())
            })
            .collect();
        if params.iter().any(Option::is_some) {
            outs.insert(tokens[name].text.clone(), params);
        }
    }
    if outs.is_empty() {
        return;
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for at in 0..tokens.len() {
        let Some(params) = outs
            .get(&tokens[at].text)
            .filter(|_| tokens[at].kind == Kind::Word)
        else {
            continue;
        };
        let statement_start = lexer::prev_solid(tokens, at).is_none_or(|p| {
            ['{', '}', ';', ')'].iter().any(|c| tokens[p].is_punct(*c)) || tokens[p].is_word("else")
        });
        let Some(open) = lexer::next_solid(tokens, at + 1).filter(|o| tokens[*o].is_punct('('))
        else {
            continue;
        };
        let Some((args, close)) = lexer::arguments(tokens, open) else {
            continue;
        };
        let Some(end) = lexer::next_solid(tokens, close + 1).filter(|e| tokens[*e].is_punct(';'))
        else {
            continue;
        };
        if !statement_start || args.len() != params.len() {
            continue;
        }
        let (mut before, mut after) = (String::new(), String::new());
        let mut call = Vec::new();
        for (index, ((from, to), param)) in args.iter().zip(params).enumerate() {
            let solid: Vec<usize> = (*from..*to).filter(|i| !tokens[*i].is_trivia()).collect();
            let component = match (param, solid.as_slice()) {
                (Some(ty), [base, dot, comp])
                    if tokens[*base].kind == Kind::Word
                        && tokens[*dot].is_punct('.')
                        && tokens[*comp].text.len() == 1
                        && "xyzwrgbastpq".contains(tokens[*comp].text.as_str()) =>
                {
                    Some((ty, format!("{}.{}", tokens[*base].text, tokens[*comp].text)))
                }
                _ => None,
            };
            match component {
                Some((ty, place)) => {
                    let temp = format!("iris_out{index}");
                    before.push_str(&format!("{ty} {temp} = {place}; "));
                    after.push_str(&format!(" {place} = {temp};"));
                    call.push(temp);
                }
                None => call.push(lexer::render(&tokens[*from..*to]).trim().to_owned()),
            }
        }
        if before.is_empty() {
            continue;
        }
        edits.push((
            at,
            end,
            format!(
                "{{ {before}{}({});{after} }}",
                tokens[at].text,
                call.join(", ")
            ),
        ));
    }
    for (from, to, text) in edits.into_iter().rev() {
        tokens.splice(from..=to, lexer::lex(&text));
    }
}

fn functions(tokens: &[Token]) -> Vec<(usize, usize, usize)> {
    let depths = lexer::brace_depths(tokens);
    let mut out = Vec::new();
    for open in 0..tokens.len() {
        if depths[open] != 1 || !tokens[open].is_punct('{') {
            continue;
        }
        let Some(close_paren) =
            lexer::prev_solid(tokens, open).filter(|p| tokens[*p].is_punct(')'))
        else {
            continue;
        };
        let Some(open_paren) = (0..close_paren)
            .rev()
            .find(|i| tokens[*i].is_punct('(') && lexer::matching(tokens, *i) == Some(close_paren))
        else {
            continue;
        };
        let Some(name) = lexer::prev_solid(tokens, open_paren) else {
            continue;
        };
        let Some(ty) = lexer::prev_solid(tokens, name) else {
            continue;
        };
        if tokens[name].kind != Kind::Word
            || tokens[ty].kind != Kind::Word
            || zero_value(&tokens[ty].text).is_none()
        {
            continue;
        }
        if let Some(close) = lexer::matching(tokens, open) {
            out.push((ty, open, close));
        }
    }
    out
}

fn switch_returns(tokens: &mut Vec<Token>) {
    for serial in 0.. {
        let depths = lexer::brace_depths(tokens);
        let found = functions(tokens).into_iter().find_map(|(ty, open, close)| {
            (open + 1..close).find_map(|at| {
                if !tokens[at].is_word("switch") || depths[at] != 1 {
                    return None;
                }
                let brace = (at..close).find(|i| tokens[*i].is_punct('{'))?;
                let end = lexer::matching(tokens, brace)?;
                let body = depths[brace];
                let returns: Vec<(usize, usize)> = (brace + 1..end)
                    .filter(|r| tokens[*r].is_word("return") && depths[*r] == body)
                    .filter_map(|r| {
                        Some((
                            r,
                            (r..end).find(|i| tokens[*i].is_punct(';') && depths[*i] == body)?,
                        ))
                    })
                    .collect();
                (!returns.is_empty()).then(|| (tokens[ty].text.clone(), at, end, returns))
            })
        });
        let Some((ty, switch, end, returns)) = found else {
            return;
        };
        let zero = zero_value(&ty).expect("`functions` kept only types with a zero");
        let (value, done) = (
            format!("iris_sw_ret{serial}"),
            format!("iris_sw_done{serial}"),
        );
        tokens.splice(
            end + 1..end + 1,
            lexer::lex(&format!(" if ({done}) {{ return {value}; }}")),
        );
        for (ret, semicolon) in returns.into_iter().rev() {
            tokens.splice(
                semicolon + 1..semicolon + 1,
                lexer::lex(&format!(" {done} = true; break;")),
            );
            tokens[ret] = Token::raw(&format!("{value} ="));
        }
        tokens.splice(
            switch..switch,
            lexer::lex(&format!("{ty} {value} = {zero}; bool {done} = false; ")),
        );
    }
}

fn missing_returns(tokens: &mut Vec<Token>) {
    let depths = lexer::brace_depths(tokens);
    let mut inserts: Vec<(usize, String)> = Vec::new();
    for (ty, open, close) in functions(tokens) {
        let zero = zero_value(&tokens[ty].text).expect("`functions` kept only types with a zero");
        let Some(last) = lexer::prev_solid(tokens, close).filter(|l| *l > open) else {
            continue;
        };
        let last_is_return = tokens[last].is_punct(';') && {
            let start = (open + 1..last).rev().find(|i| {
                depths[*i] == 1 && ['{', '}', ';'].iter().any(|c| tokens[*i].is_punct(*c))
            });
            lexer::next_solid(tokens, start.map_or(open + 1, |s| s + 1))
                .is_some_and(|f| tokens[f].is_word("return"))
        };
        if !last_is_return {
            inserts.push((close, format!(" return {zero}; ")));
        }
    }
    for (at, text) in inserts.into_iter().rev() {
        tokens.splice(at..at, lexer::lex(&text));
    }
}

fn zero_value(ty: &str) -> Option<String> {
    Some(match ty {
        "float" => "0.0".to_owned(),
        "int" => "0".to_owned(),
        "uint" => "0u".to_owned(),
        "bool" => "false".to_owned(),
        "vec2" | "vec3" | "vec4" | "mat2" | "mat3" | "mat4" => format!("{ty}(0.0)"),
        "ivec2" | "ivec3" | "ivec4" => format!("{ty}(0)"),
        "uvec2" | "uvec3" | "uvec4" => format!("{ty}(0u)"),
        "bvec2" | "bvec3" | "bvec4" => format!("{ty}(false)"),
        _ => return None,
    })
}

fn shadow_lookups(tokens: &mut Vec<Token>) {
    lexer::in_defines(tokens, shadow_lookups_in);
    shadow_lookups_in(tokens);
}

fn shadow_lookups_in(tokens: &mut Vec<Token>) {
    let mut at = 0;
    while at < tokens.len() {
        let name = match tokens[at].text.as_str() {
            "shadow2D" if tokens[at].kind == Kind::Word => "texture",
            "shadow2DLod" if tokens[at].kind == Kind::Word => "textureLod",
            _ => {
                at += 1;
                continue;
            }
        };
        let Some(open) = lexer::next_solid(tokens, at + 1).filter(|o| tokens[*o].is_punct('('))
        else {
            at += 1;
            continue;
        };
        let Some(close) = lexer::matching(tokens, open) else {
            at += 1;
            continue;
        };
        tokens.insert(close + 1, Token::punct(')'));
        tokens.splice(at..=at, lexer::lex(&format!("vec4({name}")));
        at += 3;
    }
}

fn texture_matrix(tokens: &mut Vec<Token>) {
    lexer::in_defines(tokens, texture_matrix_in);
    texture_matrix_in(tokens);
}

fn texture_matrix_in(tokens: &mut Vec<Token>) {
    let mut at = 0;
    while at < tokens.len() {
        if !tokens[at].is_word("gl_TextureMatrix") {
            at += 1;
            continue;
        }
        match lexer::solid_run::<3>(tokens, at + 1) {
            Some([open, index, close])
                if tokens[open].is_punct('[') && tokens[close].is_punct(']') =>
            {
                let matrix = match tokens[index].text.as_str() {
                    "1" | "2" => LIGHTMAP_TEXTURE_MATRIX,
                    _ => "mat4(1.0)",
                };
                tokens.drain(at..=close);
                tokens.insert(at, Token::raw(matrix));
            }
            _ => tokens[at].text = "mat4(1.0)".to_owned(),
        }
        at += 1;
    }
}

pub(super) fn outputs(tokens: &mut Vec<Token>, interface: &mut Interface, stage: Stage) {
    let mut highest: Option<u32> = None;
    if stage == Stage::Fragment {
        declared_outputs(tokens, &mut highest);
    }
    lexer::in_defines(tokens, |body| frag_data(body, &mut highest));
    frag_data(tokens, &mut highest);
    interface.outputs = highest.map_or(0, |n| n + 1);
}

fn frag_data(tokens: &mut Vec<Token>, highest: &mut Option<u32>) {
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at].is_word("gl_FragColor") {
            tokens[at].text = "iris_FragData0".to_owned();
            *highest = Some(highest.unwrap_or(0));
            at += 1;
            continue;
        }
        if !tokens[at].is_word("gl_FragData") {
            at += 1;
            continue;
        }
        let Some([open, index, close]) = lexer::solid_run::<3>(tokens, at + 1) else {
            at += 1;
            continue;
        };
        if !tokens[open].is_punct('[') || !tokens[close].is_punct(']') {
            at += 1;
            continue;
        }
        let Some(n) = literal::int(&tokens[index].text).and_then(|n| u32::try_from(n).ok()) else {
            at += 1;
            continue;
        };
        tokens.drain(at..=close);
        tokens.insert(at, Token::raw(&format!("iris_FragData{n}")));
        *highest = Some(highest.unwrap_or(0).max(n));
        at += 1;
    }
}

fn declared_outputs(tokens: &mut Vec<Token>, highest: &mut Option<u32>) {
    let depths = lexer::brace_depths(tokens);
    let mut found: Vec<(usize, usize, String, u32)> = Vec::new();
    let mut next = 0u32;
    for at in 0..tokens.len() {
        if depths[at] != 0 || !tokens[at].is_word("out") {
            continue;
        }
        let Some([ty, name, end]) = lexer::solid_run::<3>(tokens, at + 1) else {
            continue;
        };
        if !tokens[ty].is_word("vec4")
            || tokens[name].kind != Kind::Word
            || !tokens[end].is_punct(';')
        {
            continue;
        }
        let mut first = at;
        let mut location = None;
        if let Some(close) = lexer::prev_solid(tokens, at).filter(|c| tokens[*c].is_punct(')')) {
            let open = (0..close).rev().find(|i| tokens[*i].is_punct('('));
            let layout = open
                .and_then(|o| lexer::prev_solid(tokens, o))
                .filter(|l| tokens[*l].is_word("layout"));
            if let (Some(open), Some(layout)) = (open, layout) {
                let inner: Vec<usize> = (open + 1..close)
                    .filter(|i| !tokens[*i].is_trivia())
                    .collect();
                match inner[..] {
                    [key, eq, n] if tokens[key].is_word("location") && tokens[eq].is_punct('=') => {
                        let Some(n) =
                            literal::int(&tokens[n].text).and_then(|n| u32::try_from(n).ok())
                        else {
                            continue;
                        };
                        location = Some(n);
                        first = layout;
                    }
                    _ => continue,
                }
            }
        }
        let location = location.unwrap_or_else(|| {
            next += 1;
            next - 1
        });
        found.push((first, end, tokens[name].text.clone(), location));
    }
    if found.is_empty() {
        return;
    }
    for &(_, _, _, location) in &found {
        *highest = Some(highest.unwrap_or(0).max(location));
    }
    let renames: Vec<(String, String)> = found
        .iter()
        .map(|(_, _, name, location)| (name.clone(), format!("iris_FragData{location}")))
        .collect();
    for (first, end, _, _) in found.into_iter().rev() {
        tokens.drain(first..=end);
    }
    let rename = |word: &mut Token| {
        if let Some((_, to)) = renames.iter().find(|(from, _)| *from == word.text) {
            word.text = to.clone();
        }
    };
    for token in tokens.iter_mut() {
        match token.kind {
            Kind::Word => rename(token),
            Kind::Directive => lexer::in_define(token, &rename),
            _ => {}
        }
    }
}

pub(super) fn explicit_lod(tokens: &mut Vec<Token>) {
    lexer::in_defines(tokens, explicit_lod_in);
    explicit_lod_in(tokens);
}

fn explicit_lod_in(tokens: &mut Vec<Token>) {
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at].is_word("texture")
            && let Some(open) =
                lexer::next_solid(tokens, at + 1).filter(|o| tokens[*o].is_punct('('))
            && let Some((args, close)) = lexer::arguments(tokens, open)
            && args.len() == 2
        {
            tokens[at].text = "textureLod".to_owned();
            tokens.splice(close..close, lexer::lex(", 0.0"));
        }
        at += 1;
    }
}

pub(super) fn unsigned_literals(tokens: &mut [Token]) {
    let mut unsigned: HashMap<String, bool> = HashMap::new();
    for at in 0..tokens.len() {
        let ty = tokens[at].text.as_str();
        if tokens[at].kind != Kind::Word
            || !matches!(ty, "uint" | "int" | "float" | "bool" | "double")
        {
            continue;
        }
        let Some(name) = lexer::next_solid(tokens, at + 1) else {
            continue;
        };
        if tokens[name].kind != Kind::Word {
            continue;
        }
        let is_uint = ty == "uint";
        unsigned
            .entry(tokens[name].text.clone())
            .and_modify(|known| *known &= is_uint)
            .or_insert(is_uint);
    }
    if !unsigned.values().any(|u| *u) {
        return;
    }
    lexer::in_defines(tokens, |body| suffix_literals(body, &unsigned));
    suffix_literals(tokens, &unsigned);
}

fn suffix_literals(tokens: &mut [Token], unsigned: &HashMap<String, bool>) {
    for at in 0..tokens.len() {
        if !matches!(tokens[at].text.as_str(), "min" | "max" | "clamp")
            || tokens[at].kind != Kind::Word
        {
            continue;
        }
        let Some(open) = lexer::next_solid(tokens, at + 1).filter(|o| tokens[*o].is_punct('('))
        else {
            continue;
        };
        let Some((ranges, _)) = lexer::arguments(tokens, open) else {
            continue;
        };
        let arguments: Vec<(usize, bool)> = ranges
            .iter()
            .filter_map(|&(from, to)| {
                let solid: Vec<usize> = (from..to).filter(|j| !tokens[*j].is_trivia()).collect();
                let &first = solid.first()?;
                let literal = solid.len() == 1
                    && tokens[first].kind == Kind::Number
                    && literal::int(&tokens[first].text).is_some()
                    && !tokens[first].text.ends_with(['u', 'U']);
                Some((first, literal))
            })
            .collect();
        let beside_uint = arguments
            .iter()
            .any(|(first, literal)| !literal && unsigned.get(&tokens[*first].text) == Some(&true));
        if beside_uint {
            for (first, literal) in arguments {
                if literal {
                    tokens[first].text.push('u');
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shadowed(source: &str) -> String {
        let mut tokens = lexer::lex(source);
        shadow_lookups(&mut tokens);
        lexer::render(&tokens)
    }

    fn rewritten(source: &str, pass: fn(&mut Vec<Token>)) -> String {
        let mut tokens = lexer::lex(source);
        pass(&mut tokens);
        lexer::render(&tokens)
    }

    #[test]
    fn a_component_passed_to_inout_goes_through_a_temporary() {
        let out = rewritten(
            "void fix(inout float d) { d += 1.0; }\nvoid main() { vec3 p; fix(p.z); fix(1.0); }",
            component_out_arguments,
        );
        assert!(
            out.contains("{ float iris_out0 = p.z; fix(iris_out0); p.z = iris_out0; }"),
            "{out}"
        );
        assert!(out.contains("fix(1.0);"), "{out}");
        assert!(!out.contains("void { "), "{out}");
    }

    #[test]
    fn a_switch_that_returns_assigns_and_breaks() {
        let out = rewritten(
            "vec3 f(uint m) { switch (m) { case 1u: return vec3(1.0); default: return vec3(2.0); } }",
            switch_returns,
        );
        assert!(
            out.contains("iris_sw_ret0 = vec3(1.0); iris_sw_done0 = true; break;"),
            "{out}"
        );
        assert!(
            out.contains("if (iris_sw_done0) { return iris_sw_ret0; }"),
            "{out}"
        );
    }

    #[test]
    fn a_function_that_can_run_off_its_end_gets_a_return() {
        let out = rewritten(
            "float f(float x) { if (x > 0.0) { return 1.0; } }\nfloat g() { return 2.0; }",
            missing_returns,
        );
        assert!(
            out.contains("return 0.0; }\nfloat g() { return 2.0; }"),
            "{out}"
        );
    }

    #[test]
    fn a_legacy_shadow_lookup_is_wrapped_in_a_vec4() {
        assert_eq!(
            shadowed("float a = shadow2D(s, p).x;"),
            "float a = vec4(texture(s, p)).x;"
        );
        assert_eq!(
            shadowed("shadow2DLod (s, vec3(p, f(q)), 0.0)"),
            "vec4(textureLod (s, vec3(p, f(q)), 0.0))"
        );
        assert_eq!(
            shadowed("#define SH(p) shadow2D(s, p).x\n"),
            "#define SH(p) vec4(texture(s, p)).x\n"
        );
    }
}
