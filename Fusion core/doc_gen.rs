//! `fusion doc`: generates Markdown or HTML reference documentation from doc comments.
//!
//! Doc comments are lines starting with `///` (or `##`) directly above a declaration, and
//! `//!` lines at the top of a file document the module. Markdown inside comments is kept as is
//! (Markdown output) or rendered with a small converter (HTML output).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    Markdown,
    Html,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub docs: String,
    pub public: bool,
}

fn doc_text(line: &str) -> Option<String> {
    let t = line.trim_start();
    let rest = t.strip_prefix("///").or_else(|| t.strip_prefix("##"))?;
    if t.starts_with("////") {
        return None;
    }
    Some(rest.strip_prefix(' ').unwrap_or(rest).trim_end().to_string())
}

fn ident(s: &str) -> String {
    s.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()
}

/// Extracts module-level docs and documented/public items from source text.
pub fn extract(source: &str) -> (String, Vec<Item>) {
    let mut module_docs: Vec<String> = Vec::new();
    let mut items = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut container: Option<String> = None;
    for raw in source.lines() {
        let trimmed = raw.trim();
        let indented = raw.starts_with(' ') || raw.starts_with('\t');
        if let Some(rest) = trimmed.strip_prefix("//!") {
            module_docs.push(rest.strip_prefix(' ').unwrap_or(rest).trim_end().to_string());
            continue;
        }
        if let Some(text) = doc_text(raw) {
            pending.push(text);
            continue;
        }
        if trimmed.is_empty() {
            pending.clear();
            continue;
        }
        if trimmed.starts_with("#[") || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }
        let (public, body) = match trimmed.strip_prefix("pub ") {
            Some(r) => (true, r.trim_start()),
            None => (false, trimmed),
        };
        let body = body.strip_prefix("async ").map(|r| format!("async {}", r)).unwrap_or_else(|| body.to_string());
        let bare = body.strip_prefix("async ").unwrap_or(&body).to_string();
        let kind = ["fn", "struct", "enum", "trait", "impl", "const"].iter().find(|k| bare.starts_with(&format!("{} ", k)));
        if !indented {
            container = None;
        }
        if let Some(kind) = kind {
            let after = bare[kind.len()..].trim_start();
            let mut name = if *kind == "impl" { after.trim_end_matches(|c: char| c == ':' || c == '{').trim().to_string() } else { ident(after) };
            if (*kind == "impl" || *kind == "trait") && !indented {
                container = Some(ident(after.trim_start_matches(|c: char| !c.is_alphanumeric())));
            } else if *kind == "fn" && indented {
                if let Some(c) = &container {
                    name = format!("{}.{}", c, name);
                }
            }
            let signature = trimmed.trim_end_matches(|c: char| c == ':' || c == '{').trim().to_string();
            let documented = !pending.is_empty();
            if documented || public {
                items.push(Item {
                    kind: kind.to_string(),
                    name,
                    signature: format!("{}{}", if public && !signature.starts_with("pub ") { "pub " } else { "" }, signature),
                    docs: pending.join("\n"),
                    public,
                });
            }
        }
        pending.clear();
    }
    (module_docs.join("\n"), items)
}

fn section_title(kind: &str) -> &'static str {
    match kind {
        "struct" => "Structs",
        "enum" => "Enums",
        "trait" => "Traits",
        "impl" => "Implementations",
        "fn" => "Functions",
        _ => "Constants",
    }
}

