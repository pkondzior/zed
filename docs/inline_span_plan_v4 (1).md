# 🚀 INLINE SPAN — FRAGMENT-BASED IMPLEMENTATION PLAN (v4, cell-based)

## 🎯 Goals

1. Render inline spans that mix **styled text** and **interactive GPUI elements** within the same wrapped lines.
2. Reuse the existing `LineWrapper` / `LineFragment` logic to get **proper wrapping** of:
   - raw text (`LineFragment::Text`),
   - inline elements with fixed widths (`LineFragment::Element`).
3. Preserve precise **hit testing**, **selection**, and **painting** by caching:
   - line-level shaped text (`ShapedLine` per visual line segment),
   - inline element geometry (`InlineElementInstance`) in a layout state owned by the element.
4. Make **logical indices** match text “cells” (grapheme-ish units), while allowing an initial implementation that simply treats each Unicode scalar as one cell.

> **Key invariant:**  
> All `logical_*` fields in this document are in **text cell units** (not bytes, not raw `char`s).  
> Initially those cells may be implemented as “one scalar per cell”, but the APIs are designed so we can switch to real grapheme/TextCell segmentation later without changing the public surface.

---

## 🔜 Assumptions & Prerequisites

1. **Fragment ownership contract**

   - `InlineSpan` owns an immutable sequence of **fragments** (`InlineFragment::Text` or `InlineFragment::Element`).
   - Text fragments:
     - hold their own `SharedString` plus `Vec<TextRun>` (style information),
     - are *not* shaped up front.
   - Element fragments:
     - hold a builder closure that can produce an `InlineElementSpec` (how to measure & paint the inline element).

2. **Per-fragment measurement (elements only)**

   - Before wrapping, every **element fragment** must be measured so its `width`, `height`, and `baseline_offset` are known.
   - This is done via a builder:
     - `InlineElementSlot` → `InlineElementContext` → `InlineElementSpec`,
     - `InlineSpan` then calls `layout_as_root` (or equivalent) using the spec’s `requested_space`; measured geometry never lives in the spec itself.

3. **Shaping happens _after_ wrapping, per visual line**

   - `LineWrapper` works on raw text and element widths, using per-character width caching and returning **byte offsets** (`Boundary.ix`) into the concatenated text.
   - We first:
     - feed `LineWrapper` a list of `LineFragment::Text` and `LineFragment::Element` to compute wrap boundaries,
   - then:
     - for each visual line, convert those byte boundaries to **cell ranges** per fragment,
     - call the text system (`layout_line` / `shape_line`) to get `ShapedLine` per line segment,
     - compute line ascent/descent/baseline from shaped text + elements.

4. **Single-owner layout state**

   - `InlineSpan` has:
     - immutable `InlineContent` (fragments, default font/size),
     - mutable `InlineLayoutState` (lines, shaped segments, element instances, measured size, last wrap width).
   - The layout state is the **only** place that holds cached per-frame/per-layout data.

5. **Cell abstraction and transitional implementation**

   - Each text fragment is associated with a sequence of **text cells**; each cell has a byte range within the fragment.
   - In the **first implementation**, a “cell” can be:
     - one Unicode scalar (`char_indices()`), i.e. one scalar == one cell.
   - Later, this can be swapped to:
     - the real text system’s `TextCell` / grapheme segmentation, without changing `InlineSpan`’s external behavior.

---

## 🧱 Core Data Structures

```rust
const ELEMENT_LEN_UTF8: usize = 1;
```

```rust

```


```rust

```

### 1. Public element & content

```rust
pub struct InlineSpan {
    content: InlineContent,
    layout_state: InlineLayoutState,
}

pub struct InlineContent {
    fragments: Vec<InlineFragment>,
    default_font: Font,
    default_font_size: Pixels,
}
```

```rust
pub enum InlineFragment {
    Text {
        text: SharedString,
        runs: Vec<TextRun>, // style info for this fragment
    },
    Element {
        slot: InlineElementSlot,
    },
}
```

### 2. Inline element builders

```rust
pub struct InlineElementSlot {
    pub builder: Arc<
        dyn Send
            + Sync
            + for<'a, 'w> Fn(
                &'w mut Window,
                &'a mut Context<InlineSpan>,
                InlineElementContext,
            ) -> InlineElementSpec,
    >,
}

pub struct InlineElementContext {
    pub max_width: Pixels,
    pub line_height: Pixels,
    pub font: Font,
    pub font_size: Pixels,
}

pub struct InlineElementSpec {
    pub element: AnyElement,
    pub requested_space: Size<AvailableSpace>,
    pub baseline_offset: Option<Pixels>, // distance from element top to its baseline
    pub constrain_width: bool,
}
```

