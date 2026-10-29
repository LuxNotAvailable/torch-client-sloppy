//! Expands a pack's macros before any token pass runs.
//!
//! The preprocessor only decides `#if` branches and leaves `#define`s for
//! naga's own preprocessor, which runs after every transform. Code a macro
//! produces (a sampler behind `#define tex colortex0`, an unsized array in a
//! function-like macro, a uniform declared through one) was therefore never
//! seen by the passes that rewrite it. Expanded here, with C's rules: a macro
//! is not expanded inside its own expansion, arguments are expanded before they
//! are substituted, and the result is rescanned. A macro that stringifies or
//! pastes (`#`, `##`) or takes `...` keeps its `#define` for naga.

use std::collections::HashMap;

use super::preprocess::Defines;
use super::transform::lexer::{self, Kind, Token};

const MAX_DEPTH: usize = 64;

struct Macro {
    params: Option<Vec<String>>,
    body: Vec<Token>,
}

enum Line {
    Define(String, Option<Macro>),
    Undef(String),
    Other,
}

fn is_ident(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn body(text: &str) -> Vec<Token> {
    let tokens: Vec<Token> = lexer::lex(text)
        .into_iter()
        .filter(|t| t.kind != Kind::Comment)
        .collect();
    let first = tokens
        .iter()
        .position(|t| !t.is_trivia())
        .unwrap_or(tokens.len());
    let last = tokens
        .iter()
        .rposition(|t| !t.is_trivia())
        .map_or(first, |i| i + 1);
    tokens[first..last].to_vec()
}

/// A `#define` this pass can expand, `None` for one it leaves to naga.
fn parse_define(rest: &str) -> Option<(String, Option<Macro>)> {
    let rest = rest.trim_start();
    let name_end = rest
        .find(|c: char| !(c == '_' || c.is_ascii_alphanumeric()))
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at(name_end);
    if !is_ident(name) {
        return None;
    }
    let (params, text) = match after.strip_prefix('(') {
        Some(list) => {
            let close = list.find(')')?;
            let params: Vec<String> = list[..close]
                .split(',')
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty())
                .collect();
            if params.iter().any(|p| !is_ident(p)) {
                return Some((name.to_owned(), None));
            }
            (Some(params), &list[close + 1..])
        }
        None => (None, after),
    };
    if text.contains('#') {
        return Some((name.to_owned(), None));
    }
    Some((
        name.to_owned(),
        Some(Macro {
            params,
            body: body(text),
        }),
    ))
}

fn classify(directive: &str) -> Line {
    let joined = directive.replace("\\\r\n", " ").replace("\\\n", " ");
    let Some(rest) = joined.trim_start().strip_prefix('#') else {
        return Line::Other;
    };
    let rest = rest.trim_start();
    if let Some(define) = rest.strip_prefix("define")
        && define.starts_with(char::is_whitespace)
    {
        return match parse_define(define) {
            Some((name, mac)) => Line::Define(name, mac),
            None => Line::Other,
        };
    }
    if let Some(undef) = rest.strip_prefix("undef")
        && undef.starts_with(char::is_whitespace)
    {
        return Line::Undef(undef.trim().to_owned());
    }
    Line::Other
}

struct Expander<'a> {
    macros: &'a HashMap<String, Macro>,
    disabled: Vec<String>,
}

impl Expander<'_> {
    fn run(&mut self, tokens: &[Token], depth: usize) -> Vec<Token> {
        let mut out = Vec::with_capacity(tokens.len());
        let mut at = 0;
        while at < tokens.len() {
            let token = &tokens[at];
            if token.kind == Kind::Word
                && depth < MAX_DEPTH
                && !self.disabled.contains(&token.text)
                && let Some(mac) = self.macros.get(&token.text)
            {
                match &mac.params {
                    None => {
                        let body = mac.body.clone();
                        let expanded = self.within(&token.text, &body, depth);
                        push_spaced(&mut out, expanded);
                        at += 1;
                        continue;
                    }
                    Some(params) => {
                        if let Some((args, close)) = call(tokens, at, params.len()) {
                            let args: Vec<Vec<Token>> = args
                                .iter()
                                .map(|&(start, end)| self.run(&tokens[start..end], depth + 1))
                                .collect();
                            let mut substituted = Vec::with_capacity(mac.body.len());
                            for piece in &mac.body {
                                match params.iter().position(|p| piece.is_word(p)) {
                                    Some(i) => {
                                        substituted.push(Token::space(" "));
                                        substituted.extend(args[i].iter().cloned());
                                        substituted.push(Token::space(" "));
                                    }
                                    None => substituted.push(piece.clone()),
                                }
                            }
                            let expanded = self.within(&token.text, &substituted, depth);
                            push_spaced(&mut out, expanded);
                            at = close + 1;
                            continue;
                        }
                    }
                }
            }
            out.push(token.clone());
            at += 1;
        }
        out
    }

    /// Rescans `tokens`, the expansion of `name`, with `name` itself off.
    fn within(&mut self, name: &str, tokens: &[Token], depth: usize) -> Vec<Token> {
        self.disabled.push(name.to_owned());
        let out = self.run(tokens, depth + 1);
        self.disabled.pop();
        out
    }
}

