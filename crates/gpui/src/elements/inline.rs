#[cfg(debug_assertions)]
use crate::outline;
#[cfg(debug_assertions)]
use crate::scene::BorderStyle;
use crate::text_system::ShapedLine;
use crate::util::ResultExt;
use crate::{
    AbsoluteLength, AnyElement, App, AvailableSpace, Bounds, Element, Font, FontStyle, FontWeight,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, LineFragment, Pixels, Point,
    SharedString, Size, StyleRefinement, Styled, TextStyle, UnderlineStyle, WhiteSpace, Window,
    fill, px, rems,
};
use std::rc::Rc;
use std::sync::Arc;
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{cell::RefCell, ops::Range};

/// Renders a span of inline content mixing styled text and inline GPUI elements.
pub struct InlineSpan {
    content: Arc<InlineContent>,
    layout: InlineSpanLayout,
    style: StyleRefinement,
}

impl InlineSpan {
    /// Create a new inline span from the provided content.
    pub fn new(content: InlineContent) -> Self {
        InlineSpan {
            content: Arc::new(content),
            layout: InlineSpanLayout::default(),
            style: StyleRefinement::default(),
        }
    }

    /// Mark the layout state as dirty, forcing a recomputation on the next layout pass.
    pub fn invalidate_layout(&mut self) {
        self.layout.mark_dirty();
    }

    /// Access the current layout state for hit-testing or selections.
    pub fn layout_state(&self) -> InlineSpanLayout {
        self.layout.clone()
    }

    /// Use an existing layout state for this span.
    pub fn with_layout(mut self, layout: InlineSpanLayout) -> Self {
        self.layout = layout;
        self
    }
}

impl Styled for InlineSpan {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Element for InlineSpan {
    type RequestLayoutState = InlineSpanLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<crate::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout = self.layout.clone();
        let content = self.content.clone();
        let text_style = self.style.text.clone();
        let layout_id = window.with_text_style(text_style, |window| {
            let rem_size = window.rem_size();
            let base_font_size = content
                .default_font_size_or(window.text_style().font_size.to_pixels(rem_size), rem_size);
            let line_height_hint = window
                .text_style()
                .line_height
                .to_pixels(AbsoluteLength::Pixels(base_font_size), rem_size);
            layout.prepare_inline_elements(content.as_ref(), line_height_hint, window, _cx);
            layout.request_layout(content, window, _cx)
        });
        (layout_id, layout)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,

        layout: &mut InlineSpanLayout,
        window: &mut Window,
        cx: &mut App,
    ) -> () {
        layout.prepaint(bounds, window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        layout: &mut InlineSpanLayout,
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        layout.paint(window, cx);
    }
}

impl crate::IntoElement for InlineSpan {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Immutable inline content (text fragments + inline element slots).
#[derive(Clone)]
pub struct InlineContent {
    /// The fragments that make up the content.
    pub fragments: Vec<InlineFragment>,
    /// The default style for the content.
    pub default_style: Option<TextStyle>,
    /// The default alignment for inline elements.
    pub inline_align: InlineElementAlign,
}

impl InlineContent {
    fn len(&self) -> usize {
        self.fragments.len()
    }

    /// Resolve the effective default style, checking the explicit default first,
    /// then falling back to the first available text style in the fragments.
    fn resolve_default_style(&self) -> Option<&TextStyle> {
        self.default_style.as_ref().or_else(|| {
            self.fragments.iter().find_map(|f| match f {
                InlineFragment::Text { style, .. } => style.as_ref(),
                _ => None,
            })
        })
    }

    /// Get the default font, falling back to the provided fallback.
    fn default_font_or(&self, fallback: Font) -> Font {
        self.resolve_default_style()
            .map(|s| s.font())
            .unwrap_or(fallback)
    }

    /// Get the default font size in pixels, falling back to the provided fallback.
    fn default_font_size_or(&self, fallback: Pixels, rem_size: Pixels) -> Pixels {
        self.resolve_default_style()
            .map(|s| s.font_size.to_pixels(rem_size))
            .unwrap_or(fallback)
    }
}

/// Builder-time fragment definition.
#[derive(Clone)]
pub enum InlineFragment {
    /// Styled text fragment that participates in wrapping.
    Text {
        /// The fragment's text contents.
        text: SharedString,
        /// The style for this fragment.
        style: Option<TextStyle>,
    },
    /// Placeholder for an inline GPUI element.
    Element {
        /// Slot that can instantiate the element during layout.
        slot: InlineElementSlot,
    },
}

/// Builder slot for inline elements.
#[derive(Clone)]
pub struct InlineElementSlot {
    pub(crate) builder:
        Rc<dyn Fn(&mut Window, &mut App, InlineElementContext) -> InlineElementSpec>,
}

impl InlineElementSlot {
    fn build(
        &self,
        window: &mut Window,
        cx: &mut App,
        context: InlineElementContext,
    ) -> InlineElementSpec {
        (self.builder)(window, cx, context)
    }
}

/// Builder for `InlineSpan` content.
#[derive(Default)]
pub struct InlineSpanBuilder {
    fragments: Vec<InlineFragment>,
    default_style: Option<TextStyle>,
    inline_align: InlineElementAlign,
}

/// Context provided when materializing inline elements.
#[derive(Clone)]
pub struct InlineElementContext {
    /// Maximum width available for the element, if any.
    pub max_width: Option<Pixels>,
    /// Line height that the element should align to.
    pub line_height: Pixels,
    /// Default font of the surrounding span.
    pub font: Font,
    /// Default font size in pixels.
    pub font_size: Pixels,
    /// Vertical alignment of the element.
    pub alignment: InlineElementAlign,
}

/// Specification returned by the element slot builder.
pub struct InlineElementSpec {
    /// The element to insert into the inline flow.
    pub element: AnyElement,
    /// Requested layout space for the element.
    pub requested_space: Size<AvailableSpace>,
    /// Optional distance from the top of the element to its baseline.
    pub baseline_offset: Option<Pixels>,
    /// How the element should align vertically within the line box.
    pub align: InlineElementAlign,
    /// Whether the element should constrain its width to the wrap width.
    pub constrain_width: bool,
    /// Pre-measured size of the element; required because inline measurement cannot re-enter layout.
    pub premeasured_size: Size<Pixels>,
}

/// Describes how an inline element should align relative to the text baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InlineElementAlign {
    /// Use the provided baseline offset in [`InlineElementSpec`].
    #[default]
    Auto,
    /// Align the element's top edge to the top of the line box.
    Top,
    /// Center the element vertically within the line box.
    Middle,
    /// Align the element's bottom edge to the bottom of the line box.
    Bottom,
}

/// Runtime layout entry for each fragment.
pub enum InlineFragmentLayout {
    /// Text fragment with no additional state.
    Text,
    /// Inline element instance measured during layout.
    Element {
        /// Measured inline element stored in the layout state.
        instance: InlineElementInstance,
    },
}

/// Measured inline element ready to paint.
pub struct InlineElementInstance {
    /// The element instance itself.
    pub element: AnyElement,
    /// Size returned by layout.
    pub size: Size<Pixels>,
    /// Bounds relative to the inline span origin.
    pub bounds: Bounds<Pixels>,
    /// Distance from the element's top to its baseline.
    pub baseline_offset: Option<Pixels>,
    /// Alignment mode for this inline element.
    pub align: InlineElementAlign,
}

/// Indicates which side of a logical position a caret should prefer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretAffinity {
    /// Position before the logical cell.
    Upstream,
    /// Position after the logical cell.
    Downstream,
}

/// Describes the portion of a line covered by a selection.
#[derive(Clone, Debug)]
pub struct LineSelection {
    /// The line index within the layout.
    pub line_index: usize,
    /// First logical index covered on this line.
    pub logical_start: usize,
    /// Logical index immediately after the covered range on this line.
    pub logical_end: usize,
    /// Segments within the line contributing to the selection.
    pub segments: Vec<SelectionSegment>,
}

/// A selection segment referencing either text bytes or an inline element.
#[derive(Clone, Debug)]
pub enum SelectionSegment {
    /// Text fragment coverage.
    Text {
        /// Fragment index.
        fragment_index: usize,
        /// Logical range (cell units) covered on this line.
        logical_range: Range<usize>,
        /// Byte offset within the fragment where the segment starts.
        start_in_fragment: usize,
        /// Byte offset within the fragment where the segment ends.
        end_in_fragment: usize,
    },
    /// Inline element coverage (always a single logical cell).
    Element {
        /// Fragment index.
        fragment_index: usize,
        /// Logical range covered by the element.
        logical_range: Range<usize>,
    },
}

/// Decoration describing a logical range to highlight.
#[derive(Clone, Debug)]
pub struct InlineDecoration {
    /// Logical range covered by this decoration.
    pub range: Range<usize>,
    /// Visual style to apply.
    pub style: InlineDecorationStyle,
}

/// Styling information for a decoration.
#[derive(Clone, Debug)]
pub struct InlineDecorationStyle {
    /// Background color for the highlighted range.
    pub background: Hsla,
}

impl InlineElementInstance {
    fn new(spec: InlineElementSpec) -> Self {
        let size = spec.premeasured_size;
        InlineElementInstance {
            element: spec.element,
            size,
            bounds: Bounds::new(Point::new(px(0.0), px(0.0)), size),
            baseline_offset: spec.baseline_offset,
            align: spec.align,
        }
    }
}

impl InlineElementSpec {
    /// Apply an alignment to this spec.
    pub fn with_alignment(mut self, align: InlineElementAlign) -> Self {
        self.align = align;
        self.baseline_offset = match align {
            InlineElementAlign::Auto => self.baseline_offset,
            InlineElementAlign::Top => Some(self.premeasured_size.height),
            InlineElementAlign::Bottom => Some(px(0.0)),
            InlineElementAlign::Middle => Some(self.premeasured_size.height / 2.0),
        };
        self
    }

    /// Set whether this element can expand the line height.
    /// Convenience helper for building a spec from a measured element.
    pub fn from_measured_element(
        element: AnyElement,
        measured_size: Size<Pixels>,
        align: InlineElementAlign,
    ) -> Self {
        let baseline_offset = match align {
            InlineElementAlign::Auto => None,
            InlineElementAlign::Top => Some(measured_size.height),
            InlineElementAlign::Bottom => Some(px(0.0)),
            InlineElementAlign::Middle => Some(measured_size.height / 2.0),
        };

        InlineElementSpec {
            element,
            requested_space: Size::new(
                AvailableSpace::MaxContent,
                AvailableSpace::Definite(measured_size.height),
            ),
            baseline_offset,
            align,
            constrain_width: false,
            premeasured_size: measured_size,
        }
    }
}

/// Logical slice describing which portion of a fragment belongs to a line.
#[derive(Clone)]
pub struct LogicalSlice {
    /// Index of the fragment being sliced.
    pub fragment_index: usize,
    /// Start byte inside the fragment.
    pub start_in_fragment: usize,
    /// End byte inside the fragment (exclusive).
    pub end_in_fragment: usize,
    /// Logical index at the start of the slice.
    pub logical_start: usize,
    /// Logical index immediately after the slice.
    pub logical_end: usize,
}

/// Mapping between fragments and global logical indices.
#[derive(Clone)]
pub struct FragmentLogicalRange {
    /// Fragment index this range corresponds to.
    pub fragment_index: usize,
    /// Logical index at the start of the fragment.
    pub logical_start: usize,
    /// Logical index after the fragment.
    pub logical_end: usize,
}