### 3. Layout-time structures

```rust
pub enum InlineFragmentLayout {
    Text, // no ShapedLine here; shaping is per line
    Element {
        instance: InlineElementInstance,
    },
}

pub struct InlineElementInstance {
    pub element: AnyElement,
    pub size: Size<Pixels>,
    pub bounds: Bounds<Pixels>,
    pub baseline_offset: Pixels, // same semantics as in spec, but resolved
}
```

Per visual line:

```rust
pub struct InlineWrapLine {
    // Which bits of which fragments form this visual line, in cell space
    pub logical_slices: Vec<LogicalSlice>,
    // Shaped content for this visual line
    pub shaped_segments: Vec<ShapedSegment>,
    // Geometry
    pub width: Pixels,
    pub height: Pixels,
    pub baseline_above_top: Pixels,
    pub line_origin: Point<Pixels>,
}

pub struct LogicalSlice {
    pub fragment_index: usize,
    pub start_in_fragment: usize, // byte offset
    pub end_in_fragment: usize,   // byte offset (exclusive)
    pub logical_start: usize,     // cell index in the global logical stream
    pub logical_end: usize,       // cell index (exclusive)
}
```

Fragment-level mapping between logical cells and bytes:

```rust
pub struct FragmentLogicalRange {
    pub fragment_index: usize,
    pub logical_start: usize, // starting cell index in global stream
    pub logical_end: usize,   // exclusive
}

pub struct TextFragmentCells {
    // offsets[k] = byte offset of cell boundary k inside this fragment
    // length == number_of_cells + 1
    pub offsets: Vec<usize>,
}
```

Per-line shaped segments:

```rust
pub enum ShapedSegment {
    Text {
        fragment_index: usize,
        start_in_fragment: usize,
        end_in_fragment: usize,
        shaped: ShapedLine,
    },
    Element {
        fragment_index: usize,
    },
}
```

Global layout state:

```rust
pub struct InlineLayoutState {
    pub lines: Vec<InlineWrapLine>,
    pub fragment_layouts: Vec<InlineFragmentLayout>,
    pub measured_size: Size<Pixels>,
    pub last_wrap_width: Option<Pixels>,
    pub dirty: bool,
}
```

> **Invariant:**  
> `logical_*` fields are always in **cell units**. The internal choice of “what a cell is” (char vs grapheme) is hidden behind `TextFragmentCells`.

---

## 🔁 Layout Pipeline (Fragment-Based)

### 1. Resolve available width & line height

In `Element::layout` for `InlineSpan`:

- From `SizeConstraint`:
  - Resolve wrap width (`wrap_width: Pixels`):
    - If `known_dimensions.width` is definite: use that.
    - Otherwise: treat width as unconstrained or use a parent/default (similar to `StyledText`).
  - Compute `line_height` from text style (font metrics + line spacing).

Track wrap width changes:

```rust
if self.layout_state.last_wrap_width != Some(wrap_width) {
    self.layout_state.last_wrap_width = Some(wrap_width);
    self.layout_state.dirty = true;
}

if self.layout_state.dirty {
    self.rewrap_and_shape(cx, wrap_width);
}

self.layout_state.measured_size
```

### 2. Measure element fragments

Same as before:

- Iterate `InlineContent.fragments` in order.

For each `InlineFragment::Element { slot }`:

```rust
let inline_ctx = InlineElementContext {
    max_width: wrap_width,
    line_height,
    font: content.default_font.clone(),
    font_size: content.default_font_size,
};

let spec = (slot.builder)(window, cx, inline_ctx);
let measured_size = layout_as_root(&spec.element, spec.requested_space);

let baseline_offset = spec
    .baseline_offset
    .unwrap_or(measured_size.height);

layout_state.fragment_layouts.push(
    InlineFragmentLayout::Element {
        instance: InlineElementInstance {
            element: spec.element,
            size: measured_size,
            bounds: Bounds::zero(),
            baseline_offset,
        },
    },
);
```

For each `InlineFragment::Text { .. }`:

```rust
layout_state.fragment_layouts.push(InlineFragmentLayout::Text);
```

(no shaping yet).

### 3. Build `LineFragment` list and cell mapping (no shaping yet)

Goals:

- Provide `LineWrapper` with:
  - `LineFragment::Text` for raw text,
  - `LineFragment::Element` for fixed-width inline boxes.
