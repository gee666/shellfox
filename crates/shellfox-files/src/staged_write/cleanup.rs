use super::*;
impl Drop for StagedWrite {
    fn drop(&mut self) {
        // After a successful rename the stage name is the target; never delete it.
        if self.publication_entered {
            return;
        }
        // Remove the stage only while its name still opens the file this write created.
        if let Ok(current) = content::open_regular(&self.target.parent, &self.name) {
            if content::same_held_file(&current, &self.file).unwrap_or(false) {
                let _ = self.target.parent.remove_file(&self.name);
            }
        }
    }
}