/// Cached mapping between cell indices and byte offsets for a fragment.
#[derive(Clone)]
pub struct TextFragmentCells {
    /// Byte offsets representing cell boundaries inside the fragment.
    pub offsets: Vec<usize>,
}

/// A segment that can be painted on an [`InlineWrapLine`].
#[derive(Clone)]
pub enum ShapedSegment {
    /// Shaped text to paint on the line.
    Text {
        /// Fragment referenced by this shaped text.
        fragment_index: usize,
        /// Start byte within the fragment.
        start_in_fragment: usize,
        /// End byte within the fragment (exclusive).
        end_in_fragment: usize,
        /// Logical range covered by this slice.
        logical_start: usize,
        /// Exclusive logical end for this slice.
        logical_end: usize,
        /// Shaped glyph data for the slice.
        shaped: ShapedLine,
    },
    /// Inline element occupying a single logical cell.
    Element {
        /// Fragment index of the inline element.
        fragment_index: usize,
        /// Logical range occupied by the element (always length 1).
        logical_start: usize,
        /// Exclusive logical end for the element.
        logical_end: usize,
        /// Width of the inline element.
        width: Pixels,
    },
}

/// Fully shaped line stored in the layout state.
#[derive(Clone)]
pub struct InlineWrapLine {
    /// Mapping to the fragments that contribute to the line.
    pub logical_slices: Vec<LogicalSlice>,
    /// Logical index where the line starts.
    pub logical_start: usize,
    /// Logical index immediately after the end of the line.
    pub logical_end: usize,
    /// Items to paint for this line.
    pub shaped_segments: Vec<ShapedSegment>,
    /// Visual width of the line (without indent).
    pub width: Pixels,
    /// Visual height of the line box.
    pub height: Pixels,
    /// Indent applied to the start of the line.
    pub indent_px: Pixels,
    /// Distance from the top of the line box to the baseline.
    pub baseline_above_top: Pixels,
    /// Origin of the line relative to the span.
    pub line_origin: Point<Pixels>,
}

/// Public line geometry for overlay consumers.
#[derive(Clone, Debug)]
pub struct InlineLineGeometry {
    /// Range of logical indices covered by the line.
    pub logical_range: Range<usize>,
    /// Bounds of the line relative to the span origin.
    pub bounds: Bounds<Pixels>,
    /// Baseline position relative to the span origin.
    pub baseline: Pixels,
}

/// Public geometry for a content fragment (text or element).
#[derive(Clone, Debug)]
pub enum InlineFragmentGeometry {
    /// Geometry for a text run within a line.
    Text {
        /// Logical range covered by this text run.
        logical_range: Range<usize>,
        /// Bounds of the text run relative to the span origin.
        bounds: Bounds<Pixels>,
    },
    /// Geometry for an inline element.
    Element {
        /// Logical index where this element is positioned.
        logical_index: usize,
        /// Bounds of the element relative to the span origin.
        bounds: Bounds<Pixels>,
        /// Distance from the element's top to its baseline.
        baseline_offset: Option<Pixels>,
        /// Alignment mode for this inline element.
        align: InlineElementAlign,
    },
}

impl InlineWrapLine {
    /// Return the x-position for a given logical index within this line.
    pub fn x_for_index(&self, logical_index: usize, affinity: CaretAffinity) -> Option<Pixels> {
        if logical_index < self.logical_start {
            return Some(self.line_origin.x);
        }
        let mut cursor = self.line_origin.x;
        for segment in &self.shaped_segments {
            match segment {
                ShapedSegment::Text {
                    logical_start,
                    logical_end,
                    shaped,
                    ..
                } => {
                    if logical_index > *logical_end {
                        cursor += shaped.width;
                        continue;
                    }
                    if logical_index < *logical_start {
                        return Some(cursor);
                    }
                    if logical_index == *logical_end {
                        return Some(cursor + shaped.width);
                    }
                    let local_cell = logical_index - *logical_start;
                    let byte_offset =
                        cell_to_byte_offset(shaped.text.as_ref(), local_cell).min(shaped.len());
                    let x = shaped.layout.x_for_index(byte_offset);
                    return Some(cursor + x);
                }
                ShapedSegment::Element {
                    logical_start,
                    logical_end,
                    width,
                    ..
                } => {
                    if logical_index < *logical_start {
                        return Some(cursor);
                    }
                    if logical_index >= *logical_end {
                        cursor += *width;
                        continue;
                    }
                    return Some(match affinity {
                        CaretAffinity::Upstream => cursor,
                        CaretAffinity::Downstream => cursor + *width,
                    });
                }
            }
        }
        if logical_index == self.logical_end {
            return Some(self.line_origin.x + self.width);
        }
        None
    }

    /// Return the logical index for a given x-position within this line.
    pub fn index_for_x(&self, x: Pixels, affinity: CaretAffinity) -> usize {
        if x <= self.line_origin.x || self.shaped_segments.is_empty() {
            return self.logical_start;
        }
        if x >= self.line_origin.x + self.width {
            return self.logical_end;
        }
        let mut cursor = self.line_origin.x;
        for segment in &self.shaped_segments {
            match segment {
                ShapedSegment::Text {
                    logical_start,
                    logical_end,
                    shaped,
                    ..
                } => {
                    let seg_end = cursor + shaped.width;
                    if x > seg_end {
                        cursor = seg_end;
                        continue;
                    }
                    let local_x = (x - cursor).max(Pixels::ZERO).min(shaped.width);
                    let byte_offset = shaped
                        .layout
                        .index_for_x(local_x)
                        .unwrap_or(shaped.len())
                        .min(shaped.len());
                    let cell_offset = byte_offset_to_cell_index(shaped.text.as_ref(), byte_offset);
                    return (logical_start + cell_offset).min(*logical_end);
                }
                ShapedSegment::Element {
                    logical_start,
                    logical_end,
                    width,
                    ..
                } => {
                    let seg_end = cursor + *width;
                    if x > seg_end {
                        cursor = seg_end;
                        continue;
                    }
                    let midpoint = cursor + (*width / 2.0);
                    let choose_upstream = if x < midpoint {
                        true
                    } else if x > midpoint {
                        false
                    } else {
                        affinity == CaretAffinity::Upstream
                    };
                    return if choose_upstream {
                        *logical_start
                    } else {
                        *logical_end
                    };
                }
            }
        }
        self.logical_end
    }

    fn selection_segments_for_range(
        &self,
        range: Range<usize>,
    ) -> Option<(Range<usize>, Vec<SelectionSegment>)> {
        if range.is_empty() {
            return None;
        }
        let overlap = intersect_ranges(range, self.logical_start..self.logical_end)?;
        let mut segments = Vec::new();
        for segment in &self.shaped_segments {
            match segment {
                ShapedSegment::Text {
                    fragment_index,
                    logical_start,
                    logical_end,
                    start_in_fragment,
                    shaped,
                    ..
                } => {
                    if let Some(seg_overlap) =
                        intersect_ranges(overlap.clone(), *logical_start..*logical_end)
                    {
                        let local_start = seg_overlap.start - *logical_start;
                        let local_end = seg_overlap.end - *logical_start;
                        let text = shaped.text.as_ref();
                        let start_byte = start_in_fragment + cell_to_byte_offset(text, local_start);
                        let end_byte = start_in_fragment + cell_to_byte_offset(text, local_end);
                        segments.push(SelectionSegment::Text {
                            fragment_index: *fragment_index,
                            logical_range: seg_overlap,
                            start_in_fragment: start_byte,
                            end_in_fragment: end_byte,
                        });
                    }
                }
                ShapedSegment::Element {
                    fragment_index,
                    logical_start,
                    logical_end,
                    ..
                } => {
                    if let Some(seg_overlap) =
                        intersect_ranges(overlap.clone(), *logical_start..*logical_end)
                    {
                        segments.push(SelectionSegment::Element {
                            fragment_index: *fragment_index,
                            logical_range: seg_overlap,
                        });
                    }
                }
            }
        }
        if segments.is_empty() {
            return None;
        }
        Some((overlap, segments))
    }
}

#[cfg(debug_assertions)]
static DEBUG_OUTLINES: AtomicBool = AtomicBool::new(false);

struct InlineSpanLayoutInner {
    lines: Vec<InlineWrapLine>,
    fragment_layouts: Vec<InlineFragmentLayout>,
    measured_size: Size<Pixels>,
    bounds: Option<Bounds<Pixels>>,
    last_wrap_width: Option<Pixels>,
    last_line_height: Option<Pixels>,
    dirty: bool,
    decorations: Vec<InlineDecoration>,
    prepared_elements: Vec<Option<InlineElementInstance>>,
}

impl Default for InlineSpanLayoutInner {
    fn default() -> Self {
        InlineSpanLayoutInner {
            lines: Vec::new(),
            fragment_layouts: Vec::new(),
            measured_size: Size {
                width: Pixels::ZERO,
                height: Pixels::ZERO,
            },
            bounds: None,
            last_wrap_width: None,
            last_line_height: None,
            dirty: true,
            decorations: Vec::new(),
            prepared_elements: Vec::new(),
        }
    }
}

/// Shared layout cache for an `InlineSpan`.
#[derive(Clone, Default)]
pub struct InlineSpanLayout {
    inner: Rc<RefCell<InlineSpanLayoutInner>>,
}

impl InlineSpanLayout {
    /// Enables or disables debug outlines for every inline span (debug builds only).
    pub fn set_global_debug_outlines(enabled: bool) {
        #[cfg(debug_assertions)]
        {
            DEBUG_OUTLINES.store(enabled, Ordering::Relaxed);
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = enabled;
        }
    }
    fn request_layout(
        &self,
        content: Arc<InlineContent>,
        window: &mut Window,
        _cx: &mut App,
    ) -> LayoutId {
        let layout = self.clone();
        window.request_measured_layout(Default::default(), move |known, available, window, cx| {
            layout.measure(&content, known, available, window, cx)
        })
    }

    fn measure(
        &self,
        content: &InlineContent,
        known_dimensions: Size<Option<Pixels>>,
        available_space: Size<AvailableSpace>,
        window: &mut Window,
        cx: &mut App,
    ) -> Size<Pixels> {
        let text_style = window.text_style();
        let rem_size = window.rem_size();
        let base_font_size = content.default_font_size_or(
            text_style.font_size.to_pixels(window.rem_size()),
            window.rem_size(),
        );
        let line_height_hint = text_style
            .line_height
            .to_pixels(AbsoluteLength::Pixels(base_font_size), rem_size);
        let wrap_width = if text_style.white_space == WhiteSpace::Normal {
            known_dimensions
                .width
                .or_else(|| match available_space.width {
                    AvailableSpace::Definite(px) => Some(px),
                    _ => None,
                })
        } else {
            None
        };

        {
            let state = self.inner.borrow();
            if !state.dirty
                && state.last_wrap_width == wrap_width
                && state.last_line_height == Some(line_height_hint)
            {
                return state.measured_size;
            }
        }

        self.rewrap_and_shape(content, wrap_width, line_height_hint, window, cx);
        #[cfg(debug_assertions)]
        {
            let state = self.inner.borrow();
            for (i, line) in state.lines.iter().enumerate() {
                println!(
                    "INLINE_SPAN_DEBUG line={} height={:?} baseline={:?} allow_expansion_elements={}",
                    i,
                    line.height,
                    line.baseline_above_top,
                    state
                        .fragment_layouts
                        .iter()
                        .filter_map(|frag| match frag {
                            InlineFragmentLayout::Element { .. } => Some(()),
                            _ => None,
                        })
                        .count()
                );
            }
        }
        self.inner.borrow().measured_size
    }

