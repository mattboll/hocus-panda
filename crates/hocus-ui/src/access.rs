//! The accessibility tree: what each widget is and says, for tests and
//! agents today, and for screen readers through AT-SPI later (features §21).

use std::fmt::Write as _;

use crate::geom::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A container; its children speak for it.
    Group,
    Text,
    Button,
    Image,
    /// An icon with a meaning; decorative ones are left out.
    Icon,
    /// A widget drawn by its own code, labelled by its author.
    Custom,
    /// A text field.
    Field,
    /// A value along a track; its label ends with the value in percent.
    Slider,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::Text => "text",
            Self::Button => "button",
            Self::Image => "image",
            Self::Icon => "icon",
            Self::Custom => "custom",
            Self::Field => "field",
            Self::Slider => "slider",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessNode {
    pub role: Role,
    pub label: Option<String>,
    pub rect: Rect,
    pub focused: bool,
    /// Shown but not usable.
    pub disabled: bool,
    /// On, for toggles.
    pub selected: bool,
    pub children: Vec<AccessNode>,
}

impl AccessNode {
    /// One line per node, indented by depth:
    /// `button "Valider" 16,40 120x32 focused`.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, depth: usize) {
        let _ = write!(
            out,
            "{:indent$}{}",
            "",
            self.role.name(),
            indent = depth * 2
        );
        if let Some(label) = &self.label {
            // Quoted, but readable: no escapes for accents or thin spaces.
            let _ = write!(out, " \"{}\"", label.replace('"', "\\\""));
        }
        let r = self.rect;
        let _ = write!(out, " {},{} {}x{}", r.x, r.y, r.w, r.h);
        for (on, state) in [
            (self.focused, " focused"),
            (self.selected, " selected"),
            (self.disabled, " disabled"),
        ] {
            if on {
                out.push_str(state);
            }
        }
        out.push('\n');
        for child in &self.children {
            child.write(out, depth + 1);
        }
    }

    /// This node and all below, depth first.
    pub fn walk(&self) -> Vec<&AccessNode> {
        let mut all = vec![self];
        for child in &self.children {
            all.extend(child.walk());
        }
        all
    }
}
