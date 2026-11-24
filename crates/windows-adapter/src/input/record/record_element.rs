// Part of Agent Record Adapter (see NOTICE).
//! Reads what a recorded action targeted: the element under the pointer or
//! holding focus, its window, its nearest identifiable ancestors, and how
//! many elements in that window share its identity. A `match_count` above 1
//! tells the consumer the recorded identity alone will be ambiguous on
//! replay and needs the ancestors to disambiguate.

use agent_desktop_core::{
    AdapterError, LocatorField, RecordRequest, Rect, WindowInfo, app_name_matches,
};
use serde_json::{Map, Value, json};
use uiautomation::patterns::{UISelectionItemPattern, UITogglePattern};
use uiautomation::types::{Handle, Point, ToggleState, TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::{UIAutomation, UITreeWalker};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::System::Console::GetConsoleWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, WindowFromPoint};

use std::cell::RefCell;
use std::collections::HashMap;

use super::record_browser::{
    CHROMIUM_RETRIES, CHROMIUM_RETRY_GAP, address, is_bare_chromium, is_browser, is_browser_frame,
    page_of,
};
use super::record_label::{is_field_role, nearby_label};
use super::record_promote::{
    PROMOTE_STEPS, WRAPPED_INPUTS, is_actionable, is_decoration, is_web_block,
};
use super::record_window::{
    WindowBook, contains, display_bounds, root_of_handle, window_center, window_id_of,
};
use crate::tree::automation::automation_client;
use crate::tree::element::UIAElement;
use crate::tree::properties::property_ids::TreeProperty;
use crate::tree::properties::{
    ElementProperties, PropertyOutcome, PropertyValue, read_live, read_one,
};
use crate::tree::roles::resolve_role;

const MAX_ANCESTORS: usize = 6;
const MAX_PARENT_STEPS: usize = 16;

/// An action's target. Either half can be missing: a point over a surface
/// with no UI Automation provider still has a window, and an element whose
/// window is not listed still has an identity.
pub(super) struct Target {
    pub(super) element: Option<UIAElement>,
    pub(super) window: Option<WindowInfo>,
    owner_id: Option<String>,
    root: isize,
    spot: Option<(i32, i32)>,
}

/// A text field's content when a typing run ends.
pub(super) enum FieldValue {
    Secret,
    Text(String),
    Unavailable,
}

pub(super) struct Observer {
    client: UIAutomation,
    walker: Option<UITreeWalker>,
    windows: WindowBook,
    own_pid: u32,
    /// A process whose windows are never recorded either: the app that
    /// started the recording (`AGENT_RECORD_IGNORE_PID`), such as
    /// Desktop Recorder's bar.
    ignored_pid: Option<u32>,
    own_console: Option<String>,
    app: Option<String>,
    window_id: Option<String>,
    display: Option<Rect>,
    /// A browser window's address by window and title (a new page, a new title).
    addresses: RefCell<HashMap<(isize, String), Option<String>>>,
}

impl Observer {
    pub(super) fn new(request: &RecordRequest) -> Result<Self, AdapterError> {
        let client = automation_client()?;
        let walker = client.get_control_view_walker().ok();
        let console = root_of_handle(unsafe { GetConsoleWindow() } as isize);
        let display = match &request.display {
            Some(id) => Some(display_bounds(id)?),
            None => None,
        };
        Ok(Self {
            client,
            walker,
            windows: WindowBook::default(),
            own_pid: std::process::id(),
            ignored_pid: std::env::var("AGENT_RECORD_IGNORE_PID")
                .ok()
                .and_then(|pid| pid.trim().parse().ok()),
            own_console: (console != 0).then(|| window_id_of(console)),
            app: request.app.clone(),
            window_id: request.window_id.clone(),
            display,
            addresses: RefCell::default(),
        })
    }

