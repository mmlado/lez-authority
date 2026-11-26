//! The engine's four modules, each naming only the modules below it.

use std::collections::BTreeSet;

/// The engine's modules, from the bottom.
const LAYERS: [(&str, &str); 4] = [
    ("wire", include_str!("../wire.rs")),
    ("policy", include_str!("../policy.rs")),
    ("hooks", include_str!("../hooks.rs")),
    ("guard", include_str!("../guard.rs")),
];

/// The names a doc link or a path in an attribute reaches the crate root through.
const ROOTS: [&str; 3] = ["crate", "lez_authority", "super"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token<'a> {
    Ident(&'a str),
    /// `::`
    Path,
    Punct(char),
    /// A string literal's text.
    Str(&'a str),
}

use Token::{Ident, Path, Punct, Str};

/// A source's code as tokens, without comments, and the text of its line doc comments.
struct Lexed<'a> {
    tokens: Vec<Token<'a>>,
    docs: Vec<&'a str>,
    /// Doc comments written as `/** */` or `/*! */`.
    block_docs: usize,
}

fn lex(source: &str) -> Lexed<'_> {
    let mut lexed = Lexed {
        tokens: Vec::new(),
        docs: Vec::new(),
        block_docs: 0,
    };
    let mut rest = source;
    while let Some(c) = rest.chars().next() {
        if let Some(comment) = rest.strip_prefix("//") {
            let line = &comment[..comment.find('\n').unwrap_or(comment.len())];
            if let Some(doc) = line.strip_prefix(['/', '!']) {
                lexed.docs.push(doc);
            }
            rest = &comment[line.len()..];
        } else if rest.starts_with("/*") {
            if rest.starts_with("/*!")
                || (rest.starts_with("/**") && !rest[3..].starts_with(['*', '/']))
            {
                lexed.block_docs += 1;
            }
            rest = after_block_comment(rest);
        } else if let Some(string) = rest.strip_prefix('"') {
            rest = after_string(string);
            let text = &string[..string.len() - rest.len()];
            lexed
                .tokens
                .push(Str(text.strip_suffix('"').unwrap_or(text)));
        } else if let Some(quoted) = rest.strip_prefix('\'') {
            rest = after_char_or_lifetime(quoted);
        } else if c.is_alphabetic() || c == '_' {
            let (ident, after) = rest.split_at(ident_end(rest));
            let hashes = after.len() - after.trim_start_matches('#').len();
            if matches!(ident, "r" | "br" | "cr") && after[hashes..].starts_with('"') {
                let close = format!("\"{}", "#".repeat(hashes));
                let body = &after[hashes + 1..];
                rest = match body.find(&close) {
                    Some(at) => {
                        lexed.tokens.push(Str(&body[..at]));
                        &body[at + close.len()..]
                    }
                    None => "",
                };
            } else if ident == "r" && hashes == 1 && ident_end(&after[1..]) > 0 {
                // A raw identifier names what its plain spelling names.
                let (raw, after) = after[1..].split_at(ident_end(&after[1..]));
                lexed.tokens.push(Ident(raw));
                rest = after;
            } else {
                lexed.tokens.push(Ident(ident));
                rest = after;
            }
        } else if c.is_ascii_digit() {
            rest = rest.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
        } else if let Some(after) = rest.strip_prefix("::") {
            lexed.tokens.push(Path);
            rest = after;
        } else {
            if !c.is_whitespace() {
                lexed.tokens.push(Punct(c));
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    lexed
}

fn ident_end(source: &str) -> usize {
    if !source.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        return 0;
    }
    source
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(source.len())
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
    /// An impl block of a trait for the type.
    TraitImpl(&'a str),
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
        [Ident("impl"), rest @ ..] => {
            let rest = skip_group(rest, '<', '>');
            match rest.iter().position(|token| *token == Ident("for")) {
                None => type_name(rest).map_or(Frame::Other, Frame::Inherent),
                Some(at) => type_name(&rest[at + 1..]).map_or(Frame::Other, Frame::TraitImpl),
            }
        }
        _ => Frame::Other,
    }
}

/// The last segment of the type path `tokens` start with.
fn type_name<'a>(tokens: &[Token<'a>]) -> Option<&'a str> {
    let mut tokens = tokens
        .iter()
        .skip_while(|token| matches!(token, Punct('&') | Path | Ident("mut" | "dyn")));
    let mut name = None;
    while let Some(Ident(segment)) = tokens.next() {
        name = Some(*segment);
        if tokens.next() != Some(&Path) {
            break;
        }
    }
    name
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
/// re-exports, the methods and consts it adds to a type defined elsewhere, every function name
/// in it, and the types defined elsewhere it implements a trait for.
#[derive(Default)]
struct Defined<'a> {
    items: BTreeSet<&'a str>,
    added: BTreeSet<&'a str>,
    fns: BTreeSet<&'a str>,
    impls_for: BTreeSet<&'a str>,
}

