use std::path::{Path, PathBuf, Component};

pub fn sanitize_path(path: &str) -> PathBuf {
    Path::new(path).components().filter(|c| c != &Component::ParentDir).collect()
}