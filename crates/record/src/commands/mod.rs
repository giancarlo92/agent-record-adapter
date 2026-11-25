// Part of Agent Record Adapter (see NOTICE).
pub mod click_at;
pub mod close_window;
pub mod menu;
pub mod navigate;
pub mod record;
pub mod record_background;
pub mod record_display;
pub mod secret;
pub mod secret_ask;
pub mod secret_policy;
pub mod secret_store;
pub mod stroke;
mod support;
pub mod type_window;
pub use support::{helpers, operation_response, point_resolve, window_target};
