use crate::{contracts::relative_path, digest::parse_hex, manifest::*, platform, Error, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::{Path, PathBuf},
};

pub(crate) struct HeldFile {
    pub handle: File,
    pub identity: platform::Fingerprint,
    pub digest: [u8; 32],
}

pub(crate) struct HeldTree {
    pub root_path: PathBuf,
    _ancestors: Vec<File>,
    directories: BTreeMap<String, File>,
    pub manifest: HeldFile,
    pub files: BTreeMap<String, HeldFile>,
}

impl HeldTree {
    pub fn open(root: &Path, expected_digest: &[u8; 32]) -> Result<(Self, Vec<u8>)> {
        let (ancestors, directory) = platform::open_root(root)?;
        let handle = platform::open_child(&directory, root, MANIFEST_NAME, false)?;
        let identity = platform::fingerprint(&handle)?;
        let bytes = read_bounded(&handle, MAX_MANIFEST)?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if digest != *expected_digest {
            return Err(Error::ManifestDigest);
        }
        if platform::fingerprint(&handle)? != identity {
            return Err(Error::Changed(MANIFEST_NAME.into()));
        }
        let manifest = HeldFile {
            handle,
            identity,
            digest,
        };
        Ok((
            Self {
                root_path: root.to_owned(),
                _ancestors: ancestors,
                directories: [(String::new(), directory)].into_iter().collect(),
                manifest,
                files: BTreeMap::new(),
            },
            bytes,
        ))
    }

    pub fn acquire_payload(&mut self, files: &FileSet) -> Result<()> {
        let expected: BTreeMap<_, _> = files
            .0
            .iter()
            .map(|file| (file.path.clone(), file))
            .collect();
        let directories = expected_directories(files)?;
        let mut stack = vec![String::new()];
        let mut identities =
            BTreeSet::from([(self.manifest.identity.device, self.manifest.identity.id)]);
        while let Some(prefix) = stack.pop() {
            let directory = self
                .directories
                .get(&prefix)
                .ok_or(Error::Manifest("missing directory handle"))?
                .try_clone()?;
            let parent_path = self.root_path.join(&prefix);
            for name in platform::entries(&directory, &parent_path)? {
                relative_path(&name)?;
                let relative = joined(&prefix, &name);
                if relative == MANIFEST_NAME {
                    continue;
                }
                if directories.contains(&relative) {
                    let file = platform::open_child(&directory, &parent_path, &name, true)?;
                    self.directories.insert(relative.clone(), file);
                    stack.push(relative);
                } else {
                    let spec = expected
                        .get(&relative)
                        .ok_or_else(|| Error::Changed(relative.clone()))?;
                    let handle = platform::open_child(&directory, &parent_path, &name, false)?;
                    let identity = platform::fingerprint(&handle)?;
                    if identity.length != spec.size
                        || !identities.insert((identity.device, identity.id))
                    {
                        return Err(Error::Alias(relative));
                    }
                    let digest = parse_hex(&spec.sha256)?;
                    if hash(&handle, spec.size)? != digest
                        || platform::fingerprint(&handle)? != identity
                    {
                        return Err(Error::Changed(relative));
                    }
                    self.files.insert(
                        relative,
                        HeldFile {
                            handle,
                            identity,
                            digest,
                        },
                    );
                }
            }
        }
        if self.files.len() != expected.len() || self.directories.len() != directories.len() + 1 {
            return Err(Error::Changed("missing payload file or directory".into()));
        }
        self.revalidate(files)
    }

