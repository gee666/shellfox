use super::EditorFile;
use shellfox_files::{EditorCancellation, WriteResult, WriteText};
use std::sync::Arc;

pub(crate) enum FileOperation {
    Open(EditorCancellation),
    Revalidate(EditorCancellation),
    Save(EditorCancellation),
}

impl FileOperation {
    pub(super) fn cancel(&self) {
        match self {
            Self::Open(cancel) | Self::Revalidate(cancel) | Self::Save(cancel) => cancel.cancel(),
        }
    }
}

#[derive(Clone)]
pub(super) struct SaveSnapshot {
    pub path: String,
    pub revision: String,
    pub text: Arc<str>,
}
impl SaveSnapshot {
    pub fn request(&self) -> WriteText {
        WriteText {
            path: self.path.clone(),
            revision: self.revision.clone(),
            text: self.text.to_string(),
        }
    }
    pub fn prepare_completion(
        &self,
        file: &EditorFile,
        written: WriteResult,
    ) -> Option<SaveCompletion> {
        if file.path != self.path || file.revision != self.revision || written.path != self.path {
            return None;
        }
        Some(SaveCompletion {
            revision: written.revision,
            size: written.size,
            text: self.text.clone(),
        })
    }
    #[cfg(test)]
    pub fn apply(&self, file: &mut EditorFile, saved: &mut Arc<str>, written: WriteResult) -> bool {
        let Some(mut prepared) = self.prepare_completion(file, written) else {
            return false;
        };
        prepared.commit(file, saved);
        true
    }
}
pub(super) struct SaveCompletion {
    revision: String,
    size: u64,
    text: Arc<str>,
}
impl SaveCompletion {
    /// Infallible bounded swaps only. Caller drops this retired payload AFTER the origin fence.
    pub fn commit(&mut self, file: &mut EditorFile, saved: &mut Arc<str>) {
        std::mem::swap(&mut file.revision, &mut self.revision);
        std::mem::swap(&mut file.size, &mut self.size);
        std::mem::swap(saved, &mut self.text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn async_save_updates_only_the_captured_baseline_and_never_mutates_later_edits() {
        let mut file = EditorFile {
            path: "/fixture/editor".into(),
            revision: "old".into(),
            size: 3,
        };
        let mut saved: Arc<str> = Arc::from("old");
        let snapshot = SaveSnapshot {
            path: file.path.clone(),
            revision: file.revision.clone(),
            text: Arc::from("changed\r\n雪"),
        };
        let later: Arc<str> = Arc::from("changed\r\n雪 while saving");
        let written = WriteResult {
            path: file.path.clone(),
            revision: "new".into(),
            size: snapshot.text.len() as u64,
        };
        assert!(snapshot.apply(&mut file, &mut saved, written));
        assert_eq!(&*saved, "changed\r\n雪");
        assert_ne!(saved, later);
        assert_eq!(file.revision, "new");
    }
    #[test]
    fn stale_save_completion_cannot_change_a_reloaded_or_other_document() {
        let snapshot = SaveSnapshot {
            path: "/fixture/a".into(),
            revision: "old".into(),
            text: Arc::from("saved"),
        };
        for (path, revision) in [("/fixture/a", "reloaded"), ("/fixture/b", "old")] {
            let mut file = EditorFile {
                path: path.into(),
                revision: revision.into(),
                size: 4,
            };
            let mut saved: Arc<str> = Arc::from("keep");
            let written = WriteResult {
                path: snapshot.path.clone(),
                revision: "new".into(),
                size: 5,
            };
            assert!(!snapshot.apply(&mut file, &mut saved, written));
            assert_eq!(file.path, path);
            assert_eq!(file.revision, revision);
            assert_eq!(&*saved, "keep");
        }
    }
}
