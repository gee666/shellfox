use serde::{Deserialize, Serialize};

pub const TEXT_BYTES: usize = 128 * 1024;
pub const PREVIEW_BYTES: usize = 2 * 1024 * 1024;
pub const LIST_LIMIT: usize = 500;
pub const LIST_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

/// Set by the trusted caller, never decoded from a file request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityKind {
    ApprovedRoot,
    /// Server-minted approved-device authority over the current OS account.
    ApprovedFullAccount,
    LocalDesktop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteText {
    pub path: String,
    pub text: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFile {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rename {
    pub path: String,
    pub destination: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteKind {
    File,
    EmptyDirectory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delete {
    pub path: String,
    pub kind: DeleteKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileList {
    pub path: String,
    pub parent_path: Option<String>,
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStat {
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextFile {
    pub path: String,
    pub text: String,
    pub size: u64,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResult {
    pub path: String,
    pub size: u64,
    pub revision: String,
}

/// Bytes are bounded and local. No URL, renderer execution or asset access is granted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Preview {
    Text(TextFile),
    Image {
        path: String,
        mime_type: String,
        bytes: Vec<u8>,
        revision: String,
    },
    Binary {
        path: String,
        size: u64,
        revision: String,
    },
}