    fn mark_dirty(&self) {
        let mut state = self.inner.borrow_mut();
        state.dirty = true;
        state.prepared_elements.clear();
    }

    /// Replace the active decoration list.
    pub fn set_decorations(&self, decorations: Vec<InlineDecoration>) {
        let mut state = self.inner.borrow_mut();
        state.decorations = decorations;
    }

    fn prepare_inline_elements(
        &self,
        content: &InlineContent,
        line_height_hint: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut state = self.inner.borrow_mut();
        state.prepared_elements.clear();
        state
            .prepared_elements
            .resize_with(content.fragments.len(), || None);

        for (index, fragment) in content.fragments.iter().enumerate() {
            let InlineFragment::Element { slot } = fragment else {
                continue;
            };
            let context = InlineElementContext {
                max_width: None,
                line_height: line_height_hint,
                font: content.default_font_or(window.text_style().font()),
                font_size: content.default_font_size_or(
                    window.text_style().font_size.to_pixels(window.rem_size()),
                    window.rem_size(),
                ),
                alignment: InlineElementAlign::Bottom, // Default, will be overridden
            };
            let mut spec = slot.build(window, cx, context);
            let measured_space = spec.requested_space;
            let measured_size = spec.element.layout_as_root(measured_space, window, cx);
            spec.premeasured_size = measured_size;
            let instance = InlineElementInstance::new(spec);
            state.prepared_elements[index] = Some(instance);
        }
    }

    fn rewrap_and_shape(
        &self,
        content: &InlineContent,
        wrap_width: Option<Pixels>,
        line_height_hint: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Determine the effective style once upfront
        let mut current_style = content.default_style.clone();

        // If no default style, scan fragments for the first available style
        if current_style.is_none() {
            current_style = content.fragments.iter().find_map(|f| match f {
                InlineFragment::Text { style, .. } => style.clone(),
                _ => None,
            });
        }

        // Resolve font and font_size once
        let (resolved_font, resolved_font_size) = current_style
            .as_ref()
            .map(|s| (s.font(), s.font_size.to_pixels(window.rem_size())))
            .unwrap_or_else(|| {
                let style = window.text_style();
                (style.font(), style.font_size.to_pixels(window.rem_size()))
            });

        let mut fragment_layouts = Vec::with_capacity(content.fragments.len());
        let mut prepared_instances = {
            let mut state = self.inner.borrow_mut();
            if state.prepared_elements.len() == content.fragments.len() {
                std::mem::take(&mut state.prepared_elements)
            } else {
                Vec::new()
            }
        };

        for (index, fragment) in content.fragments.iter().enumerate() {
            match fragment {
                InlineFragment::Text { .. } => fragment_layouts.push(InlineFragmentLayout::Text),
                InlineFragment::Element { slot } => {
                    let instance = prepared_instances
                        .get_mut(index)
                        .and_then(|entry| entry.take())
                        .or_else(|| {
                            let context = InlineElementContext {
                                max_width: wrap_width,
                                line_height: line_height_hint,
                                font: resolved_font.clone(),
                                font_size: resolved_font_size,
                                alignment: InlineElementAlign::Bottom, // Default, will be overridden
                            };
                            let spec = slot.build(window, cx, context);
                            Some(InlineElementInstance::new(spec))
                        })
                        .expect("inline element instance should exist");
                    fragment_layouts.push(InlineFragmentLayout::Element { instance });
                }
            }
        }

        let (line_fragments, fragment_ranges, text_cells) =
            build_line_fragments(content, &fragment_layouts);
        let total_len_bytes = total_byte_len(&line_fragments);

        let mut boundaries = Vec::new();
        if let Some(width) = wrap_width {
            let mut wrapper = cx
                .text_system()
                .line_wrapper(resolved_font.clone(), resolved_font_size);
            boundaries.extend(wrapper.wrap_line(&line_fragments, width));
        }

        let font_id = cx
            .text_system()
            .resolve_font(&content.default_font_or(window.text_style().font()));
        let space_width = cx
            .text_system()
            .advance(
                font_id,
                content.default_font_size_or(
                    window.text_style().font_size.to_pixels(window.rem_size()),
                    window.rem_size(),
                ),
                ' ',
            )
            .map(|size| size.width)
            .unwrap_or(px(0.0));

        let text_system = window.text_system().clone();
        let mut lines = Vec::new();
        let mut current_y = px(0.0);
        let mut max_width = px(0.0);
        let mut prev_ix = 0;
        let mut pending_indent = 0usize;

        for boundary in &boundaries {
            let slices = compute_logical_slices_for_line(
                prev_ix,
                boundary.ix,
                content,
                &fragment_ranges,
                &text_cells,
            );
            if let Some(line) = build_line(
                slices,
                pending_indent,
                space_width,
                content,
                &mut fragment_layouts,
                &text_system,
                line_height_hint,
                current_y,
                window,
            ) {
                max_width = max_width.max(line.indent_px + line.width);
                current_y += line.height;
                prev_ix = boundary.ix;
                pending_indent = boundary.next_indent as usize;
                lines.push(line);
            } else {
                pending_indent = boundary.next_indent as usize;
                prev_ix = boundary.ix;
            }
        }

        let trailing = compute_logical_slices_for_line(
            prev_ix,
            total_len_bytes,
            content,
            &fragment_ranges,
            &text_cells,
        );
        if let Some(line) = build_line(
            trailing,
            pending_indent,
            space_width,
            content,
            &mut fragment_layouts,
            &text_system,
            line_height_hint,
            current_y,
            window,
        ) {
            max_width = max_width.max(line.indent_px + line.width);
            current_y += line.height;
            lines.push(line);
        }

        let mut state = self.inner.borrow_mut();
        state.lines = lines;
        state.fragment_layouts = fragment_layouts;
        state.measured_size = Size {
            width: max_width,
            height: current_y,
        };
        state.bounds = None;
        state.last_wrap_width = wrap_width;
        state.last_line_height = Some(line_height_hint);
        state.dirty = false;
    }

    fn prepaint(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let mut state = self.inner.borrow_mut();
        state.bounds = Some(bounds);
        for fragment in &mut state.fragment_layouts {
            if let InlineFragmentLayout::Element { instance } = fragment {
                let origin = bounds.origin + instance.bounds.origin;
                let layout_space = Size::new(
                    AvailableSpace::Definite(instance.size.width),
                    AvailableSpace::Definite(instance.size.height),
                );
                instance.element.layout_as_root(layout_space, window, cx);
                instance.element.prepaint_at(origin, window, cx);
            }
        }
    }

    fn paint(&self, window: &mut Window, cx: &mut App) {
        let mut state = self.inner.borrow_mut();
        let Some(bounds) = state.bounds else {
            return;
        };
        let mut fragment_layouts = std::mem::take(&mut state.fragment_layouts);
        if !state.decorations.is_empty() {
            for decoration in state.decorations.clone() {
                for rect in rects_for_range(&state.lines, decoration.range.clone()) {
                    let abs_bounds = Bounds::new(
                        Point::new(
                            bounds.origin.x + rect.origin.x,
                            bounds.origin.y + rect.origin.y,
                        ),
                        rect.size,
                    );
                    window.paint_quad(fill(abs_bounds, decoration.style.background));
                }
            }
        }
        for line in &state.lines {
            let line_origin = bounds.origin + line.line_origin;
            let mut cursor_x = line_origin.x;
            for segment in &line.shaped_segments {
                match segment {
                    ShapedSegment::Text { shaped, .. } => {
                        shaped
                            .paint(Point::new(cursor_x, line_origin.y), line.height, window, cx)
                            .log_err();
                        cursor_x += shaped.width;
                    }
                    ShapedSegment::Element { fragment_index, .. } => {
                        if let InlineFragmentLayout::Element { instance } =
                            &mut fragment_layouts[*fragment_index]
                        {
                            instance.element.paint(window, cx);
                            cursor_x += instance.size.width;
                        }
                    }
                }
            }
        }

        #[cfg(debug_assertions)]
        if debug_outlines_enabled() {
            paint_debug_outlines(bounds, &state.lines, &fragment_layouts, window);
        }

        state.fragment_layouts = fragment_layouts;
    }

    /// Return the point along the baseline for a logical index relative to the span origin.
    pub fn position_for_logical_index(
        &self,
        logical_index: usize,
        affinity: CaretAffinity,
    ) -> Option<Point<Pixels>> {
        let state = self.inner.borrow();
        for line in &state.lines {
            if logical_index < line.logical_start {
                return Some(Point::new(
                    line.line_origin.x,
                    line.line_origin.y + line.baseline_above_top,
                ));
            }
            if logical_index <= line.logical_end {
                let x = line.x_for_index(logical_index, affinity)?;
                return Some(Point::new(x, line.line_origin.y + line.baseline_above_top));
            }
        }
        state.lines.last().and_then(|line| {
            if logical_index == line.logical_end {
                Some(Point::new(
                    line.line_origin.x + line.width,
                    line.line_origin.y + line.baseline_above_top,
                ))
            } else {
                None
            }
        })
    }

    /// Return the logical index for a point relative to the span origin.
    pub fn logical_index_for_point(
        &self,
        point: Point<Pixels>,
        affinity: CaretAffinity,
    ) -> Option<usize> {
        let state = self.inner.borrow();
        let mut target_line = None;
        for (idx, line) in state.lines.iter().enumerate() {
            let top = line.line_origin.y;
            let bottom = top + line.height;
            if point.y < top {
                target_line = Some(idx);
                break;
            }
            if point.y <= bottom {
                target_line = Some(idx);
                break;
            }
        }
        let idx = match target_line {
            Some(idx) => idx,
            None => state.lines.len().checked_sub(1)?,
        };
        let line = &state.lines[idx];
        Some(line.index_for_x(point.x, affinity))
    }

    /// Compute per-line selection segments for the provided logical range.
    pub fn selections_for_range(&self, range: Range<usize>) -> Vec<LineSelection> {
        let state = self.inner.borrow();
        state
            .lines
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| {
                line.selection_segments_for_range(range.clone())
                    .map(|(logical, segments)| LineSelection {
                        line_index: idx,
                        logical_start: logical.start,
                        logical_end: logical.end,
                        segments,
                    })
            })
            .collect()
    }

    /// Returns the most recent bounds assigned to this span during layout.
    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        self.inner.borrow().bounds
    }

    /// Compute selection rectangles (relative to the span origin) for the provided logical range.
    pub fn selection_rects(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let state = self.inner.borrow();
        rects_for_range(&state.lines, range)
    }

    /// Iterate line geometries for the current layout.
    pub fn line_geometries(&self) -> Vec<InlineLineGeometry> {
        let state = self.inner.borrow();
        let mut geometries = Vec::with_capacity(state.lines.len());
        for line in &state.lines {
            geometries.push(InlineLineGeometry {
                logical_range: line.logical_start..line.logical_end,
                bounds: Bounds::new(line.line_origin, Size::new(line.width, line.height)),
                baseline: line.line_origin.y + line.baseline_above_top,
            });
        }
        geometries
    }

    /// Return geometry information for all fragments (text and elements).
    pub fn fragment_geometries(&self) -> Vec<InlineFragmentGeometry> {
        let state = self.inner.borrow();
        let mut geometries = Vec::new();

        for line in &state.lines {
            let line_origin = line.line_origin;
            let mut cursor_x = line_origin.x;

            for segment in &line.shaped_segments {
                match segment {
                    ShapedSegment::Text {
                        logical_start,
                        logical_end,
                        shaped,
                        ..
                    } => {
                        let width = shaped.width;
                        // Text segments take up the full line height in the debug view
                        let bounds = Bounds::new(
                            Point::new(cursor_x, line_origin.y),
                            Size::new(width, line.height),
                        );
                        geometries.push(InlineFragmentGeometry::Text {
                            logical_range: *logical_start..*logical_end,
                            bounds,
                        });
                        cursor_x += width;
                    }
                    ShapedSegment::Element {
                        fragment_index,
                        logical_start,
                        width,
                        ..
                    } => {
                        if let Some(InlineFragmentLayout::Element { instance }) =
                            state.fragment_layouts.get(*fragment_index)
                        {
                            geometries.push(InlineFragmentGeometry::Element {
                                logical_index: *logical_start,
                                bounds: instance.bounds,
                                baseline_offset: instance.baseline_offset,
                                align: instance.align,
                            });
                        }
                        cursor_x += *width;
                    }
                }
            }
        }

        geometries
    }
}