- Build fragment-level maps from **cells → byte offsets**.

```rust
let mut line_fragments: Vec<LineFragment<'_>> = Vec::new();
let mut fragment_logical_ranges: Vec<FragmentLogicalRange> = Vec::new();
let mut text_fragment_cells: Vec<Option<TextFragmentCells>> = Vec::new();

let mut logical_index = 0; // global cell index (text cells + element slots)

for (frag_index, frag) in content.fragments.iter().enumerate() {
    match frag {
        InlineFragment::Text { text, .. } => {
            let text_str = text.as_ref();
            line_fragments.push(LineFragment::text(text_str));

            let logical_start = logical_index;

            // --- Cell segmentation abstraction ---
            //
            // PSEUDOCODE: use the same cell iterator as the text system.
            // Transitional impl: a "cell" is just a single char,
            // so we can implement this as char_indices() for now.

            let mut offsets = Vec::new();
            offsets.push(0); // boundary before first cell

            // Transitional implementation (1 char => 1 cell):
            for (byte_offset, ch) in text_str.char_indices() {
                offsets.push(byte_offset + ch.len_utf8());
                logical_index += 1; // 1 cell per scalar
            }

            // Later, replace loop above with:
            //
            // let cells = text_system.cells_for_text(text_str);
            // for cell in cells {
            //     offsets.push(cell.byte_end);
            //     logical_index += 1; // 1 cell per grapheme/TextCell
            // }

            fragment_logical_ranges.push(FragmentLogicalRange {
                fragment_index: frag_index,
                logical_start,
                logical_end: logical_index,
            });

            text_fragment_cells.push(Some(TextFragmentCells { offsets }));
        }

        InlineFragment::Element { .. } => {
            let width = match &layout_state.fragment_layouts[frag_index] {
                InlineFragmentLayout::Element { instance } => instance.size.width,
                _ => unreachable!(),
            };

            // Each element is treated as 1 logical cell.
            // LineWrapper expects len_utf8 to be a byte count; we use ELEMENT_LEN_UTF8
            // (synthetic) and never slice text with it.
            line_fragments.push(LineFragment::element(width, ELEMENT_LEN_UTF8 as u32));

            let logical_start = logical_index;
            logical_index += 1;

            fragment_logical_ranges.push(FragmentLogicalRange {
                fragment_index: frag_index,
                logical_start,
                logical_end: logical_index,
            });

            text_fragment_cells.push(None);
        }
    }
}
```

> **Important:**  
> `LineWrapper`’s `Boundary.ix` is still a **byte index** into the concatenated string (`line`), just like in `WrapMap`.  
> The cell mapping (`FragmentLogicalRange` + `TextFragmentCells`) is how we map those byte offsets back into **cell indices** per fragment.

### 4. Run `LineWrapper` and collect byte boundaries

```rust
let mut wrapper: LineWrapper = /* from text_system */;
let boundaries: Vec<Boundary> =
    wrapper.wrap_line(&line_fragments, wrap_width).collect();
```

Where:

```rust
pub struct Boundary {
    pub ix: usize,        // byte offset in the concatenated line
    pub next_indent: u32, // number of spaces to indent continuation line
}
```

This matches `WrapMap`: `ix` is a byte offset, not a cell index.

### 5. Build line byte ranges, then map to cell slices

Conceptually, the text we give to `LineWrapper` is a concatenation of:

- all `InlineFragment::Text.text` strings,
- plus synthetic bytes for `LineFragment::Element` (their `len_utf8`).

In practice we don’t materialize the concatenation; we track:

- how many bytes each fragment contributes,
- and maintain a running `global_byte_offset` as we walk fragments.

We then turn:

- a line’s byte range `[start_byte, end_byte)` into:
  - per-fragment **byte slices**,
  - and per-fragment **cell ranges**.

Algorithm sketch:

