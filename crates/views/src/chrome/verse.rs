use std::{cell::RefCell, rc::Rc};

use gpui::{
    App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, SharedString, Size, Window, WrappedLine, point, px,
};

#[derive(Clone, Copy)]
pub(crate) struct Letter {
    pub color: Hsla,
    pub lift: Pixels,
    pub glow: f32,
}

pub(crate) type Look = Rc<dyn Fn(usize) -> Letter>;

/// A line of lyrics the text system lays out and wraps on its own, painted one
/// glyph at a time so each letter can carry its own colour, lift and glow.
pub(crate) struct Verse {
    text: SharedString,
    look: Look,
    right: bool,
    glow: Option<Hsla>,
    shaped: Rc<RefCell<Option<Shaped>>>,
}

struct Shaped {
    lines: Vec<WrappedLine>,
    line_height: Pixels,
    width: Option<Pixels>,
    size: Size<Pixels>,
}

impl Verse {
    pub(crate) fn new(text: impl Into<SharedString>, look: Look) -> Self {
        Self {
            text: text.into(),
            look,
            right: false,
            glow: None,
            shaped: Rc::default(),
        }
    }

    pub(crate) fn right(mut self, right: bool) -> Self {
        self.right = right;
        self
    }

    // paints only the glow, for a blurred layer underneath
    pub(crate) fn glow(mut self, color: Hsla) -> Self {
        self.glow = Some(color);
        self
    }
}

impl IntoElement for Verse {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Verse {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.pixel_snap(
            style
                .line_height
                .to_pixels(font_size.into(), window.rem_size()),
        );
        let run = style.to_run(self.text.len());
        let text = self.text.clone();
        let shaped = self.shaped.clone();
        let id =
            window.request_measured_layout(Default::default(), move |known, space, window, _| {
                let width = known.width.or(match space.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                if let Some(done) = shaped.borrow().as_ref()
                    && (width.is_none() || width == done.width)
                {
                    return done.size;
                }
                let lines = window
                    .text_system()
                    .shape_text(
                        text.clone(),
                        font_size,
                        std::slice::from_ref(&run),
                        width,
                        None,
                    )
                    .map(|lines| lines.into_iter().collect::<Vec<_>>())
                    .unwrap_or_default();
                let mut size = Size::<Pixels>::default();
                for line in &lines {
                    let line = line.size(line_height);
                    size.height += line.height;
                    size.width = size.width.max(line.width).ceil();
                }
                shaped.borrow_mut().replace(Shaped {
                    lines,
                    line_height,
                    width,
                    size,
                });
                size
            });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let shaped = self.shaped.borrow();
        let Some(shaped) = shaped.as_ref() else {
            return;
        };
        let height = shaped.line_height;
        let mut top = bounds.origin.y;
        let mut offset = 0;
        for line in &shaped.lines {
            let layout = &line.unwrapped_layout;
            let baseline = (height - layout.ascent - layout.descent) / 2. + layout.ascent;
            let starts = rows(line);
            let mut row = 0;
            for (run_ix, run) in layout.runs.iter().enumerate() {
                for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
                    while starts
                        .get(row + 1)
                        .is_some_and(|start| (start.0, start.1) <= (run_ix, glyph_ix))
                    {
                        row += 1;
                    }
                    let (_, _, from, to) = starts[row];
                    let indent = match self.right {
                        true => bounds.size.width - (to - from),
                        false => px(0.),
                    };
                    let letter = (self.look)(offset + glyph.index);
                    let origin = point(
                        bounds.origin.x + indent + glyph.position.x - from,
                        top + height * row as f32 + baseline - letter.lift,
                    );
                    let color = match self.glow {
                        Some(_) if letter.glow <= 0.01 => continue,
                        Some(glow) => glow.opacity(glow.a * letter.glow.min(1.)),
                        None => letter.color,
                    };
                    let _ = match glyph.is_emoji {
                        true if self.glow.is_none() => {
                            window.paint_emoji(origin, run.font_id, glyph.id, layout.font_size)
                        }
                        true => Ok(()),
                        false => window.paint_glyph(
                            origin,
                            run.font_id,
                            glyph.id,
                            layout.font_size,
                            color,
                        ),
                    };
                }
            }
            top += height * starts.len() as f32;
            offset += layout.len + 1;
        }
    }
}

// where each wrapped row starts, and the x it runs from and to
fn rows(line: &WrappedLine) -> Vec<(usize, usize, Pixels, Pixels)> {
    let layout = &line.unwrapped_layout;
    let x = |run: usize, glyph: usize| {
        layout
            .runs
            .get(run)
            .and_then(|run| run.glyphs.get(glyph))
            .map_or(layout.width, |glyph| glyph.position.x)
    };
    let mut starts = vec![(0, 0, px(0.), px(0.))];
    starts.extend(line.wrap_boundaries.iter().map(|wrap| {
        (
            wrap.run_ix,
            wrap.glyph_ix,
            x(wrap.run_ix, wrap.glyph_ix),
            px(0.),
        )
    }));
    let ends = starts
        .iter()
        .skip(1)
        .map(|start| start.2)
        .chain([layout.width])
        .collect::<Vec<_>>();
    starts
        .into_iter()
        .zip(ends)
        .map(|((run, glyph, from, _), to)| (run, glyph, from, to))
        .collect()
}
