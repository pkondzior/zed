use gpui::{
    AnyElement, App, AppContext, Application, Bounds, CaretAffinity, Context, ElementId,
    InlineDecoration, InlineDecorationStyle, InlineElementAlign, InlineSpanLayout, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render, Rgba,
    SharedString, Styled, TextStyle, Window, WindowBounds, WindowOptions, canvas, div, inline,
    prelude::*, px, relative, rgb, rgba, span,
};
use std::ops::Range;

const TOOLTIP_BORDER_RADIUS: f32 = 4.0;

struct TooltipView {
    text: String,
}

impl Render for TooltipView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .bg(rgb(0x000000))
            .text_color(rgb(0xffffff))
            .text_xs()
            .rounded(px(TOOLTIP_BORDER_RADIUS))
            .child(self.text.clone())
    }
}

struct InlineSpanExample {
    selection_ranges: [Option<Range<usize>>; 3],
    selection_anchors: [Option<usize>; 3],
    layouts: [InlineSpanLayout; 3],
    selecting: [bool; 3],
    bounds: [Bounds<gpui::Pixels>; 3],
    badge_scale: f32, // 1.0 to 10.0
    show_line_outlines: bool,
    show_fragment_outlines: bool,
}

impl InlineSpanExample {
    fn new() -> Self {
        Self {
            selection_ranges: Default::default(),
            selection_anchors: Default::default(),
            layouts: Default::default(),
            selecting: [false; 3],
            bounds: [Bounds::default(); 3],
            badge_scale: 1.0,
            show_line_outlines: false,
            show_fragment_outlines: false,
        }
    }

    fn update_selection(
        &mut self,
        line_index: usize,
        range: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        if line_index >= 3 {
            return;
        }
        self.selection_ranges[line_index] = range.clone();
        if let Some(range) = range {
            self.layouts[line_index].set_decorations(vec![InlineDecoration {
                range,
                style: InlineDecorationStyle {
                    background: rgba(0x3366ff40).into(),
                },
            }]);
        } else {
            self.layouts[line_index].set_decorations(vec![]);
        }
        cx.notify();
    }

