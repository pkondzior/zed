# 🚀 INLINE SPAN — FRAGMENT-BASED IMPLEMENTATION PLAN

## 🎯 Goals

1. Render inline spans that mix styled text and interactive GPUI elements within the same wrapped lines.
2. Reuse the proven wrapping behavior of `LineWrapper` / `WrapMap` by feeding it a mixed fragment stream (text fragments + element fragments) just like `LineWithInvisibles`.
3. Preserve precise hit testing, selections, and painting by caching shaped fragments and inline element geometry in the layout state.

---

## 🔜 Prerequisites

1. **Fragment ownership contract**  
   - `InlineSpan` owns a sequence of fragments (`InlineFragment::Text` or `InlineFragment::Element`). Text fragments keep their own `SharedString` plus `Vec<TextRun>`. Element fragments hold the builder closure only.
   - The builder API must coalesce adjacent text fragments with identical styles so run counts stay minimal and glyph shaping works efficiently.

2. **Per-fragment measurement**  
   - Before wrapping, every element fragment must be measured (builder → `InlineElementSpec` → `layout_as_root`) so its width/height/baseline are known.
   - Each text fragment is shaped on demand (call `window.text_system().shape_line`) and caches a `ShapedLine`.

3. **Single-owner layout state**  
   - `InlineLayoutState` caches the wrapped lines, shaped text fragments, and inline element instances. No fragment stores per-frame state; everything transient lives in the layout state.

---

## 🧱 Core Data Structures

| Type | Purpose | Key Fields |
|------|---------|------------|
| `InlineSpan` | Public element implementing `Element`. Holds immutable `InlineContent` and mutable `InlineLayoutState`. | `content: InlineContent`, `layout_state: InlineLayoutState`. |
| `InlineContent` | Builder output. Immutable sequence of fragments + default style info. | `fragments: Vec<InlineFragment>`, `default_font`, `default_font_size`. |
| `InlineFragment` | Either text or inline element definition. | `Text { text: SharedString, runs: Vec<TextRun> }` or `Element { slot: InlineElementSlot }`. |
| `InlineElementSlot` | Placeholder for an inline widget. | `builder: Arc<dyn Send + Sync + Fn(InlineElementContext<'_, '_>) -> InlineElementSpec>`. |
| `InlineElementContext` | Passed to builders, mirrors `ChunkRendererContext`. | `window`, `app`, `max_width`, `line_height`, `font`, `font_size`. |
| `InlineElementSpec` | Builder output for measurement + paint. | `element: AnyElement`, `requested_space: Size<AvailableSpace>`, `measured_size: Size<Pixels>`, `baseline_offset: Option<Pixels>`, `constrain_width: bool`. |
| `InlineFragmentLayout` | Per-fragment cached data (text or inline element). | `Text { shaped_line: ShapedLine }` or `Element { instance: InlineElementInstance }`. |
| `InlineWrapLine` | A single visual line from the wrapping pass. | `fragment_ranges`, `width`, `height`, `line_origin`, `fragments: Vec<InlineFragmentLayoutRef>`. |
| `InlineLayoutState` | Cached layout per frame. | `lines: Vec<InlineWrapLine>`, `fragment_layouts: Vec<InlineFragmentLayout>`, `measured_size: Size<Pixels>`, `dirty: bool`. |
| `InlineElementInstance` | Element ready for paint in a specific frame. | `element: AnyElement`, `size: Size<Pixels>`, `bounds: Bounds<Pixels>`, `baseline_offset: Pixels`. |

These ten lightweight types fall into three buckets:

- **Builder-time** (`InlineSpanBuilder`, `InlineFragment`, `InlineElementSlot`) – collect immutable fragment definitions and keep ownership straightforward.
- **Immutable content** (`InlineContent`) – the single payload `InlineSpan` clones and shares, containing nothing per-frame.
- **Layout-time cache** (`InlineLayoutState`, `InlineFragmentLayout`, `InlineWrapLine`, `InlineElementInstance`, `InlineElementSpec`, `InlineElementContext`) – bundle all measured geometry and shaped text so painting and hit testing never touch the builder data.

By keeping metadata bundled this way we avoid extra lifetimes while still covering measurement, wrapping, and paint-time needs.

---

## 🔁 Layout Pipeline (Fragment-Based)

1. **Determine available width & line height**  
   Same as `StyledText`: resolve `known_dimensions.width` first, otherwise use `AvailableSpace::Definite`, and compute the text style’s line height.