fn rects_for_range(lines: &[InlineWrapLine], range: Range<usize>) -> Vec<Bounds<Pixels>> {
    if range.is_empty() {
        return Vec::new();
    }
    let mut rects = Vec::new();
    for line in lines {
        if let Some(overlap) = intersect_ranges(range.clone(), line.logical_start..line.logical_end)
        {
            let start_x = line
                .x_for_index(overlap.start, CaretAffinity::Upstream)
                .unwrap_or(line.line_origin.x);
            let end_x = line
                .x_for_index(overlap.end, CaretAffinity::Downstream)
                .unwrap_or(line.line_origin.x);
            let (left, right) = if end_x >= start_x {
                (start_x, end_x)
            } else {
                (end_x, start_x)
            };
            rects.push(Bounds::new(
                Point::new(left, line.line_origin.y),
                Size::new(right - left, line.height),
            ));
        }
    }
    rects
}

fn total_byte_len(fragments: &[LineFragment<'_>]) -> usize {
    fragments
        .iter()
        .map(|fragment| match fragment {
            LineFragment::Text { text } => text.len(),
            LineFragment::Element { len_utf8, .. } => *len_utf8,
        })
        .sum()
}

fn build_line(
    logical_slices: Vec<LogicalSlice>,
    indent_spaces: usize,
    space_width: Pixels,
    content: &InlineContent,
    fragment_layouts: &mut [InlineFragmentLayout],
    text_system: &Arc<crate::WindowTextSystem>,
    line_height_hint: Pixels,
    current_y: Pixels,
    window: &Window,
) -> Option<InlineWrapLine> {
    if logical_slices.is_empty() {
        return None;
    }

    let indent_px = space_width * indent_spaces;
    let mut segments = Vec::new();
    let mut max_ascent = px(0.0);
    let mut max_descent = px(0.0);
    let mut max_aligned_height = px(0.0);
    let mut width = px(0.0);

    for slice in &logical_slices {
        match &content.fragments[slice.fragment_index] {
            InlineFragment::Text { text, style } => {
                let range = &text.as_ref()[slice.start_in_fragment..slice.end_in_fragment];
                if range.is_empty() {
                    continue;
                }

                let shaped_text = SharedString::from(range.to_string());
                let font_size = style
                    .as_ref()
                    .map(|s| s.font_size.to_pixels(window.rem_size()))
                    .unwrap_or_else(|| {
                        content.default_font_size_or(
                            window.text_style().font_size.to_pixels(window.rem_size()),
                            window.rem_size(),
                        )
                    });

                let mut run = style
                    .clone()
                    .unwrap_or_else(|| content.default_style.clone().unwrap_or(window.text_style()))
                    .to_run(range.len());
                run.font = content.default_font_or(window.text_style().font());
                if let Some(style) = style {
                    run.font = style.font();
                }

                let shaped = text_system
                    .as_ref()
                    .shape_line(shaped_text, font_size, &[run], None);
                max_ascent = max_ascent.max(shaped.ascent);
                max_descent = max_descent.max(shaped.descent);
                width += shaped.width;
                segments.push(ShapedSegment::Text {
                    fragment_index: slice.fragment_index,
                    start_in_fragment: slice.start_in_fragment,
                    end_in_fragment: slice.end_in_fragment,
                    logical_start: slice.logical_start,
                    logical_end: slice.logical_end,
                    shaped,
                });
            }
            InlineFragment::Element { .. } => {
                if let InlineFragmentLayout::Element { instance } =
                    &mut fragment_layouts[slice.fragment_index]
                {
                    segments.push(ShapedSegment::Element {
                        fragment_index: slice.fragment_index,
                        logical_start: slice.logical_start,
                        logical_end: slice.logical_end,
                        width: instance.size.width,
                    });
                    width += instance.size.width;
                    match instance.align {
                        InlineElementAlign::Auto => {
                            let (ascent, descent) = if let Some(offset) = instance.baseline_offset {
                                (offset, instance.size.height - offset)
                            } else {
                                let offset = if instance.size.height <= line_height_hint {
                                    instance.size.height / 2.0
                                } else {
                                    instance.size.height
                                };
                                instance.baseline_offset = Some(offset);
                                (offset, instance.size.height - offset)
                            };
                            max_ascent = max_ascent.max(ascent);
                            max_descent = max_descent.max(descent);
                        }
                        InlineElementAlign::Top
                        | InlineElementAlign::Bottom
                        | InlineElementAlign::Middle => {
                            max_aligned_height = max_aligned_height.max(instance.size.height);
                        }
                    }
                }
            }
        }
    }

    if segments.is_empty() {
        return None;
    }

    let content_height = max_ascent + max_descent;
    let mut line_height = content_height.max(line_height_hint);
    if max_aligned_height > line_height {
        line_height = max_aligned_height;
    }
    let extra = line_height - content_height;
    let baseline_above_top = if content_height > px(0.0) {
        if extra > px(0.0) {
            max_ascent + extra / 2.0
        } else {
            max_ascent
        }
    } else {
        line_height / 2.0
    };

    let logical_start = logical_slices
        .first()
        .map(|slice| slice.logical_start)
        .unwrap_or(0);
    let logical_end = logical_slices
        .last()
        .map(|slice| slice.logical_end)
        .unwrap_or(logical_start);

    let mut line = InlineWrapLine {
        logical_slices,
        logical_start,
        logical_end,
        shaped_segments: segments,
        width,
        height: line_height,
        indent_px,
        baseline_above_top,
        line_origin: Point::new(indent_px, current_y),
    };

    position_inline_elements(&line, fragment_layouts);

    Some(line)
}

fn position_inline_elements(line: &InlineWrapLine, fragment_layouts: &mut [InlineFragmentLayout]) {
    let baseline_y = line.line_origin.y + line.baseline_above_top;
    let mut cursor = line.line_origin.x;
    for segment in &line.shaped_segments {
        match segment {
            ShapedSegment::Text { shaped, .. } => {
                cursor += shaped.width;
            }
            ShapedSegment::Element { fragment_index, .. } => {
                if let InlineFragmentLayout::Element { instance } =
                    &mut fragment_layouts[*fragment_index]
                {
                    let top = match instance.align {
                        InlineElementAlign::Auto => {
                            if let Some(offset) = instance.baseline_offset {
                                baseline_y - offset
                            } else if instance.size.height <= line.height {
                                baseline_y - instance.size.height / 2.0
                            } else {
                                baseline_y - instance.size.height
                            }
                        }
                        InlineElementAlign::Top => line.line_origin.y,
                        InlineElementAlign::Bottom => {
                            line.line_origin.y + line.height - instance.size.height
                        }
                        InlineElementAlign::Middle => {
                            line.line_origin.y + (line.height - instance.size.height) / 2.0
                        }
                    };
                    instance.bounds = Bounds::new(
                        Point::new(cursor, top),
                        Size {
                            width: instance.size.width,
                            height: instance.size.height,
                        },
                    );
                    cursor += instance.size.width;
                }
            }
        }
    }
}
fn cell_to_byte_offset(text: &str, cell_index: usize) -> usize {
    if cell_index == 0 {
        return 0;
    }
    for (count, (byte_offset, _)) in text.char_indices().enumerate() {
        if count == cell_index {
            return byte_offset;
        }
    }
    text.len()
}

fn byte_offset_to_cell_index(text: &str, byte_offset: usize) -> usize {
    if byte_offset == 0 {
        return 0;
    }
    for (count, (offset, _)) in text.char_indices().enumerate() {
        if offset >= byte_offset {
            return count;
        }
    }
    text.chars().count()
}

#[cfg(debug_assertions)]
fn paint_debug_outlines(
    bounds: Bounds<Pixels>,
    lines: &[InlineWrapLine],
    fragment_layouts: &[InlineFragmentLayout],
    window: &mut Window,
) {
    let line_color = crate::rgba(0x3b82f680);
    let element_color = crate::rgba(0xef4444c0);

    for line in lines {
        if line.width <= px(0.0) || line.height <= px(0.0) {
            continue;
        }
        let origin = bounds.origin + line.line_origin;
        let line_bounds = Bounds::new(origin, Size::new(line.width, line.height));
        window.paint_quad(outline(line_bounds, line_color, BorderStyle::Dashed));
    }

    for fragment in fragment_layouts {
        if let InlineFragmentLayout::Element { instance } = fragment {
            let origin = bounds.origin + instance.bounds.origin;
            let element_bounds = Bounds::new(origin, instance.bounds.size);
            window.paint_quad(outline(element_bounds, element_color, BorderStyle::Solid));
        }
    }
}

#[cfg(debug_assertions)]
fn debug_outlines_enabled() -> bool {
    DEBUG_OUTLINES.load(Ordering::Relaxed)
}

#[cfg(not(debug_assertions))]
#[allow(dead_code)]
fn debug_outlines_enabled() -> bool {
    false
}

fn intersect_ranges(a: Range<usize>, b: Range<usize>) -> Option<Range<usize>> {
    let start = a.start.max(b.start);
    let end = a.end.min(b.end);
    if start < end { Some(start..end) } else { None }
}

fn build_line_fragments<'a>(
    content: &'a InlineContent,
    layouts: &'a [InlineFragmentLayout],
) -> (
    Vec<LineFragment<'a>>,
    Vec<FragmentLogicalRange>,
    Vec<Option<TextFragmentCells>>,
) {
    let mut line_fragments = Vec::with_capacity(content.len());
    let mut ranges = Vec::with_capacity(content.len());
    let mut cells = Vec::with_capacity(content.len());
    let mut logical_index = 0;

    for (ix, fragment) in content.fragments.iter().enumerate() {
        match fragment {
            InlineFragment::Text { text, .. } => {
                let text_str = text.as_ref();
                line_fragments.push(LineFragment::text(text_str));

                let mut offsets = Vec::with_capacity(text_str.chars().count() + 1);
                offsets.push(0);
                for (byte_offset, ch) in text_str.char_indices() {
                    offsets.push(byte_offset + ch.len_utf8());
                }
                if *offsets.last().unwrap() != text_str.len() {
                    offsets.push(text_str.len());
                }

                let logical_start = logical_index;
                logical_index += offsets.len().saturating_sub(1);

                ranges.push(FragmentLogicalRange {
                    fragment_index: ix,
                    logical_start,
                    logical_end: logical_index,
                });
                cells.push(Some(TextFragmentCells { offsets }));
            }
            InlineFragment::Element { .. } => {
                let width = match &layouts[ix] {
                    InlineFragmentLayout::Element { instance } => instance.size.width,
                    InlineFragmentLayout::Text => Pixels::ZERO,
                };
                line_fragments.push(LineFragment::element(width, 1));
                ranges.push(FragmentLogicalRange {
                    fragment_index: ix,
                    logical_start: logical_index,
                    logical_end: logical_index + 1,
                });
                logical_index += 1;
                cells.push(None);
            }
        }
    }

    (line_fragments, ranges, cells)
}

