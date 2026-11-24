// Part of Agent Record Adapter (see NOTICE).
//! UI Automation's expand/collapse states, as upstream's private
//! `actions::disclosure::ExpandKind` names them.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExpandKind {
    Collapsed,
    Expanded,
    PartiallyExpanded,
    LeafNode,
}

impl ExpandKind {
    pub(crate) fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Collapsed),
            1 => Some(Self::Expanded),
            2 => Some(Self::PartiallyExpanded),
            3 => Some(Self::LeafNode),
            _ => None,
        }
    }
}
