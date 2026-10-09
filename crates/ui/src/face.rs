use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{App, Styled};

/// Apple's system faces, each where Apple puts it: Text for reading, Display for large
/// type, Rounded for headings. Their licence forbids shipping them outside Apple's
/// platforms, so nothing here bundles them: a face is only used when the machine
/// already has it installed, and the bundled one stays otherwise.
#[derive(Clone, Copy)]
pub enum Face {
    Text,
    Display,
    Rounded,
}

impl Face {
    const ALL: [Self; 3] = [Self::Text, Self::Display, Self::Rounded];

    fn wanted(self) -> &'static [&'static str] {
        match self {
            Self::Text => &["SF Pro Text", "SF Pro"],
            Self::Display => &["SF Pro Display", "SF Pro"],
            Self::Rounded => &["SF Pro Rounded"],
        }
    }
}

static FOUND: OnceLock<[Option<&'static str>; 3]> = OnceLock::new();
// A typeface picked in settings wins over all of them.
static CHOSEN: AtomicBool = AtomicBool::new(false);

/// Looks the faces up once; fonts installed while Sonora runs show up next launch.
pub fn find_faces(cx: &App) {
    FOUND.get_or_init(|| {
        let installed = cx.text_system().all_font_names();
        Face::ALL.map(|face| {
            face.wanted()
                .iter()
                .copied()
                .find(|name| installed.iter().any(|family| family == name))
        })
    });
}

pub fn set_chosen_font(chosen: bool) {
    CHOSEN.store(chosen, Ordering::Relaxed);
}

pub fn face(face: Face) -> Option<&'static str> {
    if CHOSEN.load(Ordering::Relaxed) {
        return None;
    }
    FOUND.get()?[face as usize]
}

pub trait Faced: Styled + Sized {
    fn face(self, face: Face) -> Self {
        match self::face(face) {
            Some(family) => self.font_family(family),
            None => self,
        }
    }
}

impl<T: Styled> Faced for T {}