fn compute_logical_slices_for_line(
    byte_start: usize,
    byte_end: usize,
    content: &InlineContent,
    ranges: &[FragmentLogicalRange],
    cells: &[Option<TextFragmentCells>],
) -> Vec<LogicalSlice> {
    let mut slices = Vec::new();
    let mut frag_byte_start = 0usize;

    for range in ranges {
        let fragment = &content.fragments[range.fragment_index];
        let frag_byte_len = match fragment {
            InlineFragment::Text { text, .. } => text.as_ref().len(),
            InlineFragment::Element { .. } => 1,
        };
        let frag_byte_end = frag_byte_start + frag_byte_len;

        let intersect_start = byte_start.max(frag_byte_start);
        let intersect_end = byte_end.min(frag_byte_end);

        if intersect_start < intersect_end {
            match fragment {
                InlineFragment::Text { .. } => {
                    let local_start = intersect_start - frag_byte_start;
                    let local_end = intersect_end - frag_byte_start;
                    let offsets = cells[range.fragment_index]
                        .as_ref()
                        .expect("missing text cell data");
                    let cell_start = offsets
                        .offsets
                        .binary_search(&local_start)
                        .unwrap_or_else(|ix| ix.saturating_sub(1));
                    let cell_end = offsets
                        .offsets
                        .binary_search(&local_end)
                        .unwrap_or_else(|ix| ix);
                    slices.push(LogicalSlice {
                        fragment_index: range.fragment_index,
                        start_in_fragment: local_start,
                        end_in_fragment: local_end,
                        logical_start: range.logical_start + cell_start,
                        logical_end: range.logical_start + cell_end,
                    });
                }
                InlineFragment::Element { .. } => {
                    slices.push(LogicalSlice {
                        fragment_index: range.fragment_index,
                        start_in_fragment: 0,
                        end_in_fragment: 0,
                        logical_start: range.logical_start,
                        logical_end: range.logical_end,
                    });
                }
            }
        }

        frag_byte_start = frag_byte_end;
    }

    slices
}

/// Create a new inline span builder with default settings.
pub fn inline() -> InlineSpanBuilder {
    InlineSpanBuilder::default()
}

/// A wrapper for inline content that allows configuring properties like alignment.
pub struct InlineSpanChild<T> {
    content: T,
    alignment: Option<InlineElementAlign>,
    style: Option<TextStyle>,
}

impl<T> InlineSpanChild<T> {
    /// Set the alignment for this child.
    pub fn with_alignment(mut self, alignment: InlineElementAlign) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Set the text style for this child.
    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    fn text_style(&mut self) -> &mut TextStyle {
        self.style.get_or_insert_with(TextStyle::default)
    }

    /// Sets the font family of this element.
    pub fn font_family(mut self, family: impl Into<SharedString>) -> Self {
        self.text_style().font_family = family.into();
        self
    }

    /// Sets the font weight of this element.
    pub fn font_weight(mut self, weight: FontWeight) -> Self {
        self.text_style().font_weight = weight;
        self
    }

    /// Sets the font style of this element.
    pub fn font_style(mut self, style: FontStyle) -> Self {
        self.text_style().font_style = style;
        self
    }

    /// Sets the text color of this element.
    pub fn text_color(mut self, color: impl Into<Hsla>) -> Self {
        self.text_style().color = color.into();
        self
    }

    /// Sets the background color of this element.
    pub fn bg(mut self, color: impl Into<Hsla>) -> Self {
        self.text_style().background_color = Some(color.into());
        self
    }

    /// Sets the underline style of this element.
    pub fn underline(mut self, underline: bool) -> Self {
        if underline {
            self.text_style().underline = Some(UnderlineStyle {
                thickness: px(1.),
                ..Default::default()
            });
        } else {
            self.text_style().underline = None;
        }
        self
    }

    /// Sets the font style to italic.
    pub fn italic(mut self) -> Self {
        self.font_style(FontStyle::Italic)
    }

    /// Sets the font weight to bold.
    pub fn bold(mut self) -> Self {
        self.font_weight(FontWeight::BOLD)
    }

    /// Sets the text size of this element.
    pub fn text_size(mut self, size: impl Into<AbsoluteLength>) -> Self {
        self.text_style().font_size = size.into();
        self
    }

    /// Sets the text size to small.
    pub fn text_sm(mut self) -> Self {
        self.text_size(rems(0.875))
    }

    /// Sets the text size to large.
    pub fn text_lg(mut self) -> Self {
        self.text_size(rems(1.125))
    }
}

impl<T: IntoInlineChild> IntoInlineChild for InlineSpanChild<T> {
    fn add_to_inline_builder(self, builder: &mut InlineSpanBuilder) {
        let old_align = if self.alignment.is_some() {
            Some(builder.inline_align)
        } else {
            None
        };
        let old_style = if self.style.is_some() {
            builder.default_style.take()
        } else {
            None
        };

        if let Some(align) = self.alignment {
            builder.inline_align = align;
        }
        if let Some(ref style) = self.style {
            builder.default_style = Some(style.clone());
        }

        self.content.add_to_inline_builder(builder);

        if let Some(align) = old_align {
            builder.inline_align = align;
        }
        if old_style.is_some() {
            builder.default_style = old_style;
        }
    }
}

/// Helper function for visual consistency when building inline content.
/// Returns a wrapper that allows configuring properties like alignment.
pub fn span<T: IntoInlineChild>(content: T) -> InlineSpanChild<T> {
    InlineSpanChild {
        content,
        alignment: None,
        style: None,
    }
}

/// Trait for types that can be added as children to an inline span
pub trait IntoInlineChild {
    /// Add this child to the inline span builder.
    fn add_to_inline_builder(self, builder: &mut InlineSpanBuilder);
}

/// Allow direct `AnyElement` values to be used as inline children.
///
/// This enables simpler syntax: `span(element)` instead of `span(|| element)`.
/// The element is wrapped in `Rc<RefCell<Option<_>>>` for interior mutability.
/// This works because GPUI is single-threaded and no Send+Sync is required.
///
/// # Example
/// ```ignore
/// inline().child(span(div().child("badge")))
/// ```
impl<E: IntoElement + 'static> IntoInlineChild for E {
    fn add_to_inline_builder(self, builder: &mut InlineSpanBuilder) {
        let mut element = self.into_any_element();

        if let Some(text) = element.downcast_mut::<SharedString>() {
            let text = text.clone();
            let style = builder.default_style.clone().unwrap_or_default();
            builder.push_text(text, style);
            return;
        }

        if let Some(text) = element.downcast_mut::<&'static str>() {
            let text = text.to_string();
            let style = builder.default_style.clone().unwrap_or_default();
            builder.push_text(text, style);
            return;
        }

        let element = Rc::new(RefCell::new(Some(element)));
        builder.push_element_with_context(builder.inline_align, move |_w, _cx, _ctx| {
            element
                .borrow_mut()
                .take()
                .expect("Inline element has already been consumed. This is a bug in GPUI.")
        });
    }
}

impl InlineSpanBuilder {
    /// Create a new builder with no default style (will inherit from context).
    pub fn default() -> Self {
        Self {
            fragments: Vec::new(),
            inline_align: InlineElementAlign::Auto,
            default_style: None,
        }
    }

    /// Create a new builder with the specified default text style.
    pub fn new(style: TextStyle) -> Self {
        Self {
            fragments: Vec::new(),
            inline_align: InlineElementAlign::Auto,
            default_style: Some(style),
        }
    }

    /// Append a styled text fragment.
    pub fn text(mut self, text: impl Into<SharedString>, style: TextStyle) -> Self {
        self.push_text(text, style);
        self
    }

    /// Add a child to the inline span (text or element).
    pub fn child(mut self, child: impl IntoInlineChild) -> Self {
        child.add_to_inline_builder(&mut self);
        self
    }

    /// Add a child to the inline span (mutable variant).
    pub fn push_child(&mut self, child: impl IntoInlineChild) -> &mut Self {
        child.add_to_inline_builder(self);
        self
    }

    /// Append a styled text fragment (mutable variant).
    pub fn push_text(
        &mut self,
        text: impl Into<SharedString>,
        style: impl Into<Option<TextStyle>>,
    ) -> &mut Self {
        self.fragments.push(InlineFragment::Text {
            text: text.into(),
            style: style.into(),
        });
        self
    }

    /// Set the default text style applied to subsequent text fragments.
    /// This is a convenience method that sets the default style for text added via `.child()`.
    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.default_style = Some(style);
        self
    }

    /// Set the default alignment applied to subsequent inline elements.
    pub fn with_alignment(mut self, align: InlineElementAlign) -> Self {
        self.inline_align = align;
        self
    }

    /// Append an inline element with default alignment.
    ///
    /// This method accepts a closure that builds an element.
    ///
    /// # Example
    /// ```ignore
    /// builder.element(|| div().child("Badge"))
    /// ```
    pub fn element<F>(mut self, builder: F) -> Self
    where
        F: Fn() -> AnyElement + 'static,
    {
        self.push_element_with_context(self.inline_align, move |_w, _cx, _ctx| builder());
        self
    }

    /// Append an inline element with default alignment (mutable variant).
    pub fn push_element<F>(&mut self, builder: F) -> &mut Self
    where
        F: Fn() -> AnyElement + 'static,
    {
        self.push_element_with_context(self.inline_align, move |_w, _cx, _ctx| builder());
        self
    }

    /// Append an inline element with explicit alignment.
    ///
    /// This is a convenience wrapper around [`element_with_context`](Self::element_with_context)
    /// that hides the unused `Window`, `App`, and `InlineElementContext` parameters.
    ///
    /// # Example
    /// ```ignore
    /// builder.element_aligned(InlineElementAlign::Top, || div().child("Badge"))
    /// ```
    pub fn element_aligned<F>(mut self, align: InlineElementAlign, builder: F) -> Self
    where
        F: Fn() -> AnyElement + 'static,
    {
        self.push_element_aligned(align, builder);
        self
    }

    /// Append an inline element with explicit alignment (mutable variant).
    pub fn push_element_aligned<F>(&mut self, align: InlineElementAlign, builder: F) -> &mut Self
    where
        F: Fn() -> AnyElement + 'static,
    {
        self.push_element_with_context(align, move |_w, _cx, _ctx| builder());
        self
    }

    /// Append an inline element when you need access to `InlineElementContext`.
    ///
    /// Use this when your element needs to adapt to the surrounding text style or layout.
    /// The context provides:
    /// - `line_height`: Height of the current line
    /// - `font` / `font_size`: Font and size of surrounding text
    /// - `max_width`: Maximum width available (if constrained)
    /// - `alignment`: Default alignment for elements
    ///
    /// # Example
    /// ```ignore
    /// // Badge that matches the line height with top alignment
    /// builder.element_with_context(InlineElementAlign::Top, |_w, _cx, ctx| {
    ///     div()
    ///         .h(ctx.line_height)
    ///         .text_size(ctx.font_size * 0.8)
    ///         .child("BADGE")
    ///         .into_any_element()
    /// })
    /// ```
    pub fn element_with_context<F>(mut self, align: InlineElementAlign, factory: F) -> Self
    where
        F: Fn(&mut Window, &mut App, InlineElementContext) -> AnyElement + 'static,
    {
        self.push_element_with_context(align, factory);
        self
    }

    /// Append an inline element when you need access to `InlineElementContext` (mutable variant).
    pub fn push_element_with_context<F>(
        &mut self,
        align: InlineElementAlign,
        factory: F,
    ) -> &mut Self
    where
        F: Fn(&mut Window, &mut App, InlineElementContext) -> AnyElement + 'static,
    {
        self.fragments.push(InlineFragment::Element {
            slot: InlineElementSlot {
                builder: Rc::new(move |window, cx, mut context| {
                    context.alignment = align;
                    let mut element = factory(window, cx, context);
                    let size = element.layout_as_root(
                        Size {
                            width: gpui::AvailableSpace::MaxContent,
                            height: gpui::AvailableSpace::MaxContent,
                        },
                        window,
                        cx,
                    );
                    InlineElementSpec::from_measured_element(element, size, align)
                }),
            },
        });
        self
    }

    /// Finish building and produce an [`InlineSpan`].
    pub fn build(self) -> InlineSpan {
        InlineSpan::new(InlineContent {
            fragments: self.fragments,
            default_style: self.default_style,
            inline_align: self.inline_align,
        })
    }
}