    fn create_interactive_example(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        font_size_offset: f32,
        line_index: usize,
    ) -> gpui::Div {
        let font_size_base = 14.0;
        let font_size = px(font_size_base + font_size_offset);
        let badge_scale = self.badge_scale;

        let mut base_style = window.text_style().clone();
        base_style.font_size = font_size.into();
        base_style.color = rgb(0x000000).into();
        base_style.line_height = relative(1.5);

        let mut interactive_builder = inline(); //.with_style(base_style.clone());
        interactive_builder
            .push_text("This is an example with ", base_style.clone())
            .push_element_aligned(InlineElementAlign::Top, move || {
                create_badge(
                    "badge",
                    rgba(0xff5555ff),
                    "Aligned to top",
                    badge_scale,
                    ("badge", line_index),
                )
            })
            .push_text(" that ", base_style.clone())
            .push_element_aligned(InlineElementAlign::Middle, move || {
                create_badge(
                    "has",
                    rgba(0xffaa55ff),
                    "Aligned to middle",
                    badge_scale,
                    ("has", line_index),
                )
            })
            .push_text(" different ", base_style.clone())
            .push_element_aligned(InlineElementAlign::Bottom, move || {
                create_badge(
                    "alignment",
                    rgba(0xffff55ff),
                    "Aligned to bottom",
                    badge_scale,
                    ("alignment", line_index),
                )
            })
            .push_text(" for each ", base_style.clone())
            .push_element_aligned(InlineElementAlign::Auto, move || {
                create_badge(
                    "badge",
                    rgba(0x55ff55ff),
                    "Auto aligned",
                    badge_scale,
                    ("badge2", line_index),
                )
            })
            .push_text(" element in the ", base_style.clone())
            .push_element_aligned(InlineElementAlign::Top, move || {
                create_badge(
                    "text",
                    rgba(0x55ffffff),
                    "Aligned to top",
                    badge_scale,
                    ("text", line_index),
                )
            })
            .push_text(". ", base_style.clone());

        // Track layout for each line (need to build to get layout state)
        let interactive_span = interactive_builder.build();
        self.layouts[line_index] = interactive_span.layout_state();

        let view_entity = cx.entity().clone();

        let layout = self.layouts[line_index].clone();
        let show_line_outlines = self.show_line_outlines;
        let show_fragment_outlines = self.show_fragment_outlines;
        let debug_overlay = if show_line_outlines || show_fragment_outlines {
            Some(div().absolute().inset_0().child(canvas(
                |_bounds, _window, _cx| {},
                move |_bounds, (), window, _cx| {
                    if let Some(span_bounds) = layout.bounds() {
                        // 1. Paint Line Boxes (Blue Dashed)
                        if show_line_outlines {
                            for line in layout.line_geometries() {
                                let line_bounds = Bounds::new(
                                    span_bounds.origin + line.bounds.origin,
                                    line.bounds.size,
                                );
                                window.paint_quad(gpui::outline(
                                    line_bounds,
                                    rgba(0x3b82f680), // Blue
                                    gpui::BorderStyle::Dashed,
                                ));
                            }
                        }

                        // 2. Paint Fragment Boxes
                        if show_fragment_outlines {
                            let fragments = layout.fragment_geometries();
                            for fragment in fragments {
                                let (bounds, color) = match fragment {
                                    gpui::InlineFragmentGeometry::Text { bounds, .. } => {
                                        (bounds, rgba(0x10b98180)) // Green for text
                                    }
                                    gpui::InlineFragmentGeometry::Element { bounds, .. } => {
                                        (bounds, rgba(0xd946efc0)) // Magenta for elements
                                    }
                                };

                                let fragment_bounds =
                                    Bounds::new(span_bounds.origin + bounds.origin, bounds.size);
                                window.paint_quad(gpui::outline(
                                    fragment_bounds,
                                    color,
                                    gpui::BorderStyle::Solid,
                                ));
                            }
                        }
                    }
                },
            )))
        } else {
            None
        };
        return div().p_4().child(
            div()
                .id(("interactive-span", line_index))
                .relative()
                .cursor(gpui::CursorStyle::IBeam)
                .child(interactive_span)
                .child({
                    let view_entity_prepaint = view_entity.clone();
                    let view_entity_paint = view_entity.clone();
                    canvas(
                        move |bounds, _window, cx| {
                            view_entity_prepaint.update(cx, |this, _cx| {
                                this.bounds[line_index] = bounds;
                            });
                        },
                        move |_bounds, _selection_range, window, cx| {
                            let example = view_entity_paint.read(cx);
                            let layout = &example.layouts[line_index];

                            // Paint selection overlay for this line
                            if let Some(bounds) = layout.bounds() {
                                if let Some(range) = example.selection_ranges[line_index].clone() {
                                    for rect in layout.selection_rects(range) {
                                        let selection_bounds = gpui::Bounds::new(
                                            gpui::Point::new(
                                                bounds.origin.x + rect.origin.x,
                                                bounds.origin.y + rect.origin.y,
                                            ),
                                            rect.size,
                                        );
                                        window.paint_quad(gpui::fill(
                                            selection_bounds,
                                            rgba(0x3366ff66),
                                        ));
                                    }
                                }
                            }
                        },
                    )
                    .absolute()
                    .inset_0()
                })
                .children(debug_overlay)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                        let offset = event.position - this.bounds[line_index].origin;
                        if let Some(index) = this.layouts[line_index]
                            .logical_index_for_point(offset, CaretAffinity::Upstream)
                        {
                            this.selecting[line_index] = true;
                            this.selection_anchors[line_index] = Some(index);
                            this.update_selection(line_index, Some(index..index), cx);
                        }
                    }),
                )
                .on_mouse_move(
                    cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                        if this.selecting[line_index] {
                            let offset = event.position - this.bounds[line_index].origin;
                            if let Some(index) = this.layouts[line_index]
                                .logical_index_for_point(offset, CaretAffinity::Upstream)
                            {
                                if let Some(anchor) = this.selection_anchors[line_index] {
                                    let range = if anchor <= index {
                                        anchor..index
                                    } else {
                                        index..anchor
                                    };
                                    this.update_selection(line_index, Some(range), cx);
                                }
                            }
                        }
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseUpEvent, _window, _cx| {
                        this.selecting[line_index] = false;
                        this.selection_anchors[line_index] = None;
                    }),
                ),
        );
    }
}

fn create_badge(
    label: impl Into<SharedString>,
    color: Rgba,
    tooltip_text: impl Into<String>,
    scale: f32,
    id: impl Into<ElementId>,
) -> AnyElement {
    let label = label.into();
    let tooltip_text = tooltip_text.into();

    div()
        .flex()
        .items_center()
        .justify_center()
        .bg(color)
        .rounded_md()
        .px(px(2.0 * scale))
        .text_size(px(10.0 * scale))
        .line_height(px(14.0 * scale))
        .border_1()
        .border_color(rgb(0x000000))
        .relative()
        .hover(|s| s.border_2().border_color(rgb(0xff0000)))
        .font_weight(gpui::FontWeight::BOLD)
        .id(id)
        .tooltip(move |_window, cx| {
            cx.new(|_cx| TooltipView {
                text: tooltip_text.clone(),
            })
            .into()
        })
        .child(label)
        .into_any_element()
}