fn defined<'a>(tokens: &[Token<'a>]) -> Defined<'a> {
    const ITEMS: [&str; 9] = [
        "fn", "struct", "enum", "trait", "const", "static", "type", "mod", "union",
    ];
    const TYPES: [&str; 5] = ["struct", "enum", "union", "trait", "type"];
    let mut defined = Defined::default();
    let (mut types, mut members, mut impls) = (BTreeSet::new(), Vec::new(), Vec::new());
    for (at, (token, frames)) in framed(tokens).into_iter().enumerate() {
        if let Some(Frame::TraitImpl(ty)) = frames.last() {
            impls.push(*ty);
        }
        let (Ident(keyword), Some(Ident(name))) = (token, tokens.get(at + 1)) else {
            continue;
        };
        if !ITEMS.contains(keyword) || matches!(*name, "fn" | "_" | "mut") {
            continue;
        }
        if TYPES.contains(keyword) {
            types.insert(*name);
        }
        if *keyword == "fn" {
            defined.fns.insert(*name);
        }
        if let (Some(Frame::Inherent(ty)), "fn" | "const") = (frames.last(), *keyword) {
            members.push((*ty, *name));
        }
        if frames.is_empty() {
            defined.items.insert(*name);
        }
    }
    let elsewhere = |ty: &&str| !types.contains(ty) && !defined.items.contains(ty);
    defined.added = members
        .into_iter()
        .filter(|(ty, _)| elsewhere(ty))
        .map(|(_, member)| member)
        .collect();
    defined.impls_for = impls.into_iter().filter(elsewhere).collect();
    defined
}

/// What a module must not name: the modules above it, their top items, and the methods and
/// consts they add to the types below.
#[derive(Default)]
struct Above<'a> {
    modules: BTreeSet<&'a str>,
    items: BTreeSet<&'a str>,
    added: BTreeSet<&'a str>,
}

/// Each place a module's source names something above it, or the root other than through a
/// module of `below`, the module's own included, and each place it holds code this test cannot
/// read.
fn names_above(source: &str, below: &[&str], above: &Above) -> Vec<String> {
    let Lexed {
        tokens,
        docs,
        block_docs,
    } = lex(source);
    let mut found = vec!["writes a doc comment this test does not read".to_string(); block_docs];
    let framed = framed(&tokens);
    let mut at = 0;
    while at < tokens.len() {
        let (token, next) = (tokens[at], tokens.get(at + 1));
        let depth = framed[at].1.iter().filter(|f| **f == Frame::Module).count();
        if token == Ident("use") {
            let end = tokens[at..]
                .iter()
                .position(|token| *token == Punct(';'))
                .map_or(tokens.len(), |end| at + end);
            for path in use_paths(&tokens[at + 1..end]) {
                found.extend(root_named(&path, depth, below));
            }
            at = end;
            continue;
        }
        let starts_path = match token {
            Ident("crate" | "lez_authority") => true,
            Ident("self" | "super") => at == 0 || tokens[at - 1] != Path,
            _ => false,
        };
        if starts_path && next == Some(&Path) {
            found.extend(root_named(&segments(&tokens[at..]), depth, below));
        }
        if let (Punct('.') | Path, Some(Ident(member))) = (token, next)
            && above.added.contains(member)
        {
            found.push(format!("calls `{member}`, which a module above adds"));
        }
        match (token, next, tokens.get(at + 2)) {
            (Ident("mod"), Some(Ident(name)), Some(Punct(';'))) => {
                found.push(format!("keeps `mod {name}` in a file of its own"));
            }
            (Ident("extern"), Some(Ident("crate")), _) => {
                found.push("renames a crate with `extern crate`".into());
            }
            (Ident("include"), Some(Punct('!')), _) => {
                found.push("includes a file this test does not read".into());
            }
            (Punct('$'), Some(Ident(variable)), Some(Path))
                if *variable != "crate" && !above.modules.is_empty() =>
            {
                found.push(format!("starts a path with a macro's `${variable}`"));
            }
            (Punct('#'), _, _) => found.extend(attribute_names_above(&tokens[at..], below, above)),
            _ => {}
        }
        at += 1;
    }
    for link in docs.iter().flat_map(|doc| links(doc)) {
        if path_names_above(link, below, above) {
            found.push(format!("links to `{link}`"));
        }
    }
    found
}