    pub(super) fn at_point(&mut self, x: i32, y: i32) -> Target {
        // A press on a window never recorded (the recorder's own, or the app
        // that started it, such as Desktop Recorder's bar) is not read through
        // UI Automation at all: reading it while that app handles the press
        // made the app lose the click (its Stop did nothing).
        let under = unsafe { WindowFromPoint(POINT { x, y }) } as isize;
        let mut pid = 0u32;
        // SAFETY: any window handle is accepted; a stale one gives pid 0.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                under as _, &mut pid,
            )
        };
        if pid != 0 && (pid == self.own_pid || Some(pid) == self.ignored_pid) {
            return self.target(None, under, Some((x, y)));
        }
        let mut element = self.hit(x, y);
        for _ in 0..CHROMIUM_RETRIES {
            if !element.as_ref().is_some_and(|hit| self.bare_chromium(hit)) {
                break;
            }
            std::thread::sleep(CHROMIUM_RETRY_GAP);
            element = self.hit(x, y);
        }
        let handle = unsafe { WindowFromPoint(POINT { x, y }) } as isize;
        self.target(element, handle, Some((x, y)))
    }

    fn hit(&self, x: i32, y: i32) -> Option<UIAElement> {
        let found = self.client.element_from_point(Point::new(x, y)).ok()?;
        Some(self.actionable(UIAElement::from(found)))
    }

    fn bare_chromium(&self, element: &UIAElement) -> bool {
        let described = element_json(&read_live(element).0);
        let class = described.get("class_name").and_then(Value::as_str);
        is_bare_chromium(class, described.get("name").is_some())
    }

    /// The control a press belongs to: the hit element, or, when that is an
    /// unnamed icon or caption drawn inside a control, the nearest actionable
    /// ancestor a few levels up (see `record_promote`).
    fn actionable(&self, element: UIAElement) -> UIAElement {
        let described = element_json(&read_live(&element).0);
        let role = described.get("role").and_then(Value::as_str);
        let named = described.get("name").is_some();
        let Some(walker) = self.walker.as_ref() else {
            return element;
        };
        if is_web_block(role, named, in_page(walker, &element.0)) {
            return web_control(walker, &element).unwrap_or(element);
        }
        if !is_decoration(role, named) {
            return element;
        }
        let mut current = element.0.clone();
        for _ in 0..PROMOTE_STEPS {
            let Ok(parent) = walker.get_parent(&current) else {
                break;
            };
            let parent_json = element_json(&read_live(&UIAElement::from(parent.clone())).0);
            let parent_role = parent_json.get("role").and_then(Value::as_str);
            if parent_role == Some("window") {
                break;
            }
            if is_actionable(parent_role) {
                return UIAElement::from(parent);
            }
            current = parent;
        }
        element
    }

    /// The target of a click agent-desktop sent to a point of window `top`
    /// (`click-at`): the element under the point when it belongs to that
    /// window, else the window alone. The window may be covered by another,
    /// whose element is then under the point instead.
    pub(super) fn at_point_in(&mut self, top: isize, x: i32, y: i32) -> Target {
        let hit = self.at_point(x, y);
        if hit.root == root_of_handle(top) {
            return hit;
        }
        match self.hit_by_bounds(top, x, y) {
            Some(element) => {
                let element = self.actionable(element);
                self.target(Some(element), top, Some((x, y)))
            }
            None => self.of_window(top, (x, y)),
        }
    }

    /// The deepest element of window `top` whose bounds hold the point, for a
    /// window covered by another one (a screen hit test answers with the one
    /// on top). Each level keeps the last child that holds it: a later
    /// sibling is drawn over an earlier one.
    fn hit_by_bounds(&self, top: isize, x: i32, y: i32) -> Option<UIAElement> {
        const MAX_DEPTH: usize = 48;
        const MAX_CHILDREN: usize = 400;
        let walker = self.walker.as_ref()?;
        let holds = |element: &uiautomation::UIElement| {
            element.get_bounding_rectangle().is_ok_and(|r| {
                r.get_width() > 0
                    && r.get_height() > 0
                    && x >= r.get_left()
                    && x < r.get_left() + r.get_width()
                    && y >= r.get_top()
                    && y < r.get_top() + r.get_height()
            }) && element.is_offscreen().ok() != Some(true)
        };
        let root = self
            .client
            .element_from_handle(uiautomation::types::Handle::from(top))
            .ok()?;
        let mut current = root.clone();
        for _ in 0..MAX_DEPTH {
            let mut chosen = None;
            let mut child = walker.get_first_child(&current).ok();
            let mut seen = 0;
            while let Some(element) = child {
                if holds(&element) {
                    chosen = Some(element.clone());
                }
                seen += 1;
                if seen >= MAX_CHILDREN {
                    break;
                }
                child = walker.get_next_sibling(&element).ok();
            }
            match chosen {
                Some(next) => current = next,
                None => break,
            }
        }
        let found_inside = !self
            .client
            .compare_elements(&current, &root)
            .unwrap_or(true);
        found_inside.then(|| UIAElement::from(current))
    }

    /// A target with no element: a window, given by its handle, and a point.
    pub(super) fn of_window(&mut self, handle: isize, spot: (i32, i32)) -> Target {
        self.target(None, handle, Some(spot))
    }

    pub(super) fn focused(&mut self) -> Target {
        let element = self.client.get_focused_element().ok().map(UIAElement::from);
        let handle = element
            .as_ref()
            .and_then(|element| self.native_handle_of(element))
            .unwrap_or_else(|| unsafe { GetForegroundWindow() } as isize);
        let spot = window_center(root_of_handle(handle));
        self.target(element, handle, spot)
    }

    /// The point a target was found at, or its element's center.
    pub(super) fn spot_of(target: &Target) -> Option<(i32, i32)> {
        target.spot
    }

    /// The target of an action agent-desktop itself did on `element` (an
    /// agent's click through accessibility): its window, and its center as
    /// the spot.
    pub(super) fn of_element(&mut self, element: UIAElement) -> Target {
        let handle = self.native_handle_of(&element).unwrap_or(0);
        let spot = element
            .0
            .get_bounding_rectangle()
            .ok()
            .filter(|r| r.get_width() > 0 && r.get_height() > 0)
            .map(|r| {
                (
                    r.get_left() + r.get_width() / 2,
                    r.get_top() + r.get_height() / 2,
                )
            });
        self.target(Some(element), handle, spot)
    }

    /// Whether an action described by another process (an agent's, read from
    /// the agent inbox) passes the session's `--app`, `--window-id` or
    /// `--display` filter, as `keeps` does for the person's own.
    pub(super) fn keeps_described(
        &self,
        fields: &Map<String, Value>,
        spot: Option<(i32, i32)>,
    ) -> bool {
        let window = fields.get("window");
        let app = window
            .and_then(|w| w.get("app_name"))
            .and_then(Value::as_str);
        let id = window.and_then(|w| w.get("id")).and_then(Value::as_str);
        let owner = window
            .and_then(|w| w.get("owner_id"))
            .and_then(Value::as_str);
        if let Some(display) = &self.display
            && !spot.is_some_and(|(x, y)| contains(display, x, y))
        {
            return false;
        }
        self.app
            .as_deref()
            .is_none_or(|wanted| app.is_some_and(|app| app_name_matches(app, wanted)))
            && self
                .window_id
                .as_deref()
                .is_none_or(|wanted| id == Some(wanted) || owner == Some(wanted))
    }

    /// Whether an agent's action on a web page (agent-browser, through our
    /// DevTools proxy) belongs in this recording: yes unless it is narrowed
    /// to one window, or to an app that is not a Chromium browser.
    pub(super) fn keeps_browser(&self) -> bool {
        const BROWSERS: &[&str] = &["chrome.exe", "msedge.exe", "brave.exe", "chromium.exe"];
        self.window_id.is_none()
            && self.app.as_deref().is_none_or(|wanted| {
                BROWSERS
                    .iter()
                    .any(|browser| app_name_matches(browser, wanted))
            })
    }

    fn target(
        &mut self,
        element: Option<UIAElement>,
        handle: isize,
        spot: Option<(i32, i32)>,
    ) -> Target {
        let resolved = self.windows.resolve(handle, self.window_id.as_deref());
        Target {
            element,
            window: resolved.window,
            owner_id: resolved.owner_id,
            root: root_of_handle(handle),
            spot,
        }
    }

    /// Whether the target passes the session's `--app`, `--window-id` or
    /// `--display` filter. The recorder's own process and the console it runs
    /// in never pass, so stopping the session with Ctrl+C is not recorded.
    pub(super) fn keeps(&self, target: &Target) -> bool {
        let element_pid = target
            .element
            .as_ref()
            .and_then(|element| element.0.get_process_id().ok());
        if element_pid == Some(self.own_pid)
            || (element_pid.is_some() && element_pid == self.ignored_pid)
        {
            return false;
        }
        if let Some(display) = &self.display {
            let on_display = target.spot.is_some_and(|(x, y)| contains(display, x, y));
            if !on_display {
                return false;
            }
        }
        match &target.window {
            Some(window) if window.pid == self.own_pid => false,
            Some(window) if Some(u32::from(window.pid)) == self.ignored_pid => false,
            Some(window) if self.own_console.as_deref() == Some(window.id.as_str()) => false,
            Some(window) => {
                self.app
                    .as_deref()
                    .is_none_or(|app| app_name_matches(&window.app, app))
                    && self
                        .window_id
                        .as_deref()
                        .is_none_or(|id| window.id == id || target.owner_id.as_deref() == Some(id))
            }
            None => self.app.is_none() && self.window_id.is_none(),
        }
    }

    pub(super) fn same(&self, left: &UIAElement, right: &UIAElement) -> bool {
        self.client
            .compare_elements(&left.0, &right.0)
            .unwrap_or(false)
    }

    /// Whether a target has no element that answers: none, or one whose
    /// properties all failed (it was replaced as the pointer was released).
    pub(super) fn blank(&self, target: &Target) -> bool {
        target
            .element
            .as_ref()
            .is_none_or(|element| element_json(&read_live(element).0).get("role").is_none())
    }

    pub(super) fn describe(&self, target: &Target) -> Map<String, Value> {
        let mut fields = Map::new();
        if let Some(window) = &target.window {
            let mut described = window_json(window);
            // Only a browser's own frame has a page; a dialog of its process (the file picker)
            // has none, and searching its large tree made the agent's actions there slow.
            if is_browser(&window.app) && is_browser_frame(target.root) {
                // The page that holds the element: the tab that received the action. The
                // window's address, kept by title, is only the fallback (tabs of one web app
                // often share a title).
                let page = match (&target.element, self.walker.as_ref()) {
                    (Some(element), Some(walker)) => page_of(walker, element),
                    _ => None,
                };
                let url = page.or_else(|| {
                    let key = (target.root, window.title.clone());
                    self.addresses
                        .borrow_mut()
                        .entry(key)
                        .or_insert_with(|| address(&self.client, target.root))
                        .clone()
                });
                if let Some(url) = url {
                    described["url"] = json!(url);
                }
            }
            // A File Explorer window's title names only its last folder: the path, so a
            // replay opens the same one.
            if window.app.eq_ignore_ascii_case("explorer.exe")
                && let Some(folder) = super::record_folder::folder_of(target.root)
            {
                described["folder"] = json!(folder);
            }
            // A dialog of another window (Chrome's file picker runs in another process): the
            // window that opened it, so a filter on that window keeps it and a script finds it.
            if let Some(owner) = target
                .owner_id
                .as_deref()
                .filter(|owner| *owner != window.id)
            {
                described["owner_id"] = json!(owner);
            }
            fields.insert("window".into(), described);
        }
        let Some(element) = &target.element else {
            return fields;
        };
        let (properties, _) = read_live(element);
        let mut described = element_json(&properties);
        let unnamed = ["name", "native_id", "help_text"]
            .iter()
            .all(|key| described.get(*key).is_none())
            && described
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(is_field_role);
        if let (true, Some(walker)) = (unnamed, self.walker.as_ref()) {
            if let Some(label) = nearby_label(walker, &element.0) {
                described["label"] = json!(label);
            }
        }
        fields.insert("element".into(), described);
        let ancestors = self.ancestors(element);
        if !ancestors.is_empty() {
            fields.insert("ancestors".into(), Value::Array(ancestors));
        }
        if let Some(count) = self.match_count(&properties, target.root) {
            fields.insert("match_count".into(), json!(count));
        }
        if let Some(selection) = self.selection(element) {
            fields.insert("selection".into(), selection);
        }
        fields
    }

    pub(super) fn value(&self, element: &UIAElement) -> FieldValue {
        if read_one(element, TreeProperty::IsPassword).flag() == Some(true) {
            return FieldValue::Secret;
        }
        match read_one(element, TreeProperty::Value) {
            PropertyOutcome::Known(PropertyValue::Text(text)) => {
                let trimmed = text.strip_suffix('\r').unwrap_or(&text).to_string();
                FieldValue::Text(trimmed)
            }
            _ => FieldValue::Unavailable,
        }
    }

    fn ancestors(&self, element: &UIAElement) -> Vec<Value> {
        let Some(walker) = self.walker.as_ref() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let mut current = element.0.clone();
        for _ in 0..MAX_PARENT_STEPS {
            let Ok(parent) = walker.get_parent(&current) else {
                break;
            };
            let (properties, _) = read_live(&UIAElement::from(parent.clone()));
            let described = element_json(&properties);
            if described.get("role").and_then(Value::as_str) == Some("window") {
                break;
            }
            if described.get("name").is_some() || described.get("native_id").is_some() {
                found.push(described);
                if found.len() == MAX_ANCESTORS {
                    break;
                }
            }
            current = parent;
        }
        found
    }

    /// Counts the elements in the action's own top-level window (the popup,
    /// for an action inside one) that share the target's control type and
    /// identity.
    fn match_count(&self, properties: &ElementProperties, root: isize) -> Option<usize> {
        if root == 0 {
            return None;
        }
        let control_type = properties.get(TreeProperty::ControlType).number()?;
        let (property, value) = match (
            properties.get(TreeProperty::AutomationId).non_blank_text(),
            properties.get(TreeProperty::Name).non_blank_text(),
        ) {
            (Some(native_id), _) => (UIProperty::AutomationId, native_id),
            (None, Some(name)) => (UIProperty::Name, name),
            (None, None) => return None,
        };
        let by_type = self
            .client
            .create_property_condition(UIProperty::ControlType, Variant::from(control_type), None)
            .ok()?;
        let by_identity = self
            .client
            .create_property_condition(property, Variant::from(value), None)
            .ok()?;
        let condition = self
            .client
            .create_and_condition(by_type, by_identity)
            .ok()?;
        let root = self.client.element_from_handle(Handle::from(root)).ok()?;
        match root.find_all(TreeScope::Descendants, &condition) {
            Ok(found) => Some(found.len()),
            Err(_) => Some(0),
        }
    }

    fn selection(&self, element: &UIAElement) -> Option<Value> {
        let pattern = element.0.get_pattern::<UISelectionItemPattern>().ok()?;
        let mut selection = Map::new();
        if let Ok(container) = pattern.get_selection_container() {
            let (properties, _) = read_live(&UIAElement::from(container));
            selection.insert("container".into(), element_json(&properties));
        }
        Some(Value::Object(selection))
    }

    /// The state a click left: `checked` for a toggle (`mixed` when
    /// indeterminate) and `selected` for a selectable item.
    pub(super) fn state_after(&self, element: &UIAElement) -> Map<String, Value> {
        let mut state = Map::new();
        if let Ok(toggle) = element.0.get_pattern::<UITogglePattern>() {
            if let Ok(value) = toggle.get_toggle_state() {
                let checked = match value {
                    ToggleState::On => json!(true),
                    ToggleState::Off => json!(false),
                    ToggleState::Indeterminate => json!("mixed"),
                };
                state.insert("checked".into(), checked);
            }
        }
        if let Ok(item) = element.0.get_pattern::<UISelectionItemPattern>() {
            if let Ok(selected) = item.is_selected() {
                state.insert("selected".into(), json!(selected));
            }
        }
        state
    }

    /// The text a combo box shows as its current choice.
    pub(super) fn choice_value(&self, element: &UIAElement) -> Option<String> {
        read_one(element, TreeProperty::Value).non_blank_text()
    }

    fn native_handle_of(&self, element: &UIAElement) -> Option<isize> {
        let mut current = element.0.clone();
        for _ in 0..MAX_PARENT_STEPS {
            if let Ok(handle) = current.get_native_window_handle() {
                let raw: isize = handle.into();
                if raw != 0 {
                    return Some(raw);
                }
            }
            current = self.walker.as_ref()?.get_parent(&current).ok()?;
        }
        None
    }
}

