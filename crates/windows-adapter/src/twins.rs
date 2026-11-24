// Part of Agent Record Adapter (see NOTICE).
//! Twins told apart by their place.
//!
//! Upstream refuses an element that shares its role and name with another
//! (an unnamed icon in each row of a table, unlabelled fields): its ref was
//! "not proven unique" and cannot be re-identified. The old fork changed
//! upstream's refs for this. Here, when upstream refuses such a ref, the
//! element is found again by its place: in the window the snapshot read, the
//! one element of the same role (and name, when it has one) at the bounds the
//! snapshot saved. If no element, or more than one, is there now (the window
//! moved, the page scrolled), the refusal stands: nothing is guessed.

use agent_desktop_core::actions::action_request::ActionRequest;
use agent_desktop_core::{
    ActionOps, ActionResult, AdapterError, Deadline, ErrorCode, LocatorField, RefEntry, SystemOps,
};
use uiautomation::types::TreeScope;

use crate::tree::properties::read_live;
use crate::tree::{UIAElement, automation, roles};

/// How far (in pixels) an element's edges may be from the saved bounds.
const TOLERANCE: f64 = 3.0;
const MAX_ELEMENTS: usize = 20_000;

/// The live element a saved ref describes, found by its place.
pub fn find_by_place(entry: &RefEntry) -> Result<UIAElement, AdapterError> {
    let refusal = |why: &str| {
        AdapterError::new(
            ErrorCode::AmbiguousTarget,
            format!("The element could not be found again by its place: {why}"),
        )
        .with_suggestion("Take a new snapshot and use the new ref.")
    };
    let saved = entry
        .geometry
        .bounds
        .ok_or_else(|| refusal("the snapshot saved no bounds"))?;
    let window = entry
        .source
        .source_window_id
        .as_deref()
        .ok_or_else(|| refusal("the snapshot saved no window"))?;
    let hwnd = crate::system::window::window_ops::parse_window_id(window)?;
    // A minimized window's elements have no place: it is shown behind the
    // person's windows (never activated) first, as a snapshot does.
    // SAFETY: IsIconic takes any window handle; a stale one answers false.
    let minimized =
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(hwnd as _) } != 0;
    if minimized {
        crate::actions::chromium_input::show_behind_if_minimized(hwnd as _);
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
    let root = automation::root_from_hwnd(hwnd, Deadline::standard()?)?;
    let root = crate::tree::uia_element(&root.into_native_handle())?;
    let client = automation::automation_client()?;
    let all = client
        .create_true_condition()
        .map_err(|error| automation::uia_error(&error, "create_true_condition"))?;
    let elements = root
        .0
        .find_all(TreeScope::Descendants, &all)
        .map_err(|error| automation::uia_error(&error, "find_all"))?;
    let mut found = Vec::new();
    for element in elements.into_iter().take(MAX_ELEMENTS) {
        let Ok(rect) = element.get_bounding_rectangle() else {
            continue;
        };
        let close = (f64::from(rect.get_left()) - saved.x).abs() <= TOLERANCE
            && (f64::from(rect.get_top()) - saved.y).abs() <= TOLERANCE
            && (f64::from(rect.get_width()) - saved.width).abs() <= TOLERANCE
            && (f64::from(rect.get_height()) - saved.height).abs() <= TOLERANCE;
        if !close {
            continue;
        }
        let element = UIAElement::from(element);
        let (properties, _) = read_live(&element);
        let role_matches = matches!(
            roles::resolve_role(&properties),
            LocatorField::Known(role) if role == entry.identity.role
        );
        let name_matches = match &entry.identity.name {
            None => true,
            Some(name) => element.0.get_name().is_ok_and(|live| live == *name),
        };
        if role_matches && name_matches {
            found.push(element);
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(refusal("no element of that role is at its place now")),
        _ => {
            // Nested twins share their bounds (a group inside a group): the
            // path the snapshot saved (child indices from the window) decides.
            let saved_path: Vec<usize> = entry.scope.path.iter().copied().collect();
            let paths: Vec<Option<Vec<usize>>> = found
                .iter()
                .map(|element| path_from(&client, &root, element))
                .collect();
            let exact: Vec<usize> = (0..found.len())
                .filter(|&i| paths[i].as_deref() == Some(saved_path.as_slice()))
                .collect();
            let by_depth: Vec<usize> = (0..found.len())
                .filter(|&i| {
                    paths[i]
                        .as_ref()
                        .is_some_and(|path| path.len() == saved_path.len())
                })
                .collect();
            match (exact.as_slice(), by_depth.as_slice()) {
                ([one], _) | ([], [one]) => Ok(found.swap_remove(*one)),
                _ => Err(refusal(
                    "more than one element of that role is at its place",
                )),
            }
        }
    }
}

/// The element's child indices from `root`, in the control view.
fn path_from(
    client: &uiautomation::UIAutomation,
    root: &UIAElement,
    element: &UIAElement,
) -> Option<Vec<usize>> {
    let walker = client.get_control_view_walker().ok()?;
    let mut path = Vec::new();
    let mut current = element.0.clone();
    for _ in 0..64 {
        if client.compare_elements(&current, &root.0).ok()? {
            path.reverse();
            return Some(path);
        }
        let parent = walker.get_parent(&current).ok()?;
        let mut index = 0;
        let mut child = walker.get_first_child(&parent).ok();
        while let Some(sibling) = child {
            if client.compare_elements(&sibling, &current).ok()? {
                break;
            }
            index += 1;
            child = walker.get_next_sibling(&sibling).ok();
        }
        path.push(index);
        current = parent;
    }
    None
}

/// Runs `request` on the element a saved ref describes, found by its place,
/// through our adapter (background routes first).
pub fn act_by_place(
    adapter: &crate::WindowsAdapter,
    entry: &RefEntry,
    request: ActionRequest,
) -> Result<ActionResult, AdapterError> {
    let element = find_by_place(entry)?;
    let lease = adapter.acquire_interaction_lease(Deadline::standard()?)?;
    let handle = element.into_native_handle();
    adapter.execute_action(&handle, request, &lease)
}
