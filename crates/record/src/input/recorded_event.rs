// Part of Agent Record Adapter (see NOTICE).
use serde_json::{Map, Value};

/// One user action a `record` session observed.
///
/// `kind` names the action (`click`, `type`, `press`, `scroll`) and
/// `elapsed_ms` is measured from the start of the session. `fields` carries
/// the platform's evidence about the target: the element's role, name and
/// native id, its window, and the action's own parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedEvent {
    pub kind: &'static str,
    pub elapsed_ms: u64,
    pub fields: Map<String, Value>,
}

impl RecordedEvent {
    /// The JSON line written for this event: `kind` and `t_ms` first, then
    /// the evidence fields, which never override those two keys.
    pub fn to_json(&self) -> Value {
        let mut line = Map::new();
        line.insert("kind".into(), Value::from(self.kind));
        line.insert("t_ms".into(), Value::from(self.elapsed_ms));
        for (key, value) in &self.fields {
            if key != "kind" && key != "t_ms" {
                line.insert(key.clone(), value.clone());
            }
        }
        Value::Object(line)
    }
}
