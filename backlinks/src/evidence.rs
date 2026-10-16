//! Append-only evidence sink: raw capture files with SHA-256 references.

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// Evidence directory manager. Files live OUTSIDE any site tree (the
/// operator chooses the directory; the tool never writes inside the site).
pub struct Evidence {
    pub dir: PathBuf,
    /// (filename, url, sha256) for every saved capture.
    pub files: Vec<(String, String, String)>,
}

impl Evidence {
    pub fn open(dir: Option<&str>) -> anyhow::Result<Option<Evidence>> {
        let Some(dir) = dir else {
            return Ok(None);
        };
        let dir = checked_directory(Path::new(dir))?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)?;
        Ok(Some(Evidence {
            dir,
            files: Vec::new(),
        }))
    }

    /// Save body under `dir/<name>-<urlhash8>-<n>.<ext>`; returns
    /// (filename, sha256). Empty bodies are not saved.
    pub fn save(
        &mut self,
        name: &str,
        url: &str,
        body: &[u8],
        ext: &str,
    ) -> anyhow::Result<Option<(String, String)>> {
        if body.is_empty() {
            return Ok(None);
        }
        for component in [name, ext] {
            anyhow::ensure!(
                !component.is_empty()
                    && component
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
                "invalid evidence filename component"
            );
        }
        let hash = crate::detector::sha256_hex(body);
        let urlhash = &crate::detector::sha256_hex(url.as_bytes())[..8];
        let mut n = self.files.len();
        let (fname, mut file) = loop {
            let fname = format!("{name}-{urlhash}-{hash}-{n}.{ext}");
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(self.dir.join(&fname))
            {
                Ok(file) => break (fname, file),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "cannot save evidence in {}: {e}",
                        self.dir.display()
                    ))
                }
            }
        };
        file.write_all(body)?;
        file.sync_all()?;
        self.files
            .push((fname.clone(), url.to_string(), hash.clone()));
        Ok(Some((fname, hash)))
    }
}

fn checked_directory(path: &Path) -> anyhow::Result<PathBuf> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            std::path::Component::CurDir => {}
            _ => resolved.push(component.as_os_str()),
        }
        if resolved.exists() {
            resolved = resolved.canonicalize()?;
        }
    }
    anyhow::ensure!(
        !resolved.starts_with(&cwd),
        "evidence directory must be outside the CWD project"
    );
    for ancestor in resolved.ancestors() {
        for marker in [
            ".git",
            "Cargo.toml",
            "package.json",
            "astro.config.mjs",
            "astro.config.js",
            "astro.config.ts",
        ] {
            anyhow::ensure!(
                !ancestor.join(marker).exists(),
                "evidence directory must be outside site/repository trees: {}",
                ancestor.display()
            );
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_saves_with_hash_and_reference() {
        let dir = tempfile::tempdir().unwrap();
        let mut ev = Evidence::open(dir.path().to_str()).unwrap().unwrap();
        let (fname, hash) = ev
            .save("page", "https://a.example/x", b"<html>hi</html>", "html")
            .unwrap()
            .unwrap();
        assert!(fname.ends_with(".html"));
        assert_eq!(hash, crate::detector::sha256_hex(b"<html>hi</html>"));
        let path = dir.path().join(&fname);
        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(on_disk, b"<html>hi</html>");
        assert_eq!(ev.files.len(), 1);
    }

    #[test]
    fn empty_body_not_saved() {
        let dir = tempfile::tempdir().unwrap();
        let mut ev = Evidence::open(dir.path().to_str()).unwrap().unwrap();
        assert!(ev
            .save("page", "https://a.example/x", b"", "html")
            .unwrap()
            .is_none());
    }

    #[test]
    fn none_dir_means_no_evidence() {
        let ev = Evidence::open(None).unwrap();
        assert!(ev.is_none());
    }
}
