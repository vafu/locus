use camino::Utf8PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathError {
    #[error("path is not UTF-8")]
    NonUtf8,
    #[error("failed to canonicalize path '{path}': {source}")]
    Canonicalize {
        path: String,
        source: std::io::Error,
    },
}

pub fn canonical_project_path(path: &str) -> Result<String, PathError> {
    let path_buf = Utf8PathBuf::from(path);
    let canonical = path_buf
        .canonicalize()
        .map_err(|source| PathError::Canonicalize {
            path: path.to_string(),
            source,
        })?;
    canonical
        .to_str()
        .map(ToOwned::to_owned)
        .ok_or(PathError::NonUtf8)
}