/// What the attribute `tokens` start with names above, or that it is a doc this test does not
/// read.
fn attribute_names_above(tokens: &[Token], below: &[&str], above: &Above) -> Vec<String> {
    let open = if tokens.get(1) == Some(&Punct('!')) {
        2
    } else {
        1
    };
    if tokens.get(open) != Some(&Punct('[')) {
        return Vec::new();
    }
    let rest = skip_group(&tokens[open..], '[', ']');
    let attribute = &tokens[open..tokens.len() - rest.len()];
    let mut found = Vec::new();
    if let [_, Ident("doc"), Punct('='), ..] = attribute {
        found.push("writes a doc attribute this test does not read".into());
    }
    for token in attribute {
        if let Str(text) = token
            && path_names_above(text, below, above)
        {
            found.push(format!("names `{text}` in an attribute"));
        }
    }
    found
}

/// The segments of the path `tokens` start with.
fn segments<'a>(tokens: &[Token<'a>]) -> Vec<Token<'a>> {
    let (mut segments, mut tokens) = (Vec::new(), tokens.iter());
    while let Some(segment @ (Ident(_) | Punct('*'))) = tokens.next() {
        segments.push(*segment);
        if tokens.next() != Some(&Path) {
            break;
        }
    }
    segments
}

/// Each path a use declaration imports, from the tree after its `use`.
fn use_paths<'a>(tree: &[Token<'a>]) -> Vec<Vec<Token<'a>>> {
    let mut paths = Vec::new();
    flatten(tree, Vec::new(), &mut paths);
    paths
}

fn flatten<'a>(tree: &[Token<'a>], mut path: Vec<Token<'a>>, paths: &mut Vec<Vec<Token<'a>>>) {
    for (at, token) in tree.iter().enumerate() {
        match token {
            Punct('{') => {
                let group = &tree[at + 1..tree.len() - skip_group(&tree[at..], '{', '}').len()];
                let group = group.strip_suffix(&[Punct('}')]).unwrap_or(group);
                for part in split_top(group).into_iter().filter(|part| !part.is_empty()) {
                    flatten(part, path.clone(), paths);
                }
                return;
            }
            Ident("as") => break,
            Ident(_) | Punct('*') => path.push(*token),
            _ => {}
        }
    }
    paths.push(path);
}

