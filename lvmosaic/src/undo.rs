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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExportSettings, MosaicKeyframe, MosaicRegion};

    fn make_region(id: &str) -> MosaicRegion {
        MosaicRegion::new(id.to_string(), "N".to_string(), 0, 100,
            MosaicKeyframe { frame: 0, center_x: 0.0, center_y: 0.0,
                             width: 10.0, height: 10.0, rotation_deg: 0.0 })
    }

    // T-09: AddMosaic → Undo → Redo
    #[test]
    fn test_undo_redo_add() {
        let mut stack = UndoStack::new();
        let mut mosaics = vec![];
        let mut export = ExportSettings::default();

        mosaics.push(make_region("m1"));
        stack.push(EditAction::AddMosaic(make_region("m1")));

        assert!(stack.can_undo());
        assert!(!stack.can_redo());

        stack.undo(&mut mosaics, &mut export);
        assert_eq!(mosaics.len(), 0);
        assert!(stack.can_redo());

        stack.redo(&mut mosaics, &mut export);
        assert_eq!(mosaics.len(), 1);
    }

    // T-10: RemoveMosaic → Undo
    #[test]
    fn test_undo_remove() {
        let mut stack = UndoStack::new();
        let mut mosaics = vec![make_region("m1"), make_region("m2")];
        let mut export = ExportSettings::default();

        let removed = mosaics.remove(0);
        stack.push(EditAction::RemoveMosaic { index: 0, region: removed });
        assert_eq!(mosaics.len(), 1);

        stack.undo(&mut mosaics, &mut export);
        assert_eq!(mosaics.len(), 2);
        assert_eq!(mosaics[0].id, "m1");
    }

    // T-11: 新 push がリドゥスタックをクリアする
    #[test]
    fn test_push_clears_redo() {
        let mut stack = UndoStack::new();
        let mut mosaics = vec![make_region("m1")];
        let mut export = ExportSettings::default();

        stack.push(EditAction::AddMosaic(make_region("m1")));
        stack.undo(&mut mosaics, &mut export);
        assert!(stack.can_redo());

        mosaics.push(make_region("m2"));
        stack.push(EditAction::AddMosaic(make_region("m2")));
        assert!(!stack.can_redo(), "redo stack should be cleared after new push");
    }

    #[test]
    fn test_empty_stack() {
        let mut stack = UndoStack::new();
        let mut mosaics = vec![];
        let mut export = ExportSettings::default();
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert!(!stack.undo(&mut mosaics, &mut export));
        assert!(!stack.redo(&mut mosaics, &mut export));
    }
}
