use gpui::{Hsla, IntoElement, RenderOnce, Window, prelude::*, px, svg};

#[derive(Clone, Copy, Debug, IntoElement)]
pub struct Icon {
    pub path: &'static str,
    pub size: f32,
    pub color: Hsla,
}

impl Icon {
    pub fn new(path: &'static str, size: f32, color: Hsla) -> Self {
        Self { path, size, color }
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, _cx: &mut gpui::App) -> impl IntoElement {
        svg()
            .path(self.path)
            .size(px(self.size))
            .text_color(self.color)
    }
}