/// `tokens` split at the commas outside their braces.
fn split_top<'t, 'a>(tokens: &'t [Token<'a>]) -> Vec<&'t [Token<'a>]> {
    let (mut parts, mut depth, mut start) = (Vec::new(), 0, 0);
    for (at, token) in tokens.iter().enumerate() {
        match token {
            Punct('{') => depth += 1,
            Punct('}') => depth -= 1,
            Punct(',') if depth == 0 => {
                parts.push(&tokens[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts
}

/// What a path at `depth` inline modules names at the crate root that it must not: a module
/// other than those of `below`, a glob, or the root itself.
fn root_named(path: &[Token], depth: usize, below: &[&str]) -> Option<String> {
    let (root, rest) = match path {
        [Ident(root @ ("crate" | "lez_authority")), rest @ ..] => (*root, rest),
        _ => {
            let rest = path.strip_prefix(&[Ident("self")]).unwrap_or(path);
            let supers = rest.iter().take_while(|s| **s == Ident("super")).count();
            // Only a `super` past the inline modules climbs out of the file's module.
            if supers <= depth {
                return None;
            }
            ("super", &rest[supers..])
        }
    };
    match rest.first() {
        Some(Ident(module)) if below.contains(module) => None,
        Some(Ident("self")) | None => Some(format!("aliases `{root}`")),
        Some(Ident(name)) => Some(format!("names `{root}::{name}`")),
        _ => Some(format!("imports `{root}::*`")),
    }
}

/// Whether a doc link or a path in an attribute names something above.
fn path_names_above(text: &str, below: &[&str], above: &Above) -> bool {
    let path = text.rsplit('@').next().unwrap_or(text);
    if path.contains("://") {
        return false;
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
    named_above || segments.iter().any(|segment| above.added.contains(segment))
}

/// The text of each bracket in a doc line, the target of each `[..](x)`, and that of a
/// `[..]: x` definition.
fn links(doc: &str) -> Vec<&str> {
    let mut links = Vec::new();
    for (at, _) in doc.match_indices('[') {
        if let Some(end) = doc[at + 1..].find(']') {
            links.push(doc[at + 1..at + 1 + end].trim_matches('`'));
        }
    }
    for (at, _) in doc.match_indices("](") {
        if let Some(end) = doc[at + 2..].find(')') {
            links.push(doc[at + 2..at + 2 + end].trim_matches(['<', '>']));
        }
    }
    let definition = doc.trim_start().strip_prefix('[');
    if let Some((_, target)) = definition.and_then(|rest| rest.split_once("]:")) {
        links.extend(target.split_whitespace().next());
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

/// Where a layer adds to the layers below: a method or const under a name one of them uses,
/// which `above` cannot tell from their own, or a trait impl for a type of theirs.
fn adds_below(defined: &[Defined], layer: usize) -> Vec<String> {
    let lower: BTreeSet<&str> = defined[..layer]
        .iter()
        .flat_map(|names| names.items.iter().chain(&names.fns))
        .copied()
        .collect();
    let mine = &defined[layer];
    let clashes = (mine.added.iter().filter(|name| lower.contains(*name)))
        .map(|name| format!("adds `{name}`, which a module below also names"));
    let impls = (mine.impls_for.iter().filter(|ty| lower.contains(*ty)))
        .map(|ty| format!("implements a trait for `{ty}`, which a module below defines"));
    clashes.chain(impls).collect()
}

fn defined_layers() -> Vec<Defined<'static>> {
    LAYERS
        .iter()
        .map(|(_, source)| defined(&lex(source).tokens))
        .collect()
}

/// The modules lib.rs declares, and what in it brings code into a layer that this test does
/// not read.
fn lib_declares(source: &str) -> (BTreeSet<&str>, Vec<String>) {
    let tokens = lex(source).tokens;
    let (mut declared, mut found) = (BTreeSet::new(), Vec::new());
    for (at, pair) in tokens.windows(2).enumerate() {
        let [Ident("mod"), Ident(name)] = pair else {
            continue;
        };
        declared.insert(*name);
        let mut before = &tokens[..at];
        if before.last() == Some(&Punct(')'))
            && let Some(open) = before.iter().rposition(|token| *token == Punct('('))
            && open > 0
            && before[open - 1] == Ident("pub")
        {
            before = &before[..open - 1];
        } else if before.last() == Some(&Ident("pub")) {
            before = &before[..before.len() - 1];
        }
        if LAYERS.iter().any(|(layer, _)| layer == name) && before.last() == Some(&Punct(']')) {
            found.push(format!("puts an attribute on `mod {name}`"));
        }
    }
    if tokens.contains(&Ident("macro_rules")) {
        found.push("defines a macro, which every module after it sees".into());
    }
    (declared, found)
}

#[test]
fn each_module_names_only_the_modules_below_it() {
    let defined = defined_layers();
    let mut found = Vec::new();
    for (layer, (name, source)) in LAYERS.iter().enumerate() {
        assert!(
            !defined[layer].items.is_empty(),
            "src/{name}.rs defines nothing"
        );
        let below: Vec<&str> = LAYERS[..=layer].iter().map(|(name, _)| *name).collect();
        let places = names_above(source, &below, &above(&defined, layer));
        for place in places.into_iter().chain(adds_below(&defined, layer)) {
            found.push(format!("src/{name}.rs {place}"));
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_engine_has_the_four_modules_and_no_other() {
    let (declared, found) = lib_declares(include_str!("../lib.rs"));
    let expected: BTreeSet<&str> = LAYERS
        .iter()
        .map(|(name, _)| *name)
        .chain(["compile_fail", "tests"])
        .collect();
    assert_eq!(declared, expected);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_check_finds_each_way_up() {
    let above = above(&defined_layers(), 1);
    let below = ["wire", "policy"];
    for source in [
        "use crate::*;",
        "use super::*;",
        "use crate::Role;",
        "use crate::hooks::Hooks;",
        "use crate::{wire::Op, hooks::Hooks};",
        "use crate as root;",
        "mod inner { use super::super::*; }",
        "mod inner { use super::{super::hooks::Hooks}; }",
        "mod inner { fn f() { super::super::hooks::own_actor(); } }",
        "mod inner { fn f() { self::super::super::Hooks::receive(); } }",
        "extern crate self as up;",
        "macro_rules! at { ($r:tt) => { $r::hooks::own_actor() } }",
        "fn f() { lez_authority::hooks::receive(); }",
        "fn f() { ::lez_authority::Hooks::receive(); }",
        "fn f(ctx: &Ctx) { ctx.own_message::<R>(KEY, Vec::new()); }",
        "fn f(ctx: &Ctx) { ctx.r#own_message(); }",
        "fn f() { Ctx::own_message::<R>; }",
        "mod hidden;",
        "include!(\"other.rs\");",
        "#[borsh(serialize_with = \"crate::hooks::own_actor\")]\nstruct S;",
        "/// Runs under [`Hooks`].\nfn f() {}",
        "/// Runs under [the hooks](crate::Hooks).\nfn f() {}",
        "/// Runs under [the hooks](<crate::hooks::Hooks>).\nfn f() {}",
        "/// Runs under [the hooks][crate::hooks::Hooks].\nfn f() {}",
        "/// Runs under [the hooks][h].\n///\n/// [h]: crate::hooks::Hooks\nfn f() {}",
        "/// Runs under [crate::hooks::Hooks].\nfn f() {}",
        "/// See [`Ctx::own_message`].\nfn f() {}",
        "/// See [`hooks`].\nfn f() {}",
        "/** Runs under [`Hooks`]. */\nfn f() {}",
        "/*! Runs under [`Hooks`]. */",
        "#[doc = \"Runs under [`Hooks`].\"]\nfn f() {}",
        "fn f<'a>(_: &'a str) { let _ = '\"'; crate::hooks::own_actor(); }",
        "fn f() { let _ = r#\"a\"b\"#; crate::hooks::own_actor(); }",
        r##"const A: &core::ffi::CStr = cr#"""#; fn f() { crate::hooks::own_actor(); }"##,
    ] {
        assert_eq!(names_above(source, &below, &above).len(), 1, "{source}");
    }
    for source in [
        "use crate::{wire::{self, Op}, policy::Ctx};",
        "use crate::{\n    wire::Op,\n    policy::Ctx,\n};",
        "use crate::wire as codec;",
        "pub(crate) fn f() { crate::wire::sent(); }",
        "mod inner { use super::*; }",
        "mod inner { fn f() { super::super::wire::sent(); } }",
        "macro_rules! at { () => { $crate::wire::sent() } }",
        "#[borsh(serialize_with = \"rest::serialize\")]\n#[doc(hidden)]\nstruct S;",
        "// crate::hooks::Hooks\n/* crate::hooks */ fn f() { \"crate::hooks\"; }",
        "/// See [`Ctx::check`] and [the guard](https://example.org/a::Hooks).\nfn f() {}",
        "/// A `[u8; 32]` key.\nfn f() {}",
        "fn f<'a>(c: &'a str) -> char { 'h' }",
    ] {
        assert_eq!(
            names_above(source, &below, &above),
            Vec::<String>::new(),
            "{source}"
        );
    }
}

#[test]
fn the_check_finds_what_a_module_adds_below() {
    let lower = "pub struct Ctx; pub enum Op {} fn gate() {}";
    let nested = "mod ext { impl Ctx { fn up(&self) {} const UP: u8 = 0; } }\n\
        const _: () = { impl crate::policy::Ctx { fn block(&self) {} } };";
    assert_eq!(
        defined(&lex(nested).tokens).added,
        BTreeSet::from(["UP", "block", "up"])
    );
    let adds = |source| {
        adds_below(
            &[defined(&lex(lower).tokens), defined(&lex(source).tokens)],
            1,
        )
    };
    for source in [
        "impl Ctx { pub(crate) fn gate(&self) {} }",
        "impl std::fmt::Display for Op {}",
        "impl<'a> From<&'a str> for &'a mut Op {}",
    ] {
        assert_eq!(adds(source).len(), 1, "{source}");
    }
    for source in [
        "impl Ctx { fn own_message(&self) {} }",
        "impl<A> Roles for (A,) {}",
        "struct Op; impl Display for Op {}",
    ] {
        assert_eq!(adds(source), Vec::<String>::new(), "{source}");
    }
}

#[test]
fn the_check_finds_what_lib_rs_hides() {
    for source in [
        "#[path = \"codec.rs\"] mod wire;",
        "#[macro_use]\npub(crate) mod guard;",
        "macro_rules! up { () => {} }\nmod wire;",
    ] {
        assert_eq!(lib_declares(source).1.len(), 1, "{source}");
    }
    let plain = "#[cfg(test)]\nmod tests;\npub mod wire;\nmod policy;";
    assert_eq!(lib_declares(plain).1, Vec::<String>::new());
}
