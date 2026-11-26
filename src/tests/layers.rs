//! The engine's four modules, each naming only the modules below it.

use std::collections::BTreeSet;

/// The engine's modules, from the bottom.
const LAYERS: [(&str, &str); 4] = [
    ("wire", include_str!("../wire.rs")),
    ("policy", include_str!("../policy.rs")),
    ("hooks", include_str!("../hooks.rs")),
    ("guard", include_str!("../guard.rs")),
];

/// The names a path reaches the crate root through.
const ROOTS: [&str; 3] = ["crate", "lez_authority", "super"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token<'a> {
    Ident(&'a str),
    /// `::`
    Path,
    Punct(char),
}

use Token::{Ident, Path, Punct};

/// A source's code as tokens, without comments and literals, and the text of its doc comments.
fn lex(source: &str) -> (Vec<Token<'_>>, Vec<&str>) {
    let (mut tokens, mut docs) = (Vec::new(), Vec::new());
    let mut rest = source;
    while let Some(c) = rest.chars().next() {
        if let Some(comment) = rest.strip_prefix("//") {
            let line = &comment[..comment.find('\n').unwrap_or(comment.len())];
            if let Some(doc) = line.strip_prefix(['/', '!']) {
                docs.push(doc);
            }
            rest = &comment[line.len()..];
        } else if rest.starts_with("/*") {
            rest = after_block_comment(rest);
        } else if let Some(string) = rest.strip_prefix('"') {
            rest = after_string(string);
        } else if let Some(quoted) = rest.strip_prefix('\'') {
            rest = after_char_or_lifetime(quoted);
        } else if c.is_alphabetic() || c == '_' {
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let (ident, after) = rest.split_at(end);
            let hashes = after.len() - after.trim_start_matches('#').len();
            if matches!(ident, "r" | "br") && after[hashes..].starts_with('"') {
                let close = format!("\"{}", "#".repeat(hashes));
                let body = &after[hashes + 1..];
                rest = body.find(&close).map_or("", |at| &body[at + close.len()..]);
            } else {
                tokens.push(Ident(ident));
                rest = after;
            }
        } else if c.is_ascii_digit() {
            rest = rest.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
        } else if let Some(after) = rest.strip_prefix("::") {
            tokens.push(Path);
            rest = after;
        } else {
            if !c.is_whitespace() {
                tokens.push(Punct(c));
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    (tokens, docs)
}

fn after_block_comment(mut rest: &str) -> &str {
    let mut depth = 0;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("/*") {
            depth += 1;
            rest = after;
        } else if let Some(after) = rest.strip_prefix("*/") {
            depth -= 1;
            rest = after;
            if depth == 0 {
                break;
            }
        } else {
            rest = &rest[c.len_utf8()..];
        }
    }
    rest
}

fn after_string(mut rest: &str) -> &str {
    while let Some(at) = rest.find(['"', '\\']) {
        if !rest[at..].starts_with('\\') {
            return &rest[at + 1..];
        }
        let escaped = rest[at + 1..].chars().next().map_or(0, char::len_utf8);
        rest = &rest[at + 1 + escaped..];
    }
    ""
}

fn after_char_or_lifetime(rest: &str) -> &str {
    let mut chars = rest.chars();
    match chars.next() {
        Some('\\') => {
            let escaped = &rest[1 + chars.next().map_or(0, char::len_utf8)..];
            escaped.find('\'').map_or("", |at| &escaped[at + 1..])
        }
        Some(c) if chars.next() == Some('\'') => &rest[c.len_utf8() + 1..],
        _ => rest.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_'),
    }
}

/// What an open brace belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Frame<'a> {
    Module,
    /// An impl block of the type's own methods.
    Inherent(&'a str),
    Other,
}

/// The frame of a brace after `header`, the tokens since the last `;`, `{` or `}`.
fn frame<'a>(mut header: &[Token<'a>]) -> Frame<'a> {
    loop {
        header = match header {
            [Punct('#'), rest @ ..] => skip_group(rest, '[', ']'),
            [Ident("pub"), rest @ ..] if rest.first() == Some(&Punct('(')) => {
                skip_group(rest, '(', ')')
            }
            [Ident("pub" | "unsafe"), rest @ ..] => rest,
            _ => break,
        };
    }
    match header {
        [Ident("mod"), ..] => Frame::Module,
        [Ident("impl"), rest @ ..] if !rest.contains(&Ident("for")) => {
            match skip_group(rest, '<', '>').first() {
                Some(Ident(ty)) => Frame::Inherent(ty),
                _ => Frame::Other,
            }
        }
        _ => Frame::Other,
    }
}