```rust
fn compute_logical_slices_for_line(
    byte_start: usize,
    byte_end: usize,
    content: &InlineContent,
    ranges: &[FragmentLogicalRange],
    cells: &[Option<TextFragmentCells>],
) -> Vec<LogicalSlice> {
    let mut slices = Vec::new();

    // Track cumulative bytes as we walk fragments in order
    let mut frag_global_byte_start = 0;

    for range in ranges {
        let frag_index = range.fragment_index;
        let frag = &content.fragments[frag_index];

        let frag_byte_len = match frag {
            InlineFragment::Text { text, .. } => text.as_ref().len(),
            InlineFragment::Element { .. } => ELEMENT_LEN_UTF8, // elements contribute ELEMENT_LEN_UTF8 bytes
        };

        let frag_global_byte_end = frag_global_byte_start + frag_byte_len;

        // Does [byte_start, byte_end) intersect this fragment's byte window?
        let intersect_start = byte_start.max(frag_global_byte_start);
        let intersect_end = byte_end.min(frag_global_byte_end);

        if intersect_start < intersect_end {
            match frag {
                InlineFragment::Text { .. } => {
                    // Local byte range inside this fragment
                    let local_byte_start = intersect_start - frag_global_byte_start;
                    let local_byte_end = intersect_end - frag_global_byte_start;

                    // Map local bytes to cell indices using offsets.
                    let cell_info = cells[frag_index]
                        .as_ref()
                        .expect("expected text cells for text fragment");
                    let offsets = &cell_info.offsets;

                    // Find first cell whose boundary >= local_byte_start
                    let local_cell_start = offsets
                        .binary_search(&local_byte_start)
                        .unwrap_or_else(|ix| ix.saturating_sub(1));

                    // Find first boundary > local_byte_end, subtract 1
                    let local_cell_end = offsets
                        .binary_search(&local_byte_end)
                        .unwrap_or_else(|ix| ix);

                    let logical_start = range.logical_start + local_cell_start;
                    let logical_end = range.logical_start + local_cell_end;

                    slices.push(LogicalSlice {
                        fragment_index: frag_index,
                        start_in_fragment: local_byte_start,
                        end_in_fragment: local_byte_end,
                        logical_start,
                        logical_end,
                    });
                }
                InlineFragment::Element { .. } => {
                    // Elements are handled in cell space only; we don't slice bytes here.
                    let logical_start = range.logical_start;
                    let logical_end = range.logical_end; // always logical_start + 1

                    slices.push(LogicalSlice {
                        fragment_index: frag_index,
                        start_in_fragment: 0,
                        end_in_fragment: 0,
                        logical_start,
                        logical_end,
                    });
                }
            }
        }

        frag_global_byte_start = frag_global_byte_end;
    }

    slices
}
```

Building lines from `boundaries`:

```rust
let mut lines = Vec::new();

let mut prev_byte_ix = 0;
for boundary in &boundaries {
    let byte_ix = boundary.ix;
    let logical_slices = compute_logical_slices_for_line(
        prev_byte_ix,
        byte_ix,
        &content,
        &fragment_logical_ranges,
        &text_fragment_cells,
    );

    // later: shape these slices into ShapedSegments and push InlineWrapLine
    // (see next step)

    prev_byte_ix = byte_ix;
}

// Trailing text after last boundary
let trailing_slices = compute_logical_slices_for_line(
    prev_byte_ix,
    /* total_line_len_bytes */,
    &content,
    &fragment_logical_ranges,
    &text_fragment_cells,
);
```

### 6. Shape text per line and build `InlineWrapLine`s

For each visual line:

1. Get `logical_slices`.
2. For each slice:
   - If text:
     - Slice `&str` by `start_in_fragment` / `end_in_fragment`.
     - Slice runs (`Vec<TextRun>`) for that byte range.
     - Call `text_system.layout_line(text_slice, font_size, &runs)`.
     - Push `ShapedSegment::Text { .. }`.
   - If element:
     - Push `ShapedSegment::Element { fragment_index }`.

3. Compute width and baseline as before:

```rust
let mut x = px(0.0);
let mut max_ascent = px(0.0);
let mut max_descent = px(0.0);

for seg in &shaped_segments {
    match seg {
        ShapedSegment::Text { shaped, .. } => {
            let width = shaped.width;
            let ascent = shaped.ascent;
            let descent = shaped.descent;

            max_ascent = max_ascent.max(ascent);
            max_descent = max_descent.max(descent);
            x += width;
        }
        ShapedSegment::Element { fragment_index } => {
            if let InlineFragmentLayout::Element { instance } =
                &layout_state.fragment_layouts[*fragment_index]
            {
                let height = instance.size.height;
                let baseline_above = instance.baseline_offset;
                let ascent = baseline_above;
                let descent = height - baseline_above;

                max_ascent = max_ascent.max(ascent);
                max_descent = max_descent.max(descent);
                x += instance.size.width;
            }
        }
    }
}

let line_width = x;
let line_height = max_ascent + max_descent;
let baseline_above_top = max_ascent;
```

4. Push line:

```rust
layout_state.lines.push(InlineWrapLine {
    logical_slices,
    shaped_segments,
    width: line_width,
    height: line_height,
    baseline_above_top,
    line_origin: Point::zero(),
});
```

### 7. Stack lines & compute `measured_size`

Same as v3:

```rust
let mut y = px(0.0);
let mut max_width = px(0.0);

for line in &mut layout_state.lines {
    line.line_origin.y = y;
    max_width = max_width.max(line.width);
    y += line.height;
}

layout_state.measured_size = Size {
    width: max_width,
    height: y,
};
```

---

## ✂️ Text Runs, Hit Testing & Decorations

### 1. Run slicing per line via `LogicalSlice`

- Each text fragment owns `Vec<TextRun>`.
- Each line’s `logical_slices` says:
  - which fragment(s),
  - which byte ranges,
  - which **cell ranges** (`logical_start` / `logical_end`) are present.

Before calling `layout_line`, we build the run sublist for `[start_in_fragment, end_in_fragment)` (same logic as editor: trim runs by bytes).

### 2. Hit testing / caret movement (cell-based)

For each `InlineWrapLine` we can expose:

```rust
impl InlineWrapLine {
    pub fn x_for_index(&self, logical_index: usize) -> Pixels { /* ... */ }
    pub fn index_for_x(&self, x: Pixels) -> usize { /* ... */ }
}
```

Implementation sketch:

- `x_for_index(logical_index)`:
  1. Walk `logical_slices` in order until you find the slice with `logical_start <= idx < logical_end`.
  2. Compute `local_cell_index = idx - slice.logical_start`.
  3. Walk `shaped_segments` in order, maintaining `x`:
     - For `ShapedSegment::Text` that corresponds to this slice:
       - Convert `local_cell_index` into the index `ShapedLine` expects (cells) and call `shaped.x_for_index(local_cell_index) + x`.
     - For `ShapedSegment::Element`:
       - If `idx` falls into that element’s `logical_*` range, return:
         - `x` (left edge) or `x + width` depending on bias.
- `index_for_x(x)`:
  1. Walk `shaped_segments`, summing widths.
  2. For text segments: call `shaped.index_for_x(local_x)` to get a **cell index**, then map back to `logical_index` using the slice’s `logical_start`.
  3. For elements: if `x` falls into element box, return its logical index.

> Because `logical_*` are cell indices, this stays consistent with `ShapedLine` as soon as the cell iterator is real.  
> In the transitional implementation, “cell” == “char”, which is still consistent (just not grapheme-correct).

### 3. Selections & decorations

- A selection is `[start_index, end_index)` in **cell units**.
- For each line:
  - Compute overlap with `[line_start_cell, line_end_cell)`.
  - For text segments:
    - Convert selection range to local cell indices and let `ShapedLine` compute decoration rects.
  - For element segments:
    - If the selection covers the element’s logical cell, decide whether to highlight its box.

---

## 🔧 Supporting Utilities

These are helper functions using the **cell abstraction**:

```rust
fn build_line_fragments(
    content: &InlineContent,
    layout_state: &InlineLayoutState,
    text_system: &TextSystem,
) -> (
    Vec<LineFragment<'_>>,
    Vec<FragmentLogicalRange>,
    Vec<Option<TextFragmentCells>>,
);
```

- Builds:
  - `LineFragment::Text` / `LineFragment::Element`,
  - `FragmentLogicalRange` (in cell units),
  - `TextFragmentCells.offsets` (cell boundaries → bytes).
- Transitional impl: cells from `char_indices`; later: from `text_system.cells_for_text`.

```rust
fn compute_logical_slices_for_line(
    byte_start: usize,
    byte_end: usize,
    content: &InlineContent,
    ranges: &[FragmentLogicalRange],
    cells: &[Option<TextFragmentCells>],
) -> Vec<LogicalSlice>;
```

```rust
fn slice_runs_for_range(
    runs: &[TextRun],
    text: &str,
    start: usize,
    end: usize,
) -> Vec<TextRun>;
```

```rust
fn baseline_for_element(instance: &InlineElementInstance) -> (Pixels /* ascent */, Pixels /* descent */);
```

---

## 🖼️ Painting Strategy

Same as v3, just restated briefly:

1. **Pre-position elements during layout**

   - For each line, compute:

     ```rust
     let baseline_y = line.line_origin.y + line.baseline_above_top;
     let mut x = line.line_origin.x;

     for seg in &line.shaped_segments {
         match seg {
             ShapedSegment::Text { shaped, .. } => {
                 // text is painted at (x, baseline_y)
                 x += shaped.width;
             }
             ShapedSegment::Element { fragment_index } => {
                 if let InlineFragmentLayout::Element { instance } =
                     &mut layout_state.fragment_layouts[*fragment_index]
                 {
                     let top_y = baseline_y - instance.baseline_offset;
                     instance.bounds.origin = Point { x, y: top_y };
                     x += instance.size.width;
                 }
             }
         }
     }
     ```

2. **Paint**

   ```rust
   fn paint(&mut self, origin: Point<Pixels>, size: Size<Pixels>, scene: &mut Scene) {
       for line in &self.layout_state.lines {
           let line_origin = origin + line.line_origin;
           let baseline_y = line_origin.y + line.baseline_above_top;
           let mut x = line_origin.x;

           for seg in &line.shaped_segments {
               match seg {
                   ShapedSegment::Text { shaped, .. } => {
                       shaped.paint(line_origin + Point::new(x, 0.0), baseline_y, scene);
                       x += shaped.width;
                   }
                   ShapedSegment::Element { fragment_index } => {
                       if let InlineFragmentLayout::Element { instance } =
                           &self.layout_state.fragment_layouts[*fragment_index]
                       {
                           let elem_origin = origin + instance.bounds.origin;
                           instance.element.paint(elem_origin, instance.size, scene);
                       }
                   }
               }
           }
       }
   }
   ```

3. **Cleanup**

   - `InlineLayoutState` persists until invalidation (`dirty`).
   - When content or style changes:
     - set `dirty = true`,
     - optionally clear `lines` / `fragment_layouts` if needed.

---

## ✍️ Public Builder API

```rust
pub struct InlineSpanBuilder {
    fragments: Vec<InlineFragment>,
    default_font: Font,
    default_font_size: Pixels,
}

impl InlineSpanBuilder {
    pub fn new(font: Font, font_size: Pixels) -> Self { /* ... */ }

    pub fn text(mut self, text: impl Into<SharedString>, style: TextStyle) -> Self {
        let fragment = InlineFragment::Text {
            text: text.into(),
            runs: style.to_runs(),
        };
        self.merge_or_push_text(fragment);
        self
    }

    pub fn element<F>(mut self, builder: F) -> Self
    where
        F: for<'a, 'w> Fn(
                &'w mut Window,
                &'a mut Context<InlineSpan>,
                InlineElementContext,
            ) -> InlineElementSpec
            + Send
            + Sync
            + 'static,
    {
        self.fragments.push(InlineFragment::Element {
            slot: InlineElementSlot {
                builder: Arc::new(builder),
            },
        });
        self
    }

    pub fn build(self) -> InlineSpan {
        InlineSpan {
            content: InlineContent {
                fragments: self.fragments,
                default_font: self.default_font,
                default_font_size: self.default_font_size,
            },
            layout_state: InlineLayoutState {
                lines: Vec::new(),
                fragment_layouts: Vec::new(),
                measured_size: Size::zero(),
                last_wrap_width: None,
                dirty: true,
            },
        }
    }

    fn merge_or_push_text(&mut self, fragment: InlineFragment) {
        // coalesce adjacent Text fragments with identical style
    }
}
```

Usage example:

```rust
let span = InlineSpanBuilder::new(default_font, px(14.))
    .text("Press ", style_normal)
    .element(|window, cx, inline_ctx| build_kbd_inline(window, cx, inline_ctx, "Esc"))
    .text(" to cancel", style_normal)
    .build();
```

And place it in a `div`:

```rust
div()
    .w(px(200))
    .inline_block(
        InlineSpanBuilder::new(default_font, px(14.))
            .text("Press ", style_normal)
            .element(|w, cx, ctx| build_kbd_inline(w, cx, ctx, "Esc"))
            .text(" to cancel", style_normal),
    );
```

(where `inline_block` accepts `Into<InlineSpan>` and just `child()`s it).

---

## 📌 Result

- Logical indices are **text cells**, not raw scalars or bytes.
- The implementation can initially use “one scalar == one cell” without changing APIs.
- `LineWrapper` is used as-is (byte-based), and we map its byte `Boundary.ix` back to per-fragment cell slices via `TextFragmentCells`.
- Inline elements participate in wrapping like text, align on the baseline, and can be hit-tested in the same logical index space as text cells.
- When you later hook into a real `TextCell`/grapheme iterator, caret navigation, selection, and wrapping behavior become grapheme-correct without redesigning `InlineSpan`.