fn window_json(window: &WindowInfo) -> Value {
    let mut fields = json!({
        "id": window.id,
        "title": window.title,
        "app_name": window.app,
        "pid": window.pid.get(),
    });
    // Read now, while the app runs: the demonstration may close it before the end.
    if let Some(path) = super::record_process::image_path(window.pid.get()) {
        fields["path"] = json!(path);
    }
    if let Some(bounds) = &window.bounds {
        fields["bounds"] =
            json!({"x": bounds.x, "y": bounds.y, "width": bounds.width, "height": bounds.height});
    }
    fields
}

fn element_json(properties: &ElementProperties) -> Value {
    let mut element = Map::new();
    if let LocatorField::Known(role) = resolve_role(properties) {
        element.insert("role".into(), json!(role));
    }
    for (key, property) in [
        ("name", TreeProperty::Name),
        ("native_id", TreeProperty::AutomationId),
        ("class_name", TreeProperty::ClassName),
        ("help_text", TreeProperty::HelpText),
    ] {
        if let Some(text) = properties.get(property).non_blank_text() {
            element.insert(key.into(), json!(text));
        }
    }
    if let LocatorField::Known(bounds) = properties.get(TreeProperty::BoundingRectangle).bounds() {
        element.insert(
            "bounds".into(),
            json!({"x": bounds.x, "y": bounds.y, "width": bounds.width, "height": bounds.height}),
        );
    }
    Value::Object(element)
}