/// `tokens` after the `open` ... `close` group they start with, if they start with one.
fn skip_group<'t, 'a>(tokens: &'t [Token<'a>], open: char, close: char) -> &'t [Token<'a>] {
    if tokens.first() != Some(&Punct(open)) {
        return tokens;
    }
    let mut depth = 0;
    for (at, token) in tokens.iter().enumerate() {
        if *token == Punct(open) {
            depth += 1;
        } else if *token == Punct(close) {
            depth -= 1;
            if depth == 0 {
                return &tokens[at + 1..];
            }
        }
    }
    &[]
}

/// Each token with the frames it sits in.
fn framed<'t, 'a>(tokens: &'t [Token<'a>]) -> Vec<(&'t Token<'a>, Vec<Frame<'a>>)> {
    let (mut frames, mut header, mut out) = (Vec::new(), 0, Vec::new());
    for (at, token) in tokens.iter().enumerate() {
        out.push((token, frames.clone()));
        match token {
            Punct('{') => {
                frames.push(frame(&tokens[header..at]));
                header = at + 1;
            }
            Punct('}') => {
                frames.pop();
                header = at + 1;
            }
            Punct(';') => header = at + 1,
            _ => {}
        }
    }
    out
}

/// What a module defines: its top items, which `crate::` reaches through the root's
/// re-exports, the methods it adds to a type defined elsewhere, and every function name in it.
#[derive(Default)]
struct Defined<'a> {
    items: BTreeSet<&'a str>,
    added: BTreeSet<&'a str>,
    fns: BTreeSet<&'a str>,
}

fn defined<'a>(tokens: &[Token<'a>]) -> Defined<'a> {
    const ITEMS: [&str; 9] = [
        "fn", "struct", "enum", "trait", "const", "static", "type", "mod", "union",
    ];
    let mut defined = Defined::default();
    let mut methods = Vec::new();
    for (at, (token, frames)) in framed(tokens).into_iter().enumerate() {
        let (Ident(keyword), Some(Ident(name))) = (token, tokens.get(at + 1)) else {
            continue;
        };
        if !ITEMS.contains(keyword) || matches!(*name, "fn" | "_" | "mut") {
            continue;
        }
        if *keyword == "fn" {
            defined.fns.insert(*name);
            if let [Frame::Inherent(ty)] = frames[..] {
                methods.push((ty, *name));
            }
        }
        if frames.is_empty() {
            defined.items.insert(*name);
        }
    }
    defined.added = methods
        .into_iter()
        .filter(|(ty, _)| !defined.items.contains(ty))
        .map(|(_, method)| method)
        .collect();
    defined
}

/// What a module must not name: the modules above it, their top items, and the methods they add
/// to the types below.
#[derive(Default)]
struct Above<'a> {
    modules: BTreeSet<&'a str>,
    items: BTreeSet<&'a str>,
    added: BTreeSet<&'a str>,
}

/// Each place a module's source names something above it, or the root other than through a
/// module of `below`, the module's own included.
fn names_above(source: &str, below: &[&str], above: &Above) -> Vec<String> {
    let (tokens, docs) = lex(source);
    let mut found = Vec::new();
    for (at, (token, frames)) in framed(&tokens).into_iter().enumerate() {
        let next = tokens.get(at + 1);
        if let Ident(root) = *token
            && next == Some(&Path)
            && (root != "super" || !frames.contains(&Frame::Module))
            && ROOTS.contains(&root)
        {
            for first in first_segments(&tokens[at + 2..]) {
                match first {
                    Ident(module) if below.contains(&module) => {}
                    Ident(name) => found.push(format!("names `{root}::{name}`")),
                    _ => found.push(format!("imports `{root}::*`")),
                }
            }
        }
        if let (Punct('.') | Path, Some(Ident(method))) = (token, next)
            && above.added.contains(method)
        {
            found.push(format!("calls `{method}`, which a module above adds"));
        }
        if let (Ident("mod"), Some(Ident(name)), Some(Punct(';'))) =
            (token, next, tokens.get(at + 2))
        {
            found.push(format!("keeps `mod {name}` in a file of its own"));
        }
    }
    for link in docs.iter().flat_map(|doc| links(doc)) {
        let path = link.rsplit('@').next().unwrap_or(link);
        if path.contains("://") {
            continue;
        }
        let segments: Vec<&str> = path
            .split("::")
            .map(|segment| {
                let end = segment
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(segment.len());
                &segment[..end]
            })
            .collect();
        let named_above = match segments[..] {
            [root, next, ..] if ROOTS.contains(&root) => !below.contains(&next),
            [first, ..] => above.modules.contains(first) || above.items.contains(first),
            [] => false,
        };
        if named_above || segments.iter().any(|segment| above.added.contains(segment)) {
            found.push(format!("links to `{link}`"));
        }
    }
    found
}