2. **Measure element fragments**  
   - Iterate fragment list. For each `InlineFragment::Element`, build `InlineElementContext` and invoke the builder.
   - Call `layout_as_root` with the builder’s requested space (default `MinContent × Definite(line_height)`), store size + baseline in `InlineElementSpec`.
   - Cache the resulting `InlineElementLayout` in `InlineLayoutState.fragment_layouts`.

3. **Shape text fragments**  
   - For each `InlineFragment::Text`, call `window.text_system().shape_line` with its `SharedString` and `runs`.
   - Store the resulting `ShapedLine` in `InlineFragmentLayout::Text`.

4. **Assemble line fragments for wrapping**  
   - Build a temporary `Vec<gpui::LineFragment<'_>>` (the type defined in `crates/gpui/src/text_system/line_wrapper.rs`) by iterating the fragment list in source order:
     - For text fragments: push `LineFragment::Text { text: shaped_line.text.as_ref() }`.
     - For element fragments: push `LineFragment::Element { width: measured_size.width, len_utf8: 1 }`, reserving a single logical byte so caret navigation treats the inline widget as occupying one position in the stream.
   - Maintain a parallel structure recording, for each fragment, its starting byte index in the logical stream.

5. **Run `LineWrapper`**  
   - Feed the fragment vector into `wrapper.wrap_line(&fragments, wrap_width)` exactly like `WrapMap`.
   - For each `Boundary`, slice the fragment vector into a new `InlineWrapLine`:
     - Track which fragments appear on the line, along with their start/end byte offsets.
     - Store the line width (`max` of text + element widths) and height (line height).
     - Record references into `InlineLayoutState.fragment_layouts` so paint can reuse the cached shaped data.

6. **Compute measured size & cache lines**  
   - Accumulate heights and maximum width across all `InlineWrapLine`s.
   - Store the resulting `Size<Pixels>` plus the wrapped lines in `InlineLayoutState`.

7. **Fragment index ↔ byte offset mapping**  
   - Because we no longer need placeholder removal, caret math operates directly on fragment ranges:
     - `InlineWrapLine.fragment_ranges` stores `(fragment_index, start_byte_in_fragment, end_byte_in_fragment)` for every fragment on the line.
     - These ranges drive hit testing and selection without conversions.

---

## ✂️ Text Runs & Decorations

1. **Run slicing happens per fragment**  
   Unlike the placeholder plan, each text fragment owns the exact `Vec<TextRun>` it needs. No slicing or byte arithmetic across the canonical buffer is required.

2. **Decorations (selection, highlights)**  
   - When painting selections or background highlights, walk the fragment ranges stored on `InlineWrapLine` and consult the `ShapedLine`’s decoration runs (identical to how `LineWithInvisibles` does it).

---

## 🔧 Supporting Utilities

1. **Fragment iterators**  
   Utility methods to iterate fragments with their layout counterparts:
   - `InlineContent::fragments()` yields `(InlineFragment, InlineFragmentLayoutRef)`.
   - `InlineWrapLine::fragments()` yields the subset for a specific visual line.

2. **Caret helpers**  
   Implement `InlineWrapLine::x_for_index` and `InlineWrapLine::index_for_x` by walking fragment ranges, reusing `ShapedLine::x_for_index` / `index_for_x` for text fragments and the measured widths for element fragments.

3. **Element width feedback loop**  
   - After layout, compare each element fragment’s `measured_size.width` to the previously cached width.
   - When widths change, notify upstream (similar to `editor.update_renderer_widths`) so future layouts start with accurate measurements.

---

## 🖼️ Painting Strategy

1. **Prepaint**  
   - Compute the line origin (`x/y`) for each `InlineWrapLine`.
   - For every fragment on the line:
     - Text fragment: advance `x` by `shaped_line.width`.
     - Element fragment: call `element.prepaint_at` at `line_origin + bounds.origin`, then advance `x` by `measured_size.width`.

2. **Paint**  
   - For text fragments: call `shaped_line.paint` / `paint_background` using the cached `ShapedLine`.
   - For element fragments: translate to their bounds and invoke `element.paint`.
   - Because fragment order is preserved, inline widgets naturally interleave with text.

3. **Cleanup**  
   - Drop per-frame `AnyElement`s after painting unless the builder explicitly retains them.
   - Keep `InlineLayoutState` alive until the next invalidation for hit testing and accessibility.

---

## ✍️ Public Builder API (Fragment-Oriented)