pub fn render_markdown(title: &str, module_docs: &str, items: &[Item]) -> String {
    let mut out = format!("# {}\n\n", title);
    if !module_docs.trim().is_empty() {
        out.push_str(module_docs.trim());
        out.push_str("\n\n");
    }
    for kind in ["struct", "enum", "trait", "impl", "fn", "const"] {
        let group: Vec<&Item> = items.iter().filter(|i| i.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        out.push_str(&format!("## {}\n\n", section_title(kind)));
        for item in group {
            out.push_str(&format!("### `{}`\n\n```fusion\n{}\n```\n\n", item.name, item.signature));
            if !item.docs.trim().is_empty() {
                out.push_str(item.docs.trim());
                out.push_str("\n\n");
            }
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn inline(s: &str) -> String {
    let escaped = escape(s);
    let mut out = String::new();
    for (i, part) in escaped.split('`').enumerate() {
        if i % 2 == 1 {
            out.push_str(&format!("<code>{}</code>", part));
        } else {
            out.push_str(part);
        }
    }
    out
}

/// Converts the small Markdown subset used in doc comments to HTML.
pub fn markdown_to_html(md: &str) -> String {
    let mut out = String::new();
    let (mut in_code, mut in_list, mut paragraph) = (false, false, Vec::<String>::new());
    let flush = |out: &mut String, paragraph: &mut Vec<String>| {
        if !paragraph.is_empty() {
            out.push_str(&format!("<p>{}</p>\n", paragraph.join(" ")));
            paragraph.clear();
        }
    };
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            flush(&mut out, &mut paragraph);
            if in_list {
                out.push_str("</ul>\n");
                in_list = false;
            }
            out.push_str(if in_code { "</code></pre>\n" } else { "<pre><code>" });
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push_str(&escape(line));
            out.push('\n');
            continue;
        }
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            flush(&mut out, &mut paragraph);
            if !in_list {
                out.push_str("<ul>\n");
                in_list = true;
            }
            out.push_str(&format!("<li>{}</li>\n", inline(item)));
            continue;
        }
        if in_list {
            out.push_str("</ul>\n");
            in_list = false;
        }
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if (1..=4).contains(&hashes) && line[hashes..].starts_with(' ') {
            flush(&mut out, &mut paragraph);
            out.push_str(&format!("<h{0}>{1}</h{0}>\n", hashes + 2, inline(line[hashes..].trim())));
        } else if line.trim().is_empty() {
            flush(&mut out, &mut paragraph);
        } else {
            paragraph.push(inline(line.trim()));
        }
    }
    flush(&mut out, &mut paragraph);
    if in_list {
        out.push_str("</ul>\n");
    }
    if in_code {
        out.push_str("</code></pre>\n");
    }
    out
}

pub fn render_html(title: &str, module_docs: &str, items: &[Item]) -> String {
    let mut body = format!("<h1>{}</h1>\n{}", escape(title), markdown_to_html(module_docs));
    for kind in ["struct", "enum", "trait", "impl", "fn", "const"] {
        let group: Vec<&Item> = items.iter().filter(|i| i.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        body.push_str(&format!("<h2>{}</h2>\n", section_title(kind)));
        for item in group {
            body.push_str(&format!("<h3 id=\"{0}\"><code>{0}</code></h3>\n<pre><code>{1}</code></pre>\n{2}", escape(&item.name), escape(&item.signature), markdown_to_html(&item.docs)));
        }
    }
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>{}</title>\n<style>body{{font-family:system-ui,sans-serif;max-width:52rem;margin:2rem auto;padding:0 1rem;line-height:1.5}}pre{{background:#f4f4f4;padding:.75rem;overflow-x:auto}}code{{font-family:ui-monospace,monospace}}@media(prefers-color-scheme:dark){{body{{background:#161616;color:#ddd}}pre{{background:#262626}}}}</style></head>\n<body>\n{}</body></html>\n",
        escape(title),
        body
    )
}

fn collect_sources(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path).map_err(|e| format!("{}: {}", path.display(), e))?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let hidden = p.file_name().map_or(false, |n| n.to_string_lossy().starts_with('.') || n == "target");
            if !hidden {
                collect_sources(&p, out)?;
            }
        }
    } else if path.extension().map_or(false, |e| e == "fusion") {
        out.push(path.to_path_buf());
    }
    Ok(())
}

/// Documents a `.fusion` file or every `.fusion` file below a directory.
pub fn generate(path: &str, format: Format) -> Result<String, String> {
    let mut files = Vec::new();
    collect_sources(Path::new(path), &mut files)?;
    if files.is_empty() {
        return Err(format!("no .fusion files found at '{}'", path));
    }
    let mut md = String::new();
    let mut html_body = String::new();
    let title = Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Fusion".to_string());
    for file in &files {
        let source = std::fs::read_to_string(file).map_err(|e| format!("{}: {}", file.display(), e))?;
        let (module_docs, items) = extract(&source);
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match format {
            Format::Markdown => md.push_str(&render_markdown(&name, &module_docs, &items)),
            Format::Html => html_body.push_str(&render_html(&name, &module_docs, &items)),
        }
    }
    Ok(match format {
        Format::Markdown => md,
        Format::Html => {
            if files.len() == 1 {
                html_body
            } else {
                format!("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title></head><body>\n{}</body></html>\n", escape(&title), html_body)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "//! Geometry helpers.\n\n/// A point in the plane.\n///\n/// Coordinates are `num`.\n#[derive(Debug)]\npub struct Point:\n    x: num\n    y: num\n\n/// Adds two numbers.\n/// ```\n/// add(1, 2)\n/// ```\npub fn add(a: num, b: num) -> num:\n    return a + b\n\nfn private_helper():\n    x = 1\n\n/// Behaviour of points.\nimpl Point:\n    /// Squared length.\n    fn len2(self) -> num:\n        return 1\n";

    #[test]
    fn extracts_documented_and_public_items() {
        let (module, items) = extract(SRC);
        assert_eq!(module, "Geometry helpers.");
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Point", "add", "Point", "Point.len2"]);
        assert!(items[0].docs.contains("Coordinates are `num`."));
        assert!(items[1].signature.starts_with("pub fn add"));
        assert!(!items.iter().any(|i| i.name == "private_helper"));
    }

    #[test]
    fn renders_markdown_and_html() {
        let (module, items) = extract(SRC);
        let md = render_markdown("geometry.fusion", &module, &items);
        assert!(md.contains("## Functions"));
        assert!(md.contains("### `add`"));
        let html = render_html("geometry.fusion", &module, &items);
        assert!(html.contains("<h3 id=\"add\">"));
        assert!(html.contains("<code>num</code>"));
        assert!(html.contains("<pre><code>add(1, 2)"));
    }
}
