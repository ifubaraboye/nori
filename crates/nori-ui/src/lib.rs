mod actions;
mod components;
pub mod input;
mod model;
mod theme;
mod views;

pub use actions::{ToggleSidebar, register_key_bindings};
pub use theme::Theme;
pub use views::MailApp;