/// Whether an element is inside a web page (a `document` above it).
fn in_page(walker: &uiautomation::UITreeWalker, element: &uiautomation::UIElement) -> bool {
    let mut current = element.clone();
    for _ in 0..24 {
        let Ok(parent) = walker.get_parent(&current) else {
            return false;
        };
        let role = element_json(&read_live(&UIAElement::from(parent.clone())).0)
            .get("role")
            .and_then(Value::as_str)
            .map(str::to_owned);
        match role.as_deref() {
            Some("document") => return true,
            Some("window") => return false,
            _ => current = parent,
        }
    }
    false
}

/// The control an unnamed web block stands for: the nearest option, link or
/// button around it (a few levels up), else the one radio button or checkbox
/// its wrapper holds (a component library's box beside a hidden input).
fn web_control(walker: &uiautomation::UITreeWalker, block: &UIAElement) -> Option<UIAElement> {
    let role_of = |element: &uiautomation::UIElement| {
        element_json(&read_live(&UIAElement::from(element.clone())).0)
            .get("role")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let mut current = block.0.clone();
    let mut wrapper = None;
    for step in 0..PROMOTE_STEPS + 2 {
        let parent = walker.get_parent(&current).ok()?;
        let role = role_of(&parent);
        if matches!(role.as_deref(), Some("document" | "window")) {
            break;
        }
        if is_actionable(role.as_deref()) {
            return Some(UIAElement::from(parent));
        }
        if step == 0 {
            wrapper = Some(parent.clone());
        }
        current = parent;
    }
    // The block itself may be the wrapper (the input inside it), else the
    // element around it holds the input beside it.
    let inputs = wrapped_inputs(walker, &block.0, &role_of);
    if inputs.len() == 1 {
        return inputs.into_iter().next().map(UIAElement::from);
    }
    let inputs = wrapped_inputs(walker, &wrapper?, &role_of);
    (inputs.len() == 1)
        .then(|| inputs.into_iter().next().map(UIAElement::from))
        .flatten()
}

fn wrapped_inputs(
    walker: &uiautomation::UITreeWalker,
    wrapper: &uiautomation::UIElement,
    role_of: &dyn Fn(&uiautomation::UIElement) -> Option<String>,
) -> Vec<uiautomation::UIElement> {
    let mut inputs = Vec::new();
    let mut stack = vec![(wrapper.clone(), 0usize)];
    while let Some((node, depth)) = stack.pop() {
        let mut child = walker.get_first_child(&node).ok();
        while let Some(item) = child {
            if role_of(&item).is_some_and(|role| WRAPPED_INPUTS.contains(&role.as_str())) {
                inputs.push(item.clone());
            } else if depth < 2 {
                stack.push((item.clone(), depth + 1));
            }
            child = walker.get_next_sibling(&item).ok();
        }
    }
    inputs
}