/// The first segment of the path after a root's `::`, or of each path in a group there.
fn first_segments<'a>(after: &[Token<'a>]) -> Vec<Token<'a>> {
    if after.first() != Some(&Punct('{')) {
        return after.first().copied().into_iter().collect();
    }
    let (mut firsts, mut depth, mut expected) = (Vec::new(), 0, false);
    for token in after {
        match token {
            Punct('{') => {
                depth += 1;
                expected = depth == 1;
            }
            Punct('}') => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Punct(',') if depth == 1 => expected = true,
            first if expected => {
                firsts.push(*first);
                expected = false;
            }
            _ => {}
        }
    }
    firsts
}

/// The code of each `` [`x`] `` link in a doc line, and the target of each `[..](x)`.
fn links(doc: &str) -> Vec<&str> {
    let mut links = Vec::new();
    for (open, close) in [("[`", "`]"), ("](", ")")] {
        for (at, _) in doc.match_indices(open) {
            let start = at + open.len();
            if let Some(end) = doc[start..].find(close) {
                links.push(&doc[start..start + end]);
            }
        }
    }
    links
}

/// What each layer must not name.
fn above(defined: &[Defined<'static>], layer: usize) -> Above<'static> {
    let mine: BTreeSet<&str> = defined[..=layer]
        .iter()
        .flat_map(|names| names.items.iter().chain(&names.fns))
        .copied()
        .collect();
    let higher = &defined[layer + 1..];
    let not_mine = |name: &&str| !mine.contains(name);
    Above {
        modules: LAYERS[layer + 1..].iter().map(|(name, _)| *name).collect(),
        items: higher
            .iter()
            .flat_map(|names| &names.items)
            .copied()
            .filter(not_mine)
            .collect(),
        added: higher
            .iter()
            .flat_map(|names| &names.added)
            .copied()
            .filter(not_mine)
            .collect(),
    }
}

#[test]
fn each_module_names_only_the_modules_below_it() {
    let defined: Vec<Defined<'static>> = LAYERS
        .iter()
        .map(|(_, source)| defined(&lex(source).0))
        .collect();
    let mut found = Vec::new();
    for (layer, (name, source)) in LAYERS.iter().enumerate() {
        assert!(
            !defined[layer].items.is_empty(),
            "src/{name}.rs defines nothing"
        );
        let below: Vec<&str> = LAYERS[..=layer].iter().map(|(name, _)| *name).collect();
        for place in names_above(source, &below, &above(&defined, layer)) {
            found.push(format!("src/{name}.rs {place}"));
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_engine_has_the_four_modules_and_no_other() {
    let (tokens, _) = lex(include_str!("../lib.rs"));
    let declared: BTreeSet<&str> = tokens
        .windows(2)
        .filter_map(|pair| match pair {
            [Ident("mod"), Ident(name)] => Some(*name),
            _ => None,
        })
        .collect();
    let expected: BTreeSet<&str> = LAYERS
        .iter()
        .map(|(name, _)| *name)
        .chain(["compile_fail", "tests"])
        .collect();
    assert_eq!(declared, expected);
}

#[test]
fn the_check_finds_each_way_up() {
    let above = Above {
        modules: BTreeSet::from(["hooks"]),
        items: BTreeSet::from(["Hooks"]),
        added: BTreeSet::from(["own_message"]),
    };
    let below = ["wire", "policy"];
    for source in [
        "use crate::*;",
        "use super::*;",
        "use crate::Role;",
        "use crate::hooks::Hooks;",
        "use crate::{wire::Op, hooks::Hooks};",
        "fn f() { lez_authority::hooks::receive(); }",
        "fn f() { ::lez_authority::Hooks::receive(); }",
        "fn f(ctx: &Ctx) { ctx.own_message::<R>(KEY, Vec::new()); }",
        "fn f() { Ctx::own_message::<R>; }",
        "mod hidden;",
        "/// Runs under [`Hooks`].\nfn f() {}",
        "/// Runs under [the hooks](crate::Hooks).\nfn f() {}",
        "/// See [`Ctx::own_message`].\nfn f() {}",
        "fn f<'a>(_: &'a str) { let _ = '\"'; crate::hooks::own_actor(); }",
        "fn f() { let _ = r#\"a\"b\"#; crate::hooks::own_actor(); }",
    ] {
        assert_eq!(names_above(source, &below, &above).len(), 1, "{source}");
    }
    for source in [
        "use crate::{wire::{self, Op}, policy::Ctx};",
        "pub(crate) fn f() { crate::wire::sent(); }",
        "mod inner { use super::*; }",
        "// crate::hooks::Hooks\n/* crate::hooks */ fn f() { \"crate::hooks\"; }",
        "/// See [`Ctx::check`] and [the guard](https://example.org/a::Hooks).\nfn f() {}",
        "fn f<'a>(c: &'a str) -> char { 'h' }",
    ] {
        assert_eq!(
            names_above(source, &below, &above),
            Vec::<String>::new(),
            "{source}"
        );
    }
}
