// Part of Agent Record Adapter (see NOTICE).
// Part of the desktop-recorder-mcp fork of agent-desktop (see NOTICE).
//! Choosing an item of a long list by its name, in the background: the list
//! finds it through UI Automation's ItemContainer even when it is out of view
//! (a virtualized list such as File Explorer's shows only the files on screen),
//! the item is made real (VirtualizedItem) and selected (SelectionItem). No
//! scrolling, no pointer, no keyboard.

use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    IUIAutomationElement, IUIAutomationItemContainerPattern, IUIAutomationSelectionItemPattern,
    IUIAutomationVirtualizedItemPattern, UIA_ItemContainerPatternId, UIA_NamePropertyId,
    UIA_SelectionItemPatternId, UIA_VirtualizedItemPatternId,
};
use windows::core::BSTR;

/// Selects the item named `name` of the list `container`. `None` when the
/// container is no item container or has no item of that name: the caller
/// goes on with its other ways.
pub(crate) fn select_by_name(container: &uiautomation::UIElement, name: &str) -> Option<()> {
    let element: &IUIAutomationElement = container.as_ref();
    unsafe {
        let items: IUIAutomationItemContainerPattern = element
            .GetCurrentPatternAs(UIA_ItemContainerPatternId)
            .ok()?;
        let wanted = VARIANT::from(BSTR::from(name));
        let item = items
            .FindItemByProperty(None, UIA_NamePropertyId, &wanted)
            .ok()?;
        if let Ok(virtualized) = item.GetCurrentPatternAs::<IUIAutomationVirtualizedItemPattern>(
            UIA_VirtualizedItemPatternId,
        ) {
            let _ = virtualized.Realize();
        }
        let selection: IUIAutomationSelectionItemPattern =
            item.GetCurrentPatternAs(UIA_SelectionItemPatternId).ok()?;
        selection.Select().ok()?;
        Some(())
    }
}