```/dev/null/example_inline_span.rs#L1-40
pub struct InlineSpanBuilder {
    fragments: Vec<InlineFragment>,
    default_font: Font,
    default_font_size: Pixels,
}

impl InlineSpanBuilder {
    pub fn new(font: Font, font_size: Pixels) -> Self { … }

    pub fn text(mut self, text: impl Into<SharedString>, style: &TextStyle) -> Self {
        let fragment = InlineFragment::text(text.into(), style.clone());
        self.merge_or_push(fragment);
        self
    }

    pub fn element<F>(mut self, builder: F) -> Self
    where
        F: Fn(InlineElementContext<'_, '_>) -> InlineElementSpec + Send + Sync + 'static,
    {
        self.fragments.push(InlineFragment::element(builder));
        self
    }

    pub fn build(self) -> InlineSpan {
        InlineSpan::new(InlineContent {
            fragments: self.fragments,
            default_font: self.default_font,
            default_font_size: self.default_font_size,
        })
    }

    fn merge_or_push(&mut self, fragment: InlineFragment) { … }
}
```

---

## ✅ Risk / Mitigation Checklist

| Risk | Mitigation |
|------|------------|
| Fragment lifetime issues | `InlineContent` owns all fragment strings; layout state only borrows via indices. |
| Hit testing drift | Caret helpers walk fragment ranges; no placeholder offset math needed. |
| Element width drift | Same feedback loop as the editor (`update_renderer_widths`-style). |
| Excess allocations | Coalesce adjacent text fragments with identical styles; reuse `SharedString`s when possible. |
| Lost indentation info | Wrapping still reports `boundary.next_indent`; if the parent container enforces zero indent (inline spans), document that expectation. |
| Layout/paint divergence | Cached `InlineFragmentLayout`s ensure paint reuses the exact shaped data produced during layout. |

---

## 🗺️ Implementation Steps & Estimates

1. **Fragment data structures (2h)**  
   Define `InlineFragment`, `InlineContent`, `InlineElementSlot`, builder API, and coalescing logic.

2. **Measurement + shaping (3h)**  
   Implement per-fragment measurement (`InlineElementSpec`) and text shaping, caching results in `InlineFragmentLayout`.

3. **Wrapping pipeline (3h)**  
   Convert fragment lists into `Vec<LineFragment>`, run `LineWrapper`, build `InlineWrapLine`s with fragment ranges.

4. **Element trait integration (3h)**  
   Implement `request_layout`, `prepaint`, `paint`, caret helpers, and element width feedback.

5. **Tests & examples (2h)**  
   Cover multi-fragment lines, element width updates, hit testing, and mixed text/element painting.

_Total: ~13 hours._

### 🔜 Immediate Next Step
- Implement the per-fragment measurement and shaping pass inside `InlineSpan::request_layout`, caching `ShapedLine` and `InlineElementInstance` data in `InlineFragmentLayout`.
  - Reference `crates/gpui/src/elements/inline_span.rs` for the struct scaffolding awaiting this logic.
  - Mirror the measurement/shaping flow from `LineWithInvisibles::from_chunks` in `crates/editor/src/element.rs#L8054-8273`.
  - Use `window.text_system().shape_line` and `window.text_system().line_wrapper(...)` as defined in `crates/gpui/src/text_system.rs`.
  - Follow the wrapping fragment contract in `crates/gpui/src/text_system/line_wrapper.rs` when building `Vec<LineFragment<'_>>`.

### 📚 Reference Index
- `docs/inline_plan.md` — overall fragment-based strategy and lifetime ownership model.
- `crates/gpui/src/elements/inline_span.rs` — data structures awaiting the measurement, wrapping, and Element logic.
- `crates/gpui/src/text_system/line.rs` — `ShapedLine` API for widths, caret math, and painting.
- `crates/gpui/src/text_system.rs` — acquiring `LineWrapper` handles plus font metrics helpers.
- `crates/editor/src/element.rs#L8054-8273` — `LineWithInvisibles::from_chunks`, the canonical measurement/shaping reference.
- `crates/gpui/src/text_system/line_wrapper.rs` — `LineFragment` contract and wrapping behavior reused here.

---

## 📌 Result

By mirroring the fragment-based approach already proven by the editor’s `LineWithInvisibles`, the inline span element avoids placeholder bookkeeping, keeps caret math precise, and still reuses `LineWrapper` for wrapping. Every stage—from measurement to paint—shares the same cached fragment data, ensuring correctness while staying aligned with GPUI’s rendering primitives.