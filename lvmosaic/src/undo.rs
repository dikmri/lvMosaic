use crate::model::{ExportSettings, MosaicRegion};

/// アンドゥ可能な操作
#[derive(Debug, Clone)]
pub enum EditAction {
    AddMosaic(MosaicRegion),
    RemoveMosaic { index: usize, region: MosaicRegion },
    UpdateMosaic { index: usize, before: MosaicRegion, after: MosaicRegion },
    UpdateExport { before: ExportSettings, after: ExportSettings },
}

pub struct UndoStack {
    history: Vec<EditAction>,
    redo_stack: Vec<EditAction>,
    max_size: usize,
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            redo_stack: Vec::new(),
            max_size: 100,
        }
    }

    pub fn push(&mut self, action: EditAction) {
        self.redo_stack.clear();
        self.history.push(action);
        if self.history.len() > self.max_size {
            self.history.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// アンドゥして逆操作を返す
    pub fn undo(&mut self, mosaics: &mut Vec<MosaicRegion>, export: &mut ExportSettings) -> bool {
        if let Some(action) = self.history.pop() {
            let reverse = apply_action_reverse(&action, mosaics, export);
            self.redo_stack.push(reverse);
            true
        } else {
            false
        }
    }

    /// リドゥして逆操作を返す
    pub fn redo(&mut self, mosaics: &mut Vec<MosaicRegion>, export: &mut ExportSettings) -> bool {
        if let Some(action) = self.redo_stack.pop() {
            let forward = apply_action_reverse(&action, mosaics, export);
            self.history.push(forward);
            true
        } else {
            false
        }
    }
}

fn apply_action_reverse(
    action: &EditAction,
    mosaics: &mut Vec<MosaicRegion>,
    export: &mut ExportSettings,
) -> EditAction {
    match action {
        EditAction::AddMosaic(region) => {
            let idx = mosaics.iter().position(|m| m.id == region.id);
            if let Some(i) = idx {
                let removed = mosaics.remove(i);
                EditAction::RemoveMosaic { index: i, region: removed }
            } else {
                EditAction::AddMosaic(region.clone())
            }
        }
        EditAction::RemoveMosaic { index, region } => {
            let idx = (*index).min(mosaics.len());
            mosaics.insert(idx, region.clone());
            EditAction::AddMosaic(region.clone())
        }
        EditAction::UpdateMosaic { index, before, after: _ } => {
            let idx = *index;
            if idx < mosaics.len() {
                let current = mosaics[idx].clone();
                mosaics[idx] = before.clone();
                EditAction::UpdateMosaic {
                    index: idx,
                    before: current,
                    after: before.clone(),
                }
            } else {
                action.clone()
            }
        }
        EditAction::UpdateExport { before, after: _ } => {
            let current = export.clone();
            *export = before.clone();
            EditAction::UpdateExport {
                before: current,
                after: before.clone(),
            }
        }
    }
}