impl Render for InlineSpanExample {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div()
            .id("main-scroll")
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0xffffff))
            .overflow_y_scroll();

        // Controls: Badge scale and debug outlines toggle
        col = col.child(
            div().p_4().border_b_1().border_color(rgb(0xcccccc)).child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_sm().child("Badge Scale:"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .bg(rgb(0xeeeeee))
                                            .rounded(px(4.0))
                                            .cursor(gpui::CursorStyle::PointingHand)
                                            .child("-")
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    |this, _event: &MouseDownEvent, _window, cx| {
                                                        this.badge_scale =
                                                            (this.badge_scale - 0.5).max(1.0);
                                                        cx.notify();
                                                    },
                                                ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .px_3()
                                            .text_sm()
                                            .child(format!("{:.1}x", self.badge_scale)),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .bg(rgb(0xeeeeee))
                                            .rounded(px(4.0))
                                            .cursor(gpui::CursorStyle::PointingHand)
                                            .child("+")
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    |this, _event: &MouseDownEvent, _window, cx| {
                                                        this.badge_scale =
                                                            (this.badge_scale + 0.5).min(10.0);
                                                        cx.notify();
                                                    },
                                                ),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_sm().child("Lines:"))
                            .child(
                                div()
                                    .w(px(50.0))
                                    .h(px(30.0))
                                    .rounded_full()
                                    .bg(if self.show_line_outlines {
                                        rgb(0x4ade80)
                                    } else {
                                        rgb(0xcccccc)
                                    })
                                    .relative()
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(2.0))
                                            .left(if self.show_line_outlines {
                                                px(22.0)
                                            } else {
                                                px(2.0)
                                            })
                                            .w(px(26.0))
                                            .h(px(26.0))
                                            .rounded_full()
                                            .bg(rgb(0xffffff)),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.show_line_outlines = !this.show_line_outlines;
                                            cx.notify();
                                        }),
                                    ),
                            )
                            .child(div().text_sm().child("Fragments:"))
                            .child(
                                div()
                                    .w(px(50.0))
                                    .h(px(30.0))
                                    .rounded_full()
                                    .bg(if self.show_fragment_outlines {
                                        rgb(0x4ade80)
                                    } else {
                                        rgb(0xcccccc)
                                    })
                                    .relative()
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(2.0))
                                            .left(if self.show_fragment_outlines {
                                                px(22.0)
                                            } else {
                                                px(2.0)
                                            })
                                            .w(px(26.0))
                                            .h(px(26.0))
                                            .rounded_full()
                                            .bg(rgb(0xffffff)),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.show_fragment_outlines =
                                                !this.show_fragment_outlines;
                                            cx.notify();
                                        }),
                                    ),
                            ),
                    ),
            ),
        );

        // Section 1: Regular InlineSpans with varying sizes and badges

        col = col.child(
            div().p_4().border_t_1().border_color(rgb(0xcccccc)).child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::BOLD)
                    .mb_2()
                    .child("Interactive Section (Click and drag to select)"),
            ),
        );

        col = col.child(self.create_interactive_example(window, cx, 20.0, 0)); // Large
        col = col.child(self.create_interactive_example(window, cx, 10.0, 1)); // Medium
        col = col.child(self.create_interactive_example(window, cx, 0.0, 2)); // Small

        col = col.child(
            div().p_4().border_t_1().border_color(rgb(0xcccccc)).child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::BOLD)
                    .mb_2()
                    .child("Ergonomic API Showcase"),
            ),
        );

        col = col.child(
            div().p_4().child(
                inline()
                    .text("Hello, ", TextStyle::default())
                    .child(
                        div()
                            .bg(rgb(0xef4444)) // Red
                            .text_color(rgb(0xffffff))
                            .px_1()
                            .rounded_sm()
                            .child("World"),
                    )
                    .text("! This is ", TextStyle::default())
                    .child(span("bold").bold())
                    .text(" text with ", TextStyle::default())
                    .child(
                        div()
                            .w_4()
                            .h_4()
                            .bg(rgb(0x3b82f6)) // Blue
                            .rounded_full(),
                    )
                    .text(" an icon.", TextStyle::default()),
            ),
        );

        col
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, gpui::size(px(800.), px(600.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| InlineSpanExample::new()),
        )
        .unwrap();

        cx.activate(true);
    });
}
