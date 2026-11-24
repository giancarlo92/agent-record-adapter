// Part of Agent Record Adapter (see NOTICE).
//! Completes a click once the application has handled it.
//!
//! The pointer press is described at once, because a menu item or a popup
//! option can vanish as soon as the click lands. What the click *did* is read
//! a moment later: the state it left (`checked`, `selected`), and, when the
//! press hit a container such as a form group or the page itself, the control
//! that took focus from it, which is the control the user meant (a browser
//! answers a hit on a checkbox's label with the surrounding group).
//!
//! A click on a combo box also opens a watch on its value. A choice made
//! by releasing the button over an option, from the keyboard, or inside a
//! popup is recorded as a `select` event carrying the value the combo box
//! ended with. The watch stays open across later clicks, because the click
//! that picks an option lands in the popup, and the value changes only after
//! that click is handled.

use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::record_element::{Observer, Target};
use crate::tree::element::UIAElement;

pub(super) const CLICK_SETTLE: Duration = Duration::from_millis(150);
const CHOICE_WATCH: Duration = Duration::from_secs(20);
const CONTAINER_ROLES: &[&str] = &[
    "group",
    "document",
    "pane",
    "window",
    "unknown",
    "statictext",
];

pub(super) struct PendingClick {
    pub(super) at: Instant,
    fields: Map<String, Value>,
    hit: Option<UIAElement>,
    focus_before: Option<UIAElement>,
    state_before: Map<String, Value>,
    window_id: Option<String>,
}

pub(super) struct ChoiceWatch {
    element: UIAElement,
    fields: Map<String, Value>,
    before: Option<String>,
    until: Instant,
}

pub(super) fn role_of(fields: &Map<String, Value>) -> Option<&str> {
    fields.get("element")?.get("role")?.as_str()
}

/// Whether a click's target is a container rather than a control: the
/// click inside an open combo box's popup is one, and its `select` event
/// already records the choice.
pub(super) fn is_container(fields: &Map<String, Value>) -> bool {
    role_of(fields).is_some_and(|role| CONTAINER_ROLES.contains(&role))
}

impl PendingClick {
    pub(super) fn new(
        observer: &mut Observer,
        target: Target,
        fields: Map<String, Value>,
        at: Instant,
    ) -> Self {
        let focus_before = observer.focused().element;
        let state_before = focus_before
            .as_ref()
            .map(|element| observer.state_after(element))
            .unwrap_or_default();
        Self {
            at,
            window_id: target.window.as_ref().map(|window| window.id.clone()),
            hit: target.element,
            fields,
            focus_before,
            state_before,
        }
    }

    /// The finished `click` fields, and the watch to open when the click
    /// landed on a combo box.
    pub(super) fn resolve(
        self,
        observer: &mut Observer,
    ) -> (Map<String, Value>, Option<ChoiceWatch>) {
        let moved = if is_container(&self.fields) {
            self.focus_moved(observer)
        } else {
            None
        };
        let at = self.at;
        let mut fields = self.fields;
        let mut subject = self.hit;
        if let Some((element, described)) = moved {
            let hit = fields.remove("element").unwrap_or(Value::Null);
            for key in ["ancestors", "match_count", "selection"] {
                fields.remove(key);
            }
            for (key, value) in described {
                if key != "window" {
                    fields.insert(key, value);
                }
            }
            fields.insert("hit".into(), hit);
            subject = Some(element);
        }
        let Some(element) = subject else {
            return (fields, None);
        };
        let state = observer.state_after(&element);
        if !state.is_empty() {
            fields.insert("state_after".into(), Value::Object(state));
        }
        let watch = (role_of(&fields) == Some("combobox")).then(|| ChoiceWatch {
            before: observer.choice_value(&element),
            fields: watch_fields(&fields),
            element,
            until: at + CHOICE_WATCH,
        });
        (fields, watch)
    }

    /// The control the click acted on, when the press hit a container: the
    /// control that took focus, or the already focused control whose state
    /// the click changed (a second click on a checkbox's label).
    fn focus_moved(&self, observer: &mut Observer) -> Option<(UIAElement, Map<String, Value>)> {
        let target = observer.focused();
        let element = target.element.clone()?;
        let same_window =
            target.window.as_ref().map(|window| window.id.as_str()) == self.window_id.as_deref();
        let refocused = self
            .focus_before
            .as_ref()
            .is_some_and(|before| observer.same(before, &element));
        let acted_in_place = refocused && observer.state_after(&element) != self.state_before;
        if !same_window || (refocused && !acted_in_place) {
            return None;
        }
        let described = observer.describe(&target);
        let role = role_of(&described)?;
        if CONTAINER_ROLES.contains(&role) {
            return None;
        }
        Some((element, described))
    }
}

impl ChoiceWatch {
    /// Whether a click's fields name the watched combo box.
    pub(super) fn targets(&self, click: &Map<String, Value>) -> bool {
        self.fields.get("element") == click.get("element")
    }

    /// The `select` fields when the combo box's value changed since the
    /// click, and whether the watch should stay open.
    pub(super) fn poll(
        &mut self,
        observer: &Observer,
        now: Instant,
    ) -> (Option<Map<String, Value>>, bool) {
        let current = observer.choice_value(&self.element);
        if current.is_some() && current != self.before {
            let mut fields = self.fields.clone();
            fields.insert("value".into(), json!(current));
            if let Some(before) = &self.before {
                fields.insert("previous".into(), json!(before));
            }
            self.before = current;
            return (Some(fields), now < self.until);
        }
        (None, now < self.until)
    }
}

fn watch_fields(click: &Map<String, Value>) -> Map<String, Value> {
    ["element", "window", "ancestors", "match_count"]
        .into_iter()
        .filter_map(|key| click.get(key).map(|value| (key.to_string(), value.clone())))
        .collect()
}
