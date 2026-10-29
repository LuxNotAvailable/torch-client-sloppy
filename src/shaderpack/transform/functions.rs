use std::collections::{HashMap, HashSet};

use super::lexer::{self, Kind, Token};

/// A top-level function definition or prototype.
struct Function {
    name: String,
    /// First token of its return type, through its closing `}` or `;`.
    span: (usize, usize),
    /// Its body's tokens, when it is a definition.
    body: Option<(usize, usize)>,
}

fn top_level_functions(tokens: &[Token]) -> Vec<Function> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut statement_start = 0;
    let mut at = 0;
    while at < tokens.len() {
        let token = &tokens[at];
        if token.is_punct('{') {
            depth += 1;
        } else if token.is_punct('}') {
            depth -= 1;
            if depth == 0 {
                statement_start = at + 1;
            }
        } else if depth == 0 && (token.is_punct(';') || token.kind == Kind::Directive) {
            statement_start = at + 1;
        } else if depth == 0
            && token.kind == Kind::Word
            && let Some(function) = function_at(tokens, at, statement_start)
        {
            at = function.span.1 + 1;
            statement_start = at;
            out.push(function);
            continue;
        }
        at += 1;
    }
    out
}

fn function_at(tokens: &[Token], name: usize, statement_start: usize) -> Option<Function> {
    let open = lexer::next_solid(tokens, name + 1).filter(|o| tokens[*o].is_punct('('))?;
    let ty = lexer::prev_solid(tokens, name).filter(|t| *t >= statement_start)?;
    if !(tokens[ty].kind == Kind::Word || tokens[ty].is_punct(']')) {
        return None;
    }
    let close = lexer::matching(tokens, open)?;
    let after = lexer::next_solid(tokens, close + 1)?;
    if tokens[after].is_punct(';') {
        return Some(Function {
            name: tokens[name].text.clone(),
            span: (first_solid(tokens, statement_start, name), after),
            body: None,
        });
    }
    if !tokens[after].is_punct('{') {
        return None;
    }
    let end = lexer::matching(tokens, after)?;
    Some(Function {
        name: tokens[name].text.clone(),
        span: (first_solid(tokens, statement_start, name), end),
        body: Some((after, end)),
    })
}

fn first_solid(tokens: &[Token], from: usize, limit: usize) -> usize {
    (from..limit)
        .find(|i| !tokens[*i].is_trivia())
        .unwrap_or(limit)
}

/// Removes functions nothing reachable from `main` calls.
///
/// Packs ship helpers that name uniforms the including program never
/// declares; a driver never sees them because nothing calls them, and Iris's
/// toolchain drops them the same way. naga refuses the undeclared name.
pub(super) fn drop_unused_functions(tokens: &mut Vec<Token>) {
    let functions = top_level_functions(tokens);
    if functions.is_empty() {
        return;
    }
    let names: HashSet<&str> = functions.iter().map(|f| f.name.as_str()).collect();
    let mut bodies: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
    for f in &functions {
        if let Some(body) = f.body {
            bodies.entry(f.name.as_str()).or_default().push(body);
        }
    }

    let mut inside = vec![false; tokens.len()];
    for f in &functions {
        inside[f.span.0..=f.span.1].fill(true);
    }
    let mut reached: HashSet<String> = HashSet::from(["main".to_owned()]);
    for (i, token) in tokens.iter().enumerate() {
        if token.kind == Kind::Word && !inside[i] && names.contains(token.text.as_str()) {
            reached.insert(token.text.clone());
        }
        if let Some(body) = lexer::define_body(token) {
            for word in body.iter().filter(|t| t.kind == Kind::Word) {
                if names.contains(word.text.as_str()) {
                    reached.insert(word.text.clone());
                }
            }
        }
    }

    let mut queue: Vec<String> = reached.iter().cloned().collect();
    while let Some(name) = queue.pop() {
        for &(open, close) in bodies.get(name.as_str()).into_iter().flatten() {
            for token in &tokens[open..=close] {
                if token.kind == Kind::Word
                    && names.contains(token.text.as_str())
                    && reached.insert(token.text.clone())
                {
                    queue.push(token.text.clone());
                }
            }
        }
    }

    for f in functions.iter().rev() {
        if !reached.contains(&f.name) {
            tokens.splice(f.span.0..=f.span.1, [Token::newline()]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dropped(source: &str) -> String {
        let mut tokens = lexer::lex(source);
        drop_unused_functions(&mut tokens);
        lexer::render(&tokens)
    }

    #[test]
    fn an_uncalled_helper_naming_an_undeclared_uniform_goes() {
        let out = dropped(
            "uniform mat4 inv;\nvec2 screen2uv(vec3 s) { return s.xy * gbufferProjection[0].x; }\n\
             vec3 uv2screen(vec2 uv) { return vec3(inv[0].x * uv, 1.0); }\n\
             void main() { vec3 p = uv2screen(vec2(0.5)); }\n",
        );
        assert!(!out.contains("gbufferProjection"), "{out}");
        assert!(out.contains("vec3 uv2screen(vec2 uv)"), "{out}");
        assert!(out.contains("void main()"), "{out}");
    }

    #[test]
    fn calls_are_followed_through_helpers_overloads_and_prototypes() {
        let out = dropped(
            "float b(float x);\nfloat a(float x) { return b(x); }\nfloat b(float x) { return x; }\n\
             float b(vec2 x) { return x.x; }\nfloat lonely() { return 1.0; }\n\
             void main() { float y = a(1.0); }\n",
        );
        assert_eq!(out.matches("float b(").count(), 3, "{out}");
        assert!(out.contains("float a(float x)"), "{out}");
        assert!(!out.contains("lonely"), "{out}");
    }

    #[test]
    fn a_global_initializer_or_a_macro_keeps_its_function() {
        let out = dropped(
            "float k() { return 2.0; }\nconst float two = k();\n\
             float twice(float x) { return 2.0 * x; }\n#define TWICE(x) twice(x)\n\
             void main() { float y = TWICE(two); }\n",
        );
        assert!(out.contains("float k()"), "{out}");
        assert!(out.contains("float twice(float x)"), "{out}");
    }

    #[test]
    fn declarations_that_merely_look_like_calls_stay() {
        let source =
            "layout(location = 0) out vec4 c;\nuniform float w;\nvoid main() { c = vec4(w); }\n";
        assert_eq!(dropped(source), source);
    }
}
