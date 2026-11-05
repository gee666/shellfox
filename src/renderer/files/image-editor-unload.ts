import { hasDirtyImageEditorDrafts, subscribeImageEditorDrafts } from '../../shared/image-editor-drafts';

let guardedWindow: Window | null = null;
const preventUnload = (event: BeforeUnloadEvent) => {
  event.preventDefault();
  event.returnValue = '';
};
function updateUnloadGuard(): void {
  if (hasDirtyImageEditorDrafts() && !guardedWindow && typeof window !== 'undefined') {
    guardedWindow = window;
    guardedWindow.addEventListener('beforeunload', preventUnload);
  } else if (!hasDirtyImageEditorDrafts() && guardedWindow) {
    guardedWindow.removeEventListener('beforeunload', preventUnload);
    guardedWindow = null;
  }
}
// The cache outlives mounted editors, so its unload guard must do the same.
subscribeImageEditorDrafts(updateUnloadGuard);
updateUnloadGuard();