/// The argument ranges and closing `)` of a call to a `count`-parameter macro
/// at `at`, if one follows its name.
fn call(tokens: &[Token], at: usize, count: usize) -> Option<(Vec<(usize, usize)>, usize)> {
    let open = lexer::next_solid(tokens, at + 1).filter(|o| tokens[*o].is_punct('('))?;
    let (args, close) = lexer::arguments(tokens, open)?;
    let empty = |&(start, end): &(usize, usize)| tokens[start..end].iter().all(Token::is_trivia);
    match (count, args.len()) {
        (0, 0) => Some((Vec::new(), close)),
        (0, 1) if empty(&args[0]) => Some((Vec::new(), close)),
        (n, m) if n == m => Some((args, close)),
        _ => None,
    }
}

/// Spaces either side, so an expansion never fuses with a neighbour
/// (`x-NEG` with `NEG` as `-1` must not become `x--1`).
fn push_spaced(out: &mut Vec<Token>, expanded: Vec<Token>) {
    out.push(Token::space(" "));
    out.extend(expanded);
    out.push(Token::space(" "));
}

/// `source` with its macros expanded and the `#define`s it expanded gone;
/// `seed` holds the macros defined before the source (the pack environment).
pub(crate) fn expand(source: &str, seed: &Defines) -> String {
    let tokens = lexer::lex(source);
    let mut macros: HashMap<String, Macro> = seed
        .iter()
        .filter(|(name, _)| is_ident(name))
        .map(|(name, value)| {
            (
                name.to_owned(),
                Macro {
                    params: None,
                    body: body(value),
                },
            )
        })
        .collect();
    let mut out = Vec::with_capacity(tokens.len());
    let mut at = 0;
    while at < tokens.len() {
        let token = &tokens[at];
        if token.kind == Kind::Directive {
            match classify(&token.text) {
                Line::Define(name, Some(mac)) => {
                    macros.insert(name, mac);
                    // Keep the line count, for error messages.
                    out.push(Token::space(&"\n".repeat(token.text.matches('\n').count())));
                }
                Line::Define(name, None) => {
                    macros.remove(&name);
                    out.push(token.clone());
                }
                Line::Undef(name) => {
                    macros.remove(&name);
                    out.push(token.clone());
                }
                Line::Other => out.push(token.clone()),
            }
            at += 1;
            continue;
        }
        let start = at;
        while at < tokens.len() && tokens[at].kind != Kind::Directive {
            at += 1;
        }
        let mut expander = Expander {
            macros: &macros,
            disabled: Vec::new(),
        };
        out.extend(expander.run(&tokens[start..at], 0));
    }
    lexer::render(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> String {
        let out = expand(source, &Defines::new());
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn object_and_function_macros_expand_in_code() {
        assert_eq!(
            run(
                "#define tex colortex0\n#define SQ(x) ((x) * (x))\nvec4 c = texture(tex, uv) * SQ(a + 1);\n"
            ),
            "vec4 c = texture( colortex0 , uv) * (( a + 1 ) * ( a + 1 )) ;"
        );
    }

    #[test]
    fn a_macro_is_not_expanded_inside_itself() {
        assert_eq!(run("#define x x + 1\nfloat y = x;\n"), "float y = x + 1 ;");
        assert_eq!(
            run("#define a b\n#define b a\nfloat y = a;\n"),
            "float y = a ;",
            "a mutual pair stops where C's would"
        );
    }

    #[test]
    fn nested_calls_and_arguments_expand() {
        assert_eq!(
            run(
                "#define ONE 1.0\n#define ADD(p, q) (p + q)\n#define TWICE(v) ADD(v, v)\nfloat y = TWICE(ONE);\n"
            ),
            "float y = ( 1.0 + 1.0 ) ;"
        );
    }

    #[test]
    fn an_expansion_never_fuses_with_its_neighbours() {
        let out = expand("#define NEG -1\nint y = x-NEG;\n", &Defines::new());
        assert!(!out.contains("--"), "{out}");
    }

    #[test]
    fn pasting_and_undef_are_respected() {
        let out =
            run("#define CAT(a, b) a##b\n#define K 2\nint CAT(x, y) = K;\n#undef K\nint z = K;\n");
        assert!(out.contains("#define CAT(a, b) a##b"), "{out}");
        assert!(out.contains("int CAT(x, y) = 2"), "{out}");
        assert!(out.ends_with("int z = K;"), "{out}");
    }

    #[test]
    fn the_environment_seeds_the_table_and_lines_keep_their_numbers() {
        let mut seed = Defines::new();
        seed.define("MC_VERSION", "12100");
        let source = "#version 330\n#define A \\\n  3\nint v = MC_VERSION + A; // done\n";
        let out = expand(source, &seed);
        assert_eq!(out.lines().count(), source.lines().count(), "{out}");
        assert!(out.contains("12100"), "{out}");
        assert!(out.starts_with("#version 330"), "{out}");
        assert!(out.contains("// done"), "{out}");
    }

    #[test]
    fn a_macro_built_array_reaches_the_sizing_pass() {
        let source = "#define printString(string) { \
	uint[] characters = uint[] string; \
	for (int i = 0; i < characters.length(); ++i) printChar(characters[i]); \
}
void f() { printString((_N, _a, _N)); }
";
        let mut tokens = lexer::lex(&expand(source, &Defines::new()));
        super::super::transform::declarations::size_unsized_arrays(&mut tokens);
        let out = lexer::render(&tokens);
        assert!(out.contains("uint[3] characters = uint[3]"), "{out}");
    }

    #[test]
    fn a_function_macro_named_without_a_call_is_left_alone() {
        assert_eq!(run("#define f(x) x\nfloat f = 1.0;\n"), "float f = 1.0;");
    }
}
