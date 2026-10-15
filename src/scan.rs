//! Scan artifactRoot for HTML pages and inventory all emitted files.

use std::path::{Path, PathBuf};

use crate::request::PageRecord;

/// Find pages: `<x>/index.html` (directory format) or `<x>.html` (file format).
pub fn find_pages(root: &Path, directory_format: bool) -> std::io::Result<Vec<PageRecord>> {
    let mut out = Vec::new();
    collect(root, root, directory_format, &mut out)?;
    out.sort_by(|a, b| a.filePath.cmp(&b.filePath));
    Ok(out)
}

/// All files under root (relative paths), excluding page HTML files.
pub fn find_asset_files(root: &Path, directory_format: bool) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    collect_all(root, root, &mut out)?;
    out.retain(|f| !is_page_file(f, directory_format));
    Ok(out)
}

fn is_page_file(rel: &str, directory_format: bool) -> bool {
    if directory_format {
        rel.ends_with("/index.html") || rel == "index.html"
    } else {
        rel.ends_with(".html")
    }
}

fn collect(
    root: &Path,
    dir: &Path,
    directory_format: bool,
    out: &mut Vec<PageRecord>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect(root, &p, directory_format, out)?;
        } else if ft.is_file() {
            let rel = pathrel(root, &p);
            if is_page_file(&rel, directory_format) {
                out.push(PageRecord {
                    urlPath: String::new(),
                    filePath: rel,
                });
            }
        }
    }
    Ok(())
}

fn collect_all(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_all(root, &p, out)?;
        } else if ft.is_file() {
            out.push(pathrel(root, &p));
        }
    }
    Ok(())
}

pub fn pathrel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Join a base-stripped URL path to the artifact root safely. `decoded`
/// must already have the configured base removed. Component-wise check so
/// `/docs` never matches `/docstring` and `..` never escapes the root.
pub fn url_path_to_fs(root: &Path, decoded: &str, directory_format: bool) -> Option<PathBuf> {
    let t = decoded.strip_prefix('/')?;
    if t.split('/').any(|seg| seg == "..") {
        return None;
    }
    if t.is_empty() {
        return Some(root.join("index.html"));
    }
    if directory_format {
        Some(root.join(t).join("index.html"))
    } else {
        Some(root.join(format!("{t}.html")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_to_fs_segment_boundaries() {
        let root = Path::new("/site");
        assert_eq!(
            url_path_to_fs(root, "/docs/", true),
            Some(root.join("docs/index.html"))
        );
        assert_eq!(
            url_path_to_fs(root, "/docstring/", true),
            Some(root.join("docstring/index.html"))
        );
    }

    #[test]
    fn url_to_fs_blocks_traversal() {
        let root = Path::new("/site");
        assert_eq!(url_path_to_fs(root, "/a/../../etc/passwd/", true), None);
        assert_eq!(
            url_path_to_fs(root, "/a/b/", true),
            Some(root.join("a/b/index.html"))
        );
    }

    #[test]
    fn url_to_fs_file_format_root() {
        let root = Path::new("/site");
        assert_eq!(
            url_path_to_fs(root, "/", false),
            Some(root.join("index.html"))
        );
        assert_eq!(
            url_path_to_fs(root, "/about", false),
            Some(root.join("about.html"))
        );
    }
}