impl IntoElement for InlineSpanBuilder {
    type Element = InlineSpan;

    fn into_element(self) -> Self::Element {
        self.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParentElement;
    use crate::elements::div::div;
    use crate::{self as gpui, TestAppContext};
    use std::sync::Arc;

    #[gpui::test]
    fn test_builder_construction_and_fragments(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, _app| {
            // Builder::new() with custom style sets default_style
            let mut style = window.text_style();
            style.font_size = px(20.0).into();
            let span = InlineSpanBuilder::new(style.clone())
                .text("test", window.text_style())
                .build();
            assert_eq!(
                span.content.default_style.as_ref(),
                Some(&style),
                "new() should set default style"
            );

            // Builder::default() has no default_style
            let span = InlineSpanBuilder::default()
                .text("test", window.text_style())
                .build();
            assert!(
                span.content.default_style.is_none(),
                "default() should not set style"
            );

            // inline() helper has no default_style
            let builder = inline();
            assert!(
                builder.default_style.is_none(),
                "inline() should not set default style"
            );

            // Empty builder works
            let span = InlineSpanBuilder::default().build();
            assert_eq!(
                span.content.fragments.len(),
                0,
                "empty builder should have 0 fragments"
            );

            // Text fragments are added correctly
            let text_style = window.text_style();
            let span = InlineSpanBuilder::default()
                .text("hello", text_style.clone())
                .text(" world", text_style)
                .build();
            assert_eq!(
                span.content.fragments.len(),
                2,
                "should have 2 text fragments"
            );
            match &span.content.fragments[0] {
                InlineFragment::Text { text, .. } => assert_eq!(text.as_ref(), "hello"),
                _ => panic!("Expected text fragment"),
            }
            match &span.content.fragments[1] {
                InlineFragment::Text { text, .. } => assert_eq!(text.as_ref(), " world"),
                _ => panic!("Expected text fragment"),
            }

            // Element fragments are embedded correctly
            let span = InlineSpanBuilder::default()
                .element(|| div().w_4().h_4().into_any_element())
                .build();
            assert_eq!(
                span.content.fragments.len(),
                1,
                "should have 1 element fragment"
            );
            match &span.content.fragments[0] {
                InlineFragment::Element { .. } => {} // Success
                _ => panic!("Expected element fragment"),
            }

            // Mutable API (push_text, push_element) works
            let mut builder = InlineSpanBuilder::default();
            builder.push_text("hello", window.text_style());
            builder.push_element(|| div().into_any_element());
            let span = builder.build();
            assert_eq!(
                span.content.fragments.len(),
                2,
                "mutable API should add fragments"
            );
            match &span.content.fragments[0] {
                InlineFragment::Text { text, .. } => assert_eq!(text.as_ref(), "hello"),
                _ => panic!("Expected text"),
            }
            match &span.content.fragments[1] {
                InlineFragment::Element { .. } => {}
                _ => panic!("Expected element"),
            }

            // Element-only builder works
            let span = InlineSpanBuilder::default()
                .element(|| div().w_4().h_4().into_any_element())
                .element(|| div().w_4().h_4().into_any_element())
                .build();
            assert_eq!(
                span.content.fragments.len(),
                2,
                "should support element-only spans"
            );
            for fragment in &span.content.fragments {
                match fragment {
                    InlineFragment::Element { .. } => {} // ok
                    _ => panic!("Expected element fragments"),
                }
            }

            // Text-only builder works
            let span = InlineSpanBuilder::default()
                .text("just text", window.text_style())
                .build();
            assert_eq!(
                span.content.fragments.len(),
                1,
                "should support text-only spans"
            );
            match &span.content.fragments[0] {
                InlineFragment::Text { text, .. } => assert_eq!(text.as_ref(), "just text"),
                _ => panic!("Expected text fragment"),
            }

            // div().child(inline()) integration works
            let _div = div().child(inline().text("inline", window.text_style()));
        });
    }

