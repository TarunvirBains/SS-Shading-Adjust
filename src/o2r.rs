//! Reading and writing `.o2r` archives (zip files of resources). Sources are only read.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use anyhow::{Context, Result};

/// A read-only archive.
pub struct Archive {
    zip: zip::ZipArchive<File>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        Ok(Self {
            zip: zip::ZipArchive::new(file)
                .with_context(|| format!("reading {}", path.display()))?,
        })
    }

    /// Every entry name (files only), sorted.
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .zip
            .file_names()
            .filter(|n| !n.ends_with('/'))
            .map(String::from)
            .collect();
        v.sort();
        v
    }

    pub fn contains(&self, name: &str) -> bool {
        self.zip.index_for_name(name).is_some()
    }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.zip
            .by_name(name)
            .with_context(|| format!("no entry {name}"))?
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// Writes an archive of stored (uncompressed) entries, like the game's own.
pub fn write(path: &Path, entries: &[(String, Vec<u8>)]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("o2r.partial");
    let file = File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(name.as_str(), options)?;
        zip.write_all(data)?;
    }
    zip.finish()?.flush()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
