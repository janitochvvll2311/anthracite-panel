//! Sandboxed file service for the demo build.
//!
//! It reuses the real file engine against a directory owned by the demo, so
//! path confinement, archives and trash exercise the same code. Writes,
//! uploads and deletions never leave that directory.

use std::fs;
use std::path::{Path, PathBuf};

use anth_wire::{
    ArchiveFormat, ArchiveListing, FileChunk, FileEntry, FileWritten, Page, PageRequest,
    TrashListing, WireError,
};

use crate::backend::FilesService;
use crate::files::{DownloadChunk, SystemFiles, TrashPaths};

pub struct DemoFiles {
    inner: SystemFiles,
    root: PathBuf,
}

impl DemoFiles {
    pub fn new(
        root: impl AsRef<Path>,
        max_entries: usize,
        max_preview_bytes: u64,
        max_upload_bytes: u64,
        max_trash_bytes: u64,
        max_archive_bytes: u64,
    ) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)
            .map_err(|error| format!("creating demo files at {}: {error}", root.display()))?;
        seed(&root)?;
        let paths = TrashPaths {
            topdirs: vec![root.clone()],
            home: root.join(".home"),
            data_home: root.join(".data"),
        };
        let inner = SystemFiles::new(
            &root,
            true,
            max_entries,
            max_preview_bytes,
            max_upload_bytes,
        )?
        .with_trash(true, max_trash_bytes)
        .with_archive_limit(max_archive_bytes)
        .with_trash_paths(paths);
        Ok(Self { inner, root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl FilesService for DemoFiles {
    fn list(&self, path: &str, request: &PageRequest) -> Result<Page<FileEntry>, WireError> {
        self.inner.list(path, request)
    }
    fn read(
        &self,
        path: &str,
        offset: u64,
        max_bytes: Option<u64>,
    ) -> Result<FileChunk, WireError> {
        self.inner.read(path, offset, max_bytes)
    }
    fn delete(&self, path: &str) -> Result<String, WireError> {
        self.inner.delete(path)
    }
    fn write(&self, path: &str, content: &str, create: bool) -> Result<FileWritten, WireError> {
        self.inner.write(path, content, create)
    }
    fn write_bytes(
        &self,
        path: &str,
        content: &[u8],
        create: bool,
    ) -> Result<FileWritten, WireError> {
        self.inner.write_bytes(path, content, create)
    }
    fn trash(&self, path: &str) -> Result<(String, String), WireError> {
        self.inner.trash(path)
    }
    fn trash_listing(&self) -> Result<TrashListing, WireError> {
        self.inner.trash_listing()
    }
    fn restore(&self, id: &str, overwrite: bool) -> Result<(String, String), WireError> {
        self.inner.restore(id, overwrite)
    }
    fn empty_trash(&self) -> Result<usize, WireError> {
        self.inner.empty_trash()
    }
    fn create_archive(
        &self,
        path: &str,
        destination: Option<&str>,
        format: Option<ArchiveFormat>,
    ) -> Result<String, WireError> {
        self.inner.create_archive(path, destination, format)
    }
    fn list_archive(&self, path: &str) -> Result<ArchiveListing, WireError> {
        self.inner.list_archive(path)
    }
    fn extract_archive(
        &self,
        path: &str,
        into: Option<&str>,
    ) -> Result<(String, Vec<String>), WireError> {
        self.inner.extract_archive(path, into)
    }
    fn image(&self, path: &str) -> Result<(Vec<u8>, &'static str), WireError> {
        self.inner.image(path)
    }
    fn download(&self, path: &str) -> Result<Vec<u8>, WireError> {
        self.inner.download(path)
    }
    fn download_chunk(
        &self,
        path: &str,
        offset: u64,
        max_bytes: Option<u64>,
    ) -> Result<DownloadChunk, WireError> {
        self.inner.download_chunk(path, offset, max_bytes)
    }
}

fn seed(root: &Path) -> Result<(), String> {
    let files: &[(&str, &str)] = &[
        (
            "README.md",
            "# Demo files\n\nThis tree is a sandbox. Writes and deletions stay here; nothing touches the host filesystem.\n",
        ),
        (
            "docs/welcome.txt",
            "Welcome to the Anthracite Panel demo.\nTry the editor, the trash and an archive.\n",
        ),
        (
            "logs/demo.log",
            "2026-01-01T00:00:00Z demo started\n2026-01-01T00:00:01Z network inventory ready\n2026-01-01T00:00:02Z firewall table ready\n",
        ),
        (
            "data/hosts.csv",
            "name,address,role\nedge-1,192.0.2.10,gateway\nedge-2,198.51.100.20,worker\n",
        ),
    ];
    for (path, content) in files {
        let target = root.join(path);
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("creating {}: {error}", parent.display()))?;
        }
        fs::write(&target, content)
            .map_err(|error| format!("writing {}: {error}", target.display()))?;
    }
    Ok(())
}