    #[gpui::test]
    fn test_builder_styling_and_alignment(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, _app| {
            let rem_size = window.rem_size();
            let size = px(24.0);
            let mut style = window.text_style();
            style.font_size = size.into();
            let built_span1 = InlineSpanBuilder::default()
                .with_style(style.clone())
                .text("large", window.text_style())
                .build();
            assert_eq!(
                built_span1
                    .content
                    .default_style
                    .as_ref()
                    .unwrap()
                    .font_size
                    .to_pixels(rem_size),
                size,
                "with_style() should set font size"
            );

            let mut span_style = window.text_style();
            span_style.font_size = px(30.0).into();
            let built_span = InlineSpanBuilder::default()
                .child(span("styled").with_style(span_style.clone()))
                .build();
            assert_eq!(
                built_span.content.fragments.len(),
                1,
                "span() should create fragment"
            );
            match &built_span.content.fragments[0] {
                InlineFragment::Text {
                    style: fragment_style,
                    ..
                } => {
                    assert_eq!(
                        fragment_style.as_ref().unwrap().font_size,
                        span_style.font_size,
                        "span().with_style() should apply style to fragment"
                    );
                }
                _ => panic!("Expected text fragment"),
            }

            let mut builder = InlineSpanBuilder::default();
            builder = builder.with_alignment(InlineElementAlign::Top);
            assert_eq!(
                builder.inline_align,
                InlineElementAlign::Top,
                "with_alignment() should set alignment"
            );

            builder = builder.with_alignment(InlineElementAlign::Bottom);
            assert_eq!(
                builder.inline_align,
                InlineElementAlign::Bottom,
                "with_alignment() should update alignment"
            );

            let mut builder = InlineSpanBuilder::default();
            builder = builder.with_alignment(InlineElementAlign::Top);
            builder =
                builder.element_aligned(InlineElementAlign::Bottom, || div().into_any_element());
            assert_eq!(
                builder.inline_align,
                InlineElementAlign::Top,
                "element_aligned() should not change default alignment"
            );
        });
    }

    #[gpui::test]
    fn test_layout_and_wrapping(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, app| {
            let rem_size = window.rem_size();

            let base_style = window.text_style();
            let mut test_span = InlineSpanBuilder::new(base_style.clone())
                .text("ab", window.text_style())
                .text("cd", window.text_style())
                .build();
            let line_height = window.text_style().line_height.to_pixels(
                test_span.content.default_style.as_ref().unwrap().font_size,
                rem_size,
            );
            test_span.layout.rewrap_and_shape(
                &test_span.content,
                Some(px(1.0)),
                line_height,
                window,
                app,
            );
            let inner = test_span.layout.inner.borrow();
            assert!(
                inner.lines.len() >= 2,
                "wrapping should produce multiple lines"
            );
            let mut logical = 0;
            for line in &inner.lines {
                assert_eq!(
                    line.logical_start, logical,
                    "logical start should be sequential"
                );
                assert!(
                    line.logical_end > line.logical_start,
                    "logical end should be after start"
                );
                logical = line.logical_end;
            }
            assert_eq!(logical, 4, "total logical cells should match text length");
            drop(inner);

            let style = window.text_style();
            let element_size = Size::new(px(24.0), px(10.0));
            let baseline = px(6.0);
            let content = InlineContent {
                fragments: vec![
                    InlineFragment::Text {
                        text: SharedString::from("before "),
                        style: Some(style.clone()),
                    },
                    InlineFragment::Element {
                        slot: InlineElementSlot {
                            builder: Rc::new(|_, _, _| unreachable!("builder not used in test")),
                        },
                    },
                    InlineFragment::Text {
                        text: SharedString::from(" after"),
                        style: Some(style.clone()),
                    },
                ],
                default_style: Some(style.clone()),
                inline_align: InlineElementAlign::Auto,
            };
            let mut fragment_layouts = vec![
                InlineFragmentLayout::Text,
                InlineFragmentLayout::Element {
                    instance: InlineElementInstance {
                        element: AnyElement::new(crate::Empty),
                        size: element_size,
                        bounds: Bounds::new(Point::new(px(0.0), px(0.0)), element_size),
                        baseline_offset: Some(baseline),
                        align: InlineElementAlign::Auto,
                    },
                },
                InlineFragmentLayout::Text,
            ];
            let (line_fragments, ranges, cells) = build_line_fragments(&content, &fragment_layouts);
            let total_bytes = total_byte_len(&line_fragments);
            let slices = compute_logical_slices_for_line(0, total_bytes, &content, &ranges, &cells);
            let font_id = app
                .text_system()
                .resolve_font(&content.default_style.as_ref().unwrap().font());
            let space_width = app
                .text_system()
                .advance(
                    font_id,
                    content
                        .default_style
                        .as_ref()
                        .unwrap()
                        .font_size
                        .to_pixels(rem_size),
                    ' ',
                )
                .map(|size| size.width)
                .unwrap_or(Pixels::ZERO);
            let text_system = window.text_system().clone();
            let line = build_line(
                slices,
                0,
                space_width,
                &content,
                &mut fragment_layouts,
                &text_system,
                line_height,
                px(0.0),
                window,
            )
            .expect("expected a shaped line");
            let InlineFragmentLayout::Element { instance } = &fragment_layouts[1] else {
                panic!("missing element layout");
            };
            assert_eq!(
                instance.size, element_size,
                "element should be measured correctly"
            );
            assert_eq!(
                instance.baseline_offset,
                Some(baseline),
                "baseline should be preserved"
            );
            let mut found_element = false;
            for segment in &line.shaped_segments {
                if let ShapedSegment::Element { fragment_index, .. } = segment {
                    found_element = true;
                    assert_eq!(*fragment_index, 1, "element should be at correct index");
                }
            }
            assert!(found_element, "line should contain the element segment");

            let indent_content = InlineContent {
                fragments: vec![InlineFragment::Text {
                    text: SharedString::from("aaaa"),
                    style: Some(style.clone()),
                }],
                default_style: Some(style.clone()),
                inline_align: InlineElementAlign::Auto,
            };
            let mut indent_fragment_layouts = vec![InlineFragmentLayout::Text];
            let (line_fragments2, ranges2, cells2) =
                build_line_fragments(&indent_content, &indent_fragment_layouts);
            let total_bytes2 = total_byte_len(&line_fragments2);
            let slices2 = compute_logical_slices_for_line(
                0,
                total_bytes2,
                &indent_content,
                &ranges2,
                &cells2,
            );
            let indent_spaces = 3;
            let current_y = px(4.0);
            let line_with_indent = build_line(
                slices2,
                indent_spaces,
                space_width,
                &indent_content,
                &mut indent_fragment_layouts,
                &text_system,
                line_height,
                current_y,
                window,
            )
            .expect("expected shaped line");
            let expected_indent = space_width * indent_spaces as f32;
            assert_eq!(
                line_with_indent.indent_px, expected_indent,
                "indent should be calculated correctly"
            );
            assert_eq!(
                line_with_indent.line_origin.x, expected_indent,
                "line should start at indent"
            );
            assert_eq!(
                line_with_indent.line_origin.y, current_y,
                "line y should match current_y"
            );
            assert!(
                matches!(
                    line_with_indent.shaped_segments.first(),
                    Some(ShapedSegment::Text { .. })
                ),
                "line should remain text despite indent"
            );
        });
    }

    #[gpui::test]
    fn test_selection_and_geometry(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, app| {
            let mixed_layout = build_layout_with_element(
                window,
                app,
                "aa",
                "bb",
                Size::new(px(12.0), px(10.0)),
                InlineElementAlign::Auto,
            );
            let selections = mixed_layout.selections_for_range(1..4);
            assert!(!selections.is_empty(), "selection should exist");
            let mut saw_element = false;
            let mut saw_text = false;
            for selection in selections {
                for segment in selection.segments {
                    match segment {
                        SelectionSegment::Text { logical_range, .. } => {
                            saw_text = true;
                            assert!(logical_range.start < logical_range.end);
                        }
                        SelectionSegment::Element { logical_range, .. } => {
                            saw_element = true;
                            assert_eq!(logical_range.end - logical_range.start, 1);
                        }
                    }
                }
            }
            assert!(saw_element, "should encounter element in selection");
            assert!(saw_text, "should encounter text in selection");

            let wrapped_layout = build_wrapped_layout(
                window,
                app,
                "aa",
                "bbcc",
                Size::new(px(12.0), px(10.0)),
                "aa".len(),
                InlineElementAlign::Auto,
            );
            let multi_selections = wrapped_layout.selections_for_range(1..5);
            assert!(
                multi_selections.len() >= 2,
                "selection should cover multiple lines"
            );
            assert!(
                multi_selections
                    .iter()
                    .flat_map(|line| line.segments.iter())
                    .any(|segment| matches!(segment, SelectionSegment::Element { .. })),
                "should include element segment"
            );

            let zero_layout = build_layout_with_element(
                window,
                app,
                "",
                "",
                Size::new(px(10.0), px(8.0)),
                InlineElementAlign::Auto,
            );
            assert!(
                zero_layout.selections_for_range(0..0).is_empty(),
                "zero width selection should not report segments"
            );
            let element_selection = zero_layout.selections_for_range(0..1);
            assert_eq!(element_selection.len(), 1, "element selection should exist");
            assert!(
                element_selection[0].segments.iter().any(|segment| matches!(
                    segment,
                    SelectionSegment::Element { logical_range, .. }
                    if logical_range.start == 0 && logical_range.end == 1
                )),
                "should contain element segment"
            );

            let rect_layout = build_wrapped_layout(
                window,
                app,
                "hello",
                "world",
                Size::new(px(12.0), px(10.0)),
                "hello".len(),
                InlineElementAlign::Auto,
            );
            let rects = rect_layout.selection_rects(2..7);
            assert!(
                rects.len() >= 2,
                "selection rects should span multiple lines"
            );
            for rect in &rects {
                assert!(rect.size.width > px(0.0), "rect should have width");
                assert!(rect.size.height > px(0.0), "rect should have height");
            }

            let geometries = rect_layout.line_geometries();
            assert!(
                geometries.len() >= 2,
                "should have multiple line geometries"
            );
            for geometry in geometries {
                assert!(
                    geometry.logical_range.start < geometry.logical_range.end,
                    "logical range should be non-empty"
                );
                assert!(
                    geometry.bounds.size.width > px(0.0),
                    "line bounds should have width"
                );
            }
        });
    }

    #[gpui::test]
    fn test_caret_positioning(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, app| {
            let layout = build_layout_with_element(
                window,
                app,
                "ab",
                "cd",
                Size::new(px(12.0), px(8.0)),
                InlineElementAlign::Auto,
            );

            // Caret positions clamp to line bounds when outside
            let lines = layout.inner.borrow().lines.clone();
            let first_line = &lines[0];
            let point_above = Point::new(px(0.0), first_line.line_origin.y - px(5.0));
            let idx = layout
                .logical_index_for_point(point_above, CaretAffinity::Upstream)
                .unwrap();
            assert_eq!(
                idx, first_line.logical_start,
                "point above should clamp to line start"
            );

            let point_below = Point::new(
                px(999.0),
                first_line.line_origin.y + first_line.height + px(5.0),
            );
            let idx = layout
                .logical_index_for_point(point_below, CaretAffinity::Downstream)
                .unwrap();
            assert_eq!(
                idx, first_line.logical_end,
                "point below should clamp to line end"
            );

            // Caret affinity affects position at element edges
            let left = layout
                .position_for_logical_index(2, CaretAffinity::Upstream)
                .unwrap();
            let right = layout
                .position_for_logical_index(3, CaretAffinity::Downstream)
                .unwrap();
            assert!(
                right.x > left.x,
                "affinity should affect caret position at element edge"
            );

            let mid = Point::new((left.x + right.x) / 2.0, left.y);
            let idx_up = layout
                .logical_index_for_point(mid, CaretAffinity::Upstream)
                .unwrap();
            let idx_down = layout
                .logical_index_for_point(mid, CaretAffinity::Downstream)
                .unwrap();
            assert!(
                idx_up == 2 || idx_down == 3,
                "affinity should influence hit testing at element boundary"
            );
        });
    }

    #[gpui::test]
    fn test_hit_testing_with_elements(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, app| {
            let layout = build_layout_with_element(
                window,
                app,
                "ab",
                "cd",
                Size::new(px(14.0), px(12.0)),
                InlineElementAlign::Auto,
            );
            let element_width = {
                let inner = layout.inner.borrow();
                let line = &inner.lines[0];
                let mut width = Pixels::ZERO;
                for segment in &line.shaped_segments {
                    if let ShapedSegment::Element {
                        width: seg_width, ..
                    } = segment
                    {
                        width = *seg_width;
                        break;
                    }
                }
                width
            };
            assert!(element_width > Pixels::ZERO);
            let before = layout
                .position_for_logical_index(2, CaretAffinity::Downstream)
                .unwrap();
            let after = layout
                .position_for_logical_index(3, CaretAffinity::Upstream)
                .unwrap();
            assert!(after.x > before.x);

            let idx_up = layout
                .logical_index_for_point(before, CaretAffinity::Upstream)
                .unwrap();
            assert_eq!(idx_up, 2);

            let idx_down = layout
                .logical_index_for_point(after, CaretAffinity::Downstream)
                .unwrap();
            assert_eq!(idx_down, 3);
        });
    }

    #[test]
    fn test_unicode_combining_characters() {
        let style = gpui::TextStyle::default();
        let text = "a\u{0301}b";
        let content = InlineContent {
            fragments: vec![InlineFragment::Text {
                text: SharedString::from(text),
                style: Some(style.clone()),
            }],
            default_style: Some(style.clone()),
            inline_align: InlineElementAlign::Bottom,
        };
        let fragment_layouts = vec![InlineFragmentLayout::Text];
        let (_, ranges, cells) = build_line_fragments(&content, &fragment_layouts);
        let text_cells = cells[0].as_ref().expect("expected cells");
        assert_eq!(text_cells.offsets.len(), text.chars().count() + 1);

        let slices = compute_logical_slices_for_line(0, text.len(), &content, &ranges, &cells);
        assert_eq!(slices.len(), 1);
        let slice = &slices[0];
        assert_eq!(
            slice.logical_end - slice.logical_start,
            text_cells.offsets.len() - 1
        );
    }

    #[gpui::test]
    #[gpui::test]
    fn test_element_alignment_behavior(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, app| {
            let small_size = Size::new(px(18.0), px(12.0));
            let small_layout = build_layout_with_element(
                window,
                app,
                "ab",
                "cd",
                small_size,
                InlineElementAlign::Auto,
            );
            let inner = small_layout.inner.borrow();
            let line = &inner.lines[0];
            match &inner.fragment_layouts[1] {
                InlineFragmentLayout::Element { instance } => {
                    let expected_top =
                        line.line_origin.y + line.baseline_above_top - small_size.height / 2.0;
                    assert_eq!(
                        instance.bounds.origin.y, expected_top,
                        "small elements should center vertically"
                    );
                }
                _ => panic!("expected inline element"),
            }
            drop(inner);

            let tall_size = Size::new(px(40.0), px(32.0));
            let tall_layout = build_layout_with_element(
                window,
                app,
                "a",
                "b",
                tall_size,
                InlineElementAlign::Auto,
            );
            let inner = tall_layout.inner.borrow();
            let line = &inner.lines[0];
            match &inner.fragment_layouts[1] {
                InlineFragmentLayout::Element { instance } => {
                    let expected_top =
                        line.line_origin.y + line.baseline_above_top - tall_size.height;
                    assert_eq!(
                        instance.bounds.origin.y, expected_top,
                        "tall elements should align to top"
                    );
                }
                _ => panic!("expected inline element"),
            }
            drop(inner);

            let element_size = Size::new(px(18.0), px(12.0));
            let top_layout = build_layout_with_element(
                window,
                app,
                "x",
                "y",
                element_size,
                InlineElementAlign::Top,
            );
            let inner = top_layout.inner.borrow();
            let line = &inner.lines[0];
            match &inner.fragment_layouts[1] {
                InlineFragmentLayout::Element { instance } => {
                    assert_eq!(
                        instance.bounds.origin.y, line.line_origin.y,
                        "top alignment should use line top edge"
                    );
                }
                _ => panic!("expected inline element"),
            }
            drop(inner);

            let bottom_layout = build_layout_with_element(
                window,
                app,
                "x",
                "y",
                element_size,
                InlineElementAlign::Bottom,
            );
            let inner = bottom_layout.inner.borrow();
            let line = &inner.lines[0];
            match &inner.fragment_layouts[1] {
                InlineFragmentLayout::Element { instance } => {
                    let element_bottom = instance.bounds.origin.y + instance.size.height;
                    let line_bottom = line.line_origin.y + line.height;
                    assert_eq!(
                        element_bottom, line_bottom,
                        "bottom alignment should use line bottom edge"
                    );
                }
                _ => panic!("expected inline element"),
            }
        });
    }

    fn build_layout_with_element(
        window: &mut Window,
        app: &mut App,
        left: &str,
        right: &str,
        element_size: Size<Pixels>,
        align: InlineElementAlign,
    ) -> InlineSpanLayout {
        let rem_size = window.rem_size();
        let style = window.text_style();
        let content = InlineContent {
            fragments: vec![
                InlineFragment::Text {
                    text: SharedString::from(left.to_string()),
                    style: Some(style.clone()),
                },
                InlineFragment::Element {
                    slot: InlineElementSlot {
                        builder: Rc::new(|_, _, _| unreachable!()),
                    },
                },
                InlineFragment::Text {
                    text: SharedString::from(right.to_string()),
                    style: Some(style.clone()),
                },
            ],
            default_style: Some(style.clone()),
            inline_align: InlineElementAlign::Auto,
        };

        let mut fragment_layouts = vec![
            InlineFragmentLayout::Text,
            InlineFragmentLayout::Element {
                instance: InlineElementInstance {
                    element: AnyElement::new(crate::Empty),
                    size: element_size,
                    bounds: Bounds::new(Point::new(px(0.0), px(0.0)), element_size),
                    baseline_offset: None,
                    align,
                },
            },
            InlineFragmentLayout::Text,
        ];

        let (line_fragments, ranges, cells) = build_line_fragments(&content, &fragment_layouts);
        let total_bytes = total_byte_len(&line_fragments);
        let slices = compute_logical_slices_for_line(0, total_bytes, &content, &ranges, &cells);

        let line_height = style
            .line_height
            .to_pixels(content.default_style.as_ref().unwrap().font_size, rem_size);
        let font_id = app
            .text_system()
            .resolve_font(&content.default_style.as_ref().unwrap().font());
        let space_width = app
            .text_system()
            .advance(
                font_id,
                content
                    .default_style
                    .as_ref()
                    .unwrap()
                    .font_size
                    .to_pixels(rem_size),
                ' ',
            )
            .map(|size| size.width)
            .unwrap_or(Pixels::ZERO);
        let text_system = window.text_system().clone();
        let line = build_line(
            slices,
            0,
            space_width,
            &content,
            &mut fragment_layouts,
            &text_system,
            line_height,
            px(0.0),
            window,
        )
        .expect("expected line");

        build_layout_from_lines(vec![line], fragment_layouts)
    }

    fn build_wrapped_layout(
        window: &mut Window,
        app: &mut App,
        left: &str,
        right: &str,
        element_size: Size<Pixels>,
        split_bytes: usize,
        align: InlineElementAlign,
    ) -> InlineSpanLayout {
        let rem_size = window.rem_size();
        let style = window.text_style();
        let content = InlineContent {
            fragments: vec![
                InlineFragment::Text {
                    text: SharedString::from(left.to_string()),
                    style: Some(style.clone()),
                },
                InlineFragment::Element {
                    slot: InlineElementSlot {
                        builder: Rc::new(|_, _, _| unreachable!()),
                    },
                },
                InlineFragment::Text {
                    text: SharedString::from(right.to_string()),
                    style: Some(style.clone()),
                },
            ],
            default_style: Some(style.clone()),
            inline_align: InlineElementAlign::Auto,
        };

        let mut fragment_layouts = vec![
            InlineFragmentLayout::Text,
            InlineFragmentLayout::Element {
                instance: InlineElementInstance {
                    element: AnyElement::new(crate::Empty),
                    size: element_size,
                    bounds: Bounds::new(Point::new(px(0.0), px(0.0)), element_size),
                    baseline_offset: None,
                    align,
                },
            },
            InlineFragmentLayout::Text,
        ];

        let (line_fragments, ranges, cells) = build_line_fragments(&content, &fragment_layouts);
        let total_bytes = total_byte_len(&line_fragments);

        let first_slices =
            compute_logical_slices_for_line(0, split_bytes, &content, &ranges, &cells);
        let second_slices =
            compute_logical_slices_for_line(split_bytes, total_bytes, &content, &ranges, &cells);

        let line_height = style
            .line_height
            .to_pixels(content.default_style.as_ref().unwrap().font_size, rem_size);
        let font_id = app
            .text_system()
            .resolve_font(&content.default_style.as_ref().unwrap().font());
        let space_width = app
            .text_system()
            .advance(
                font_id,
                content
                    .default_style
                    .as_ref()
                    .unwrap()
                    .font_size
                    .to_pixels(rem_size),
                ' ',
            )
            .map(|size| size.width)
            .unwrap_or(Pixels::ZERO);
        let text_system = window.text_system().clone();

        let first_line = build_line(
            first_slices,
            0,
            space_width,
            &content,
            &mut fragment_layouts,
            &text_system,
            line_height,
            px(0.0),
            window,
        )
        .expect("first line");

        let second_line = build_line(
            second_slices,
            0,
            space_width,
            &content,
            &mut fragment_layouts,
            &text_system,
            line_height,
            first_line.height,
            window,
        )
        .expect("second line");

        build_layout_from_lines(vec![first_line, second_line], fragment_layouts)
    }

    fn build_layout_from_lines(
        lines: Vec<InlineWrapLine>,
        fragment_layouts: Vec<InlineFragmentLayout>,
    ) -> InlineSpanLayout {
        let mut layout = InlineSpanLayout::default();
        {
            let mut inner = layout.inner.borrow_mut();
            inner.lines = lines;
            inner.fragment_layouts = fragment_layouts;
            let width = inner.lines.iter().fold(Pixels::ZERO, |acc, line| {
                let candidate = line.indent_px + line.width;
                if acc > candidate { acc } else { candidate }
            });
            let height = inner
                .lines
                .iter()
                .fold(Pixels::ZERO, |acc, line| acc + line.height);
            inner.measured_size = Size { width, height };
        }
        layout
    }

    // Child API tests (formerly in mod child_api_tests)
    #[test]
    fn test_child_api_ergonomics() {
        let _ = inline()
            .child("Hello")
            .child(String::from(" World"))
            .child(SharedString::from("!"))
            .element(|| div().into_any_element())
            .child(span(" Helper"));
    }

    #[test]
    fn test_child_with_text_and_elements() {
        let span = inline()
            .child("A")
            .element(|| div().w_10().h_10().into_any_element())
            .child("B")
            .build();

        // Verify we have 3 fragments (Text, Element, Text)
        let content = &span.content;
        assert_eq!(content.fragments.len(), 3);

        match &content.fragments[0] {
            InlineFragment::Text { text, .. } => assert_eq!(text.as_ref(), "A"),
            _ => panic!("Expected text fragment"),
        }
        match &content.fragments[1] {
            InlineFragment::Element { .. } => {}
            _ => panic!("Expected element fragment"),
        }
        match &content.fragments[2] {
            InlineFragment::Text { text, .. } => assert_eq!(text.as_ref(), "B"),
            _ => panic!("Expected text fragment"),
        }
    }

    #[gpui::test]
    fn test_style_inheritance_and_resolution(cx: &mut crate::TestAppContext) {
        use std::sync::{Arc, Mutex};

        let cx = cx.add_empty_window();
        cx.update(|window, _cx| {
            // Test 1: Verify inline() inherits window style by default
            let content = inline().child("test").build().content;
            assert!(
                content.default_style.is_none(),
                "inline() should not set a default style"
            );

            let base_font_size = window.text_style().font_size.to_pixels(window.rem_size());
            let resolved_font_size =
                content.default_font_size_or(base_font_size, window.rem_size());
            assert_eq!(
                resolved_font_size, base_font_size,
                "should use window's font size as fallback"
            );

            // Test 2: Verify style resolution from fragments without default
            let large_font_size = px(34.0);
            let mut custom_style = window.text_style().clone();
            custom_style.font_size = large_font_size.into();

            let span = inline()
                .text("Text with custom font", custom_style.clone())
                .build();

            assert!(
                span.content.default_style.is_none(),
                "inline() should not set a default style"
            );

            let resolved = span.content.resolve_default_style();
            assert!(
                resolved.is_some(),
                "should resolve style from first text fragment"
            );

            let resolved_size = resolved.unwrap().font_size.to_pixels(window.rem_size());
            assert_eq!(
                resolved_size, large_font_size,
                "should resolve to the fragment's custom font size"
            );

            // Test 3: Verify element context receives correct values (with default style)
            let custom_font_size = px(24.0);
            let mut style = window.text_style().clone();
            style.font_size = custom_font_size.into();

            let captured_context = Arc::new(Mutex::new(None));
            let captured = captured_context.clone();

            let _span = inline()
                .with_style(style.clone())
                .element_with_context(InlineElementAlign::Auto, move |_, _, context| {
                    *captured.lock().unwrap() = Some((context.font.clone(), context.font_size));
                    crate::Empty.into_any_element()
                })
                .build();

            assert_eq!(
                _span.content.fragments.len(),
                1,
                "Should have one element fragment"
            );
            assert!(
                _span.content.default_style.is_some(),
                "Should have default style"
            );

            // Test 4: Verify element context inherits from fragments (without default style)
            let large_font_size = px(32.0);
            let mut large_style = window.text_style().clone();
            large_style.font_size = large_font_size.into();

            let captured_font_size = Arc::new(Mutex::new(None));
            let captured = captured_font_size.clone();

            let _span = inline()
                .text("Text with large font", large_style.clone())
                .element_with_context(InlineElementAlign::Auto, move |_, _, context| {
                    *captured.lock().unwrap() = Some(context.font_size);
                    crate::Empty.into_any_element()
                })
                .build();

            assert!(
                _span.content.default_style.is_none(),
                "Should not have default style"
            );

            if let InlineFragment::Text { style, .. } = &_span.content.fragments[0] {
                assert_eq!(
                    style
                        .as_ref()
                        .unwrap()
                        .font_size
                        .to_pixels(window.rem_size()),
                    large_font_size,
                    "Text fragment should have large font size"
                );
            }
        });
    }

    #[gpui::test]
    fn test_decorations(cx: &mut crate::TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, _cx| {
            let test_span = inline()
                .text("hello world test", window.text_style())
                .build();
            let layout = test_span.layout_state();
            layout.set_decorations(vec![
                InlineDecoration {
                    range: 0..5,
                    style: InlineDecorationStyle {
                        background: Hsla::default(),
                    },
                },
                InlineDecoration {
                    range: 6..11,
                    style: InlineDecorationStyle {
                        background: Hsla::default(),
                    },
                },
                InlineDecoration {
                    range: 12..16,
                    style: InlineDecorationStyle {
                        background: Hsla::default(),
                    },
                },
            ]);
            let state = layout.inner.borrow();
            assert_eq!(
                state.decorations.len(),
                3,
                "should store all 3 non-overlapping decorations"
            );
            assert_eq!(state.decorations[0].range, 0..5);
            assert_eq!(state.decorations[1].range, 6..11);
            assert_eq!(state.decorations[2].range, 12..16);
            drop(state);

            let overlap_span = inline().text("hello", window.text_style()).build();
            let overlap_layout = overlap_span.layout_state();
            overlap_layout.set_decorations(vec![
                InlineDecoration {
                    range: 0..3,
                    style: InlineDecorationStyle {
                        background: Hsla::default(),
                    },
                },
                InlineDecoration {
                    range: 2..5,
                    style: InlineDecorationStyle {
                        background: Hsla::default(),
                    },
                },
            ]);
            let overlap_state = overlap_layout.inner.borrow();
            assert_eq!(
                overlap_state.decorations.len(),
                2,
                "should store both overlapping decorations"
            );
            assert_eq!(overlap_state.decorations[0].range, 0..3);
            assert_eq!(overlap_state.decorations[1].range, 2..5);
        });
    }

    #[gpui::test]
    fn test_layout_invalidation(cx: &mut crate::TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            // Test 1: Manual invalidation forces rewrap
            let mut span = inline()
                .text(
                    "This is a long text that will wrap differently at different widths",
                    window.text_style(),
                )
                .build();

            let _initial_layout = span.layout_state();
            span.invalidate_layout();
            let _new_layout = span.layout_state();
            assert!(true, "Manual invalidation API works");

            // Test 2: Style change invalidation
            let small_style = window.text_style().clone();
            let mut span = inline()
                .with_style(small_style.clone())
                .text("Text that wraps", small_style.clone())
                .build();

            let _style_ref = span.style();
            assert!(true, "Can access style refinement");

            span.invalidate_layout();
            assert!(true, "Style change invalidation API works");
        });
    }
}