    pub fn revalidate(&self, files: &FileSet) -> Result<()> {
        let (_ancestors, current_root) = platform::open_root(&self.root_path)?;
        let original_root = self
            .directories
            .get("")
            .ok_or(Error::Manifest("root handle missing"))?;
        same_object(original_root, &current_root, "root")?;
        let expected_dirs = expected_directories(files)?;
        let mut observed = BTreeSet::new();
        let mut observed_dirs = BTreeSet::new();
        let mut stack = vec![String::new()];
        while let Some(prefix) = stack.pop() {
            let directory = self
                .directories
                .get(&prefix)
                .ok_or(Error::Manifest("directory handle missing"))?;
            let parent_path = self.root_path.join(&prefix);
            for name in platform::entries(directory, &parent_path)? {
                relative_path(&name)?;
                let relative = joined(&prefix, &name);
                if expected_dirs.contains(&relative) {
                    let current = platform::open_child(directory, &parent_path, &name, true)?;
                    let held = self
                        .directories
                        .get(&relative)
                        .ok_or_else(|| Error::Changed(relative.clone()))?;
                    same_object(held, &current, &relative)?;
                    observed_dirs.insert(relative.clone());
                    stack.push(relative);
                } else {
                    let held = if relative == MANIFEST_NAME {
                        &self.manifest
                    } else {
                        self.files
                            .get(&relative)
                            .ok_or_else(|| Error::Changed(relative.clone()))?
                    };
                    let current = platform::open_child(directory, &parent_path, &name, false)?;
                    same_object(&held.handle, &current, &relative)?;
                    verify_held(held, &relative)?;
                    observed.insert(relative);
                }
            }
        }
        if observed.len() != self.files.len() + 1 || observed_dirs != expected_dirs {
            return Err(Error::Changed("payload file set differs".into()));
        }
        Ok(())
    }

    pub fn read_verified(&self, relative: &str, maximum: u64) -> Result<Vec<u8>> {
        relative_path(relative)?;
        let held = self
            .files
            .get(relative)
            .ok_or(Error::Manifest("file not in trusted manifest"))?;
        if maximum > 16 * 1024 * 1024 {
            return Err(Error::Manifest("resource read exceeds API bound"));
        }
        let bytes = read_bounded(&held.handle, maximum)?;
        if platform::fingerprint(&held.handle)? != held.identity
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != held.digest
        {
            return Err(Error::Changed(relative.into()));
        }
        Ok(bytes)
    }
}

fn joined(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.into()
    } else {
        format!("{prefix}/{name}")
    }
}

fn expected_directories(files: &FileSet) -> Result<BTreeSet<String>> {
    let mut result = BTreeSet::new();
    for file in &files.0 {
        let parts: Vec<_> = file.path.split('/').collect();
        for end in 1..parts.len() {
            result.insert(parts[..end].join("/"));
        }
    }
    if result.len() + files.0.len() > 4096 {
        return Err(Error::Manifest("total namespace bound exceeded"));
    }
    Ok(result)
}

fn same_object(original: &File, current: &File, name: &str) -> Result<()> {
    let left = platform::fingerprint(original)?;
    let right = platform::fingerprint(current)?;
    if left.device != right.device || left.id != right.id || left.directory != right.directory {
        return Err(Error::Changed(name.into()));
    }
    Ok(())
}

fn verify_held(held: &HeldFile, name: &str) -> Result<()> {
    if platform::fingerprint(&held.handle)? != held.identity
        || hash(&held.handle, held.identity.length)? != held.digest
    {
        return Err(Error::Changed(name.into()));
    }
    Ok(())
}

pub(crate) fn read_bounded(file: &File, maximum: u64) -> Result<Vec<u8>> {
    let length = platform::fingerprint(file)?.length;
    if length > maximum {
        return Err(Error::Manifest("resource exceeds size bound"));
    }
    let mut result = Vec::with_capacity(length as usize);
    let mut buffer = [0; 65536];
    loop {
        let n = platform::read_at(file, &mut buffer, result.len() as u64)?;
        if n == 0 {
            break;
        }
        if (result.len() + n) as u64 > maximum {
            return Err(Error::Manifest("resource grew beyond bound"));
        }
        result.extend_from_slice(&buffer[..n]);
    }
    Ok(result)
}

fn hash(file: &File, expected_length: u64) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    let mut offset = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = platform::read_at(file, &mut buffer, offset)?;
        if n == 0 {
            break;
        }
        offset += n as u64;
        if offset > expected_length {
            return Err(Error::Manifest("payload grew during verification"));
        }
        hash.update(&buffer[..n]);
    }
    if offset != expected_length {
        return Err(Error::Manifest("payload truncated during verification"));
    }
    Ok(hash.finalize().into())
}
