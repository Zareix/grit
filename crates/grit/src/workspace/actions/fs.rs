//! File-level repository operations: staging toggles, discard / delete
//! from the working tree, and clipboard path copies.

use std::path::{Path, PathBuf};

use gpui_kit::*;

use crate::workspace::{CopyPath, Workspace};

impl Workspace {
    /// Click on a staged file unstages it; on an unstaged file stages it.
    /// When the row's file is part of the current multi-selection (and the
    /// selection holds several files), the command applies to the whole
    /// selection.
    pub(crate) fn toggle_file(&mut self, path: PathBuf, staged: bool, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let targets: Vec<PathBuf> = if self.selected_staged == staged
            && self.selected_paths.len() > 1
            && self.selected_paths.iter().any(|p| p == &path)
        {
            self.selected_paths.clone()
        } else {
            vec![path]
        };
        for p in &targets {
            let result = if staged { repo.unstage_file(p) } else { repo.stage_file(p) };
            if let Err(e) = result {
                self.error = Some(e.to_string().into());
            }
        }
        self.refresh(cx);
    }

    pub(crate) fn stage_all(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        if let Err(e) = repo.stage_all() {
            self.error = Some(e.to_string().into());
        }
        self.refresh(cx);
    }

    pub(crate) fn unstage_all(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        if let Err(e) = repo.unstage_all() {
            self.error = Some(e.to_string().into());
        }
        self.refresh(cx);
    }

    /// Discard local changes of every path in `paths` (staged + unstaged).
    /// The multi-selection fan-out is done upfront by
    /// [`Workspace::open_discard_dialog`]; the confirm closure only forwards
    /// its resolved targets.
    pub(crate) fn discard_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        for path in &paths {
            if let Err(e) = repo.discard_file(path) {
                self.error = Some(e.to_string().into());
            }
        }
        self.refresh(cx);
    }

    /// Remove the file from the working tree; a tracked file shows up as an
    /// unstaged deletion. Only called from the confirm dialog.
    pub(crate) fn delete_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        if let Err(e) = std::fs::remove_file(repo.path.join(&path)) {
            self.error = Some(e.to_string().into());
            cx.notify();
            return;
        }
        self.refresh(cx);
    }

    /// Copy part of a file path to the clipboard (submenu choice).
    pub(crate) fn copy_file_path(&mut self, part: CopyPath, path: PathBuf, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let text = match part {
            CopyPath::Absolute => repo.path.join(&path).to_string_lossy().into_owned(),
            CopyPath::Relative => {
                // Paths from the status walk are already repo-root relative;
                // normalize so display and clipboard agree.
                Path::new(&path).components().collect::<PathBuf>().to_string_lossy().into_owned()
            }
            CopyPath::Name => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.info = Some(format!("Copied: {text}").into());
        cx.notify();
    }
}
