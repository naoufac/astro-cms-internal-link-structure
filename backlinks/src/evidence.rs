//! Append-only evidence sink: raw capture files with SHA-256 references.

use std::path::PathBuf;

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
        std::fs::create_dir_all(dir)?;
        Ok(Some(Evidence {
            dir: PathBuf::from(dir),
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
    ) -> Option<(String, String)> {
        if body.is_empty() {
            return None;
        }
        let hash = crate::detector::sha256_hex(body);
        let n = self.files.len();
        let urlhash = &crate::detector::sha256_hex(url.as_bytes())[..8];
        let fname = format!("{name}-{urlhash}-{n}.{ext}");
        let path = self.dir.join(&fname);
        std::fs::write(&path, body).ok()?;
        self.files
            .push((fname.clone(), url.to_string(), hash.clone()));
        Some((fname, hash))
    }
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
            .is_none());
    }

    #[test]
    fn none_dir_means_no_evidence() {
        let ev = Evidence::open(None).unwrap();
        assert!(ev.is_none());
    }
}
