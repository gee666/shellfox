//! Retained native directory topology, bound to its original capability.
#[path = "directory_plan_create.rs"]
pub(crate) mod create;
#[cfg(windows)]
#[path = "windows_atomic_creation_anchor.rs"]
mod windows_atomic_creation_anchor;
use crate::{
    directory_walk::{self, DirectoryWalk},
    paths, FileError, FileService, Location, Result,
};
use cap_std::{ambient_authority, fs::Dir};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};
const MAX_COMPONENTS: usize = 256;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryRelation {
    Same,
    Ancestor,
    Descendant,
    Disjoint,
    Unproven,
}
/// Opaque native handles/identities, never a wire DTO or a write-access upgrade.
pub struct DirectoryPlan {
    authority: Arc<FileService>,
    requested: Location,
    configured: PathBuf,
    original: DirectoryWalk,
}
fn inspect(path: &Path) -> Result<DirectoryWalk> {
    let (root, relative) = paths::absolute_parts(path)?;
    directory_walk::walk(
        Dir::open_ambient_dir(root, ambient_authority())?,
        &relative,
        true,
        true,
        MAX_COMPONENTS,
    )
}
impl FileService {
    pub fn plan_directory(self: &Arc<Self>, target: &Location) -> Result<DirectoryPlan> {
        self.require_path_resolution()?;
        let (_, _, configured) = self.base(&target.path)?;
        let original = inspect(&configured)?;
        self.authorized(false)?;
        Ok(DirectoryPlan {
            authority: self.clone(),
            requested: target.clone(),
            configured,
            original,
        })
    }
}
impl DirectoryPlan {
    pub fn configured_path(&self) -> &Path {
        &self.configured
    }
    fn current(&self) -> Result<DirectoryWalk> {
        let (_, _, absolute) = self.authority.base(&self.requested.path)?;
        if absolute != self.configured {
            return Err(FileError::conflict());
        }
        let current = inspect(&absolute)?;
        if current.identities.len() < self.original.identities.len()
            || !current
                .identities
                .iter()
                .zip(&self.original.identities)
                .all(|(a, b)| a == b)
        {
            return Err(FileError::conflict());
        }
        let original_names: Vec<_> = self
            .original
            .names
            .iter()
            .chain(&self.original.missing)
            .collect();
        let current_names: Vec<_> = current.names.iter().chain(&current.missing).collect();
        if original_names != current_names {
            return Err(FileError::conflict());
        }
        let ledger = self
            .authority
            .plan_creations
            .lock()
            .map_err(|_| FileError::conflict())?;
        for index in self.original.identities.len()..current.identities.len() {
            if !ledger.proves(
                &current.identities[index - 1],
                &current.names[index - 1],
                &current.identities[index],
            ) {
                return Err(FileError::conflict());
            }
        }
        drop(ledger);
        self.authority.authorized(false)?;
        Ok(current)
    }
    pub fn revalidate(&self) -> Result<()> {
        self.current().map(|_| ())
    }
    pub fn relation_to(&self, other: &Self) -> Result<DirectoryRelation> {
        let a = self.current()?;
        let b = other.current()?;
        Ok(relation(&a, &b))
    }
}
fn same(a: &DirectoryWalk, ai: usize, b: &DirectoryWalk, bi: usize) -> bool {
    a.identities[ai] == b.identities[bi]
}
fn relation(a: &DirectoryWalk, b: &DirectoryWalk) -> DirectoryRelation {
    let (alast, blast) = (a.identities.len() - 1, b.identities.len() - 1);
    if a.missing.is_empty() {
        if let Some(index) = b
            .identities
            .iter()
            .position(|info| a.identities[alast] == *info)
        {
            return if index == blast && b.missing.is_empty() {
                DirectoryRelation::Same
            } else {
                DirectoryRelation::Ancestor
            };
        }
    }
    if b.missing.is_empty() && a.identities.iter().any(|info| b.identities[blast] == *info) {
        return DirectoryRelation::Descendant;
    }
    if a.missing.is_empty() || b.missing.is_empty() {
        return DirectoryRelation::Disjoint;
    }
    let a_in_b = b
        .identities
        .iter()
        .position(|info| a.identities[alast] == *info);
    let b_in_a = a
        .identities
        .iter()
        .position(|info| b.identities[blast] == *info);
    let (ai, bi) = match (a_in_b, b_in_a) {
        (Some(bi), _) => (alast, bi),
        (_, Some(ai)) => (ai, blast),
        _ => return DirectoryRelation::Disjoint, // distinct retained existing subtrees/volumes
    };
    if !same(a, ai, b, bi) {
        return DirectoryRelation::Unproven;
    }
    let mut ap: Vec<OsString> = a.names[ai..].to_vec();
    ap.extend(a.missing.iter().cloned());
    let mut bp: Vec<OsString> = b.names[bi..].to_vec();
    bp.extend(b.missing.iter().cloned());
    for (left, right) in ap.iter().zip(&bp) {
        if left == right {
            continue;
        }
        return match crate::directory_names::distinct(&a.dirs[ai], left, right) {
            Some(true) => DirectoryRelation::Disjoint,
            _ => DirectoryRelation::Unproven,
        };
    }
    match ap.len().cmp(&bp.len()) {
        std::cmp::Ordering::Equal => DirectoryRelation::Same,
        std::cmp::Ordering::Less => DirectoryRelation::Ancestor,
        std::cmp::Ordering::Greater => DirectoryRelation::Descendant,
    }
}
