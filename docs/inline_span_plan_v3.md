# 🚀 INLINE SPAN — FRAGMENT-BASED IMPLEMENTATION PLAN (v3)

## 🎯 Goals

1. Render inline spans that mix **styled text** and **interactive GPUI elements** within the same wrapped lines.
2. Reuse the existing `LineWrapper` / `LineFragment` logic to get **proper wrapping** of:
   - raw text (`LineFragment::Text`),
   - inline elements with fixed widths (`LineFragment::Element`).
3. Preserve precise **hit testing**, **selection**, and **painting** by caching:
   - line-level shaped text (`ShapedLine` per visual line segment),
   - inline element geometry (`InlineElementInstance`) in a layout state owned by the element.

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

   - `LineWrapper` works on raw text and element widths, using per-character width caching.
   - We first:
     - feed `LineWrapper` a list of `LineFragment::Text` and `LineFragment::Element` to compute wrap boundaries,
   - then:
     - for each visual line, slice the text ranges,
     - call the text system (`layout_line` / `shape_line`) to get `ShapedLine` per line segment,
     - compute line ascent/descent/baseline from shaped text + elements.

4. **Single-owner layout state**

   - `InlineSpan` has:
     - immutable `InlineContent` (fragments, default font/size),
     - mutable `InlineLayoutState` (lines, shaped segments, element instances, measured size).
   - The layout state is the **only** place that holds cached per-frame/per-layout data.

---

## 🧱 Core Data Structures

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
    // For mapping logical ranges to fragment byte slices
    pub logical_slices: Vec<LogicalSlice>,
    // shaped content for this visual line
    pub shaped_segments: Vec<ShapedSegment>,
    // geometry
    pub width: Pixels,
    pub height: Pixels,
    pub baseline_above_top: Pixels,
    pub line_origin: Point<Pixels>,
}

pub struct LogicalSlice {
    pub fragment_index: usize,
    pub start_in_fragment: usize, // byte offset
    pub end_in_fragment: usize,   // byte offset (exclusive)
    pub logical_start: usize,
    pub logical_end: usize,
}

pub struct FragmentLogicalRange {
    pub fragment_index: usize,
    pub logical_start: usize,
    pub logical_end: usize,
}

pub struct TextFragmentCells {
    // offsets[k] = byte offset of logical boundary k inside this fragment
    // length == number_of_chars + 1
    pub offsets: Vec<usize>,
}
```

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
    pub dirty: bool,
}
```

---

## 🔁 Layout Pipeline (Fragment-Based)

### 1. Resolve available width & line height

- From `SizeConstraint` in `Element::layout`:
  - Resolve wrap width (`wrap_width: Pixels`):
    - If `known_dimensions.width` is definite: use that.
    - Otherwise: use some default/parent width (similar to `StyledText`).
  - Compute `line_height` from the active text style (font metrics + line spacing).

### 2. Measure element fragments

- Iterate `InlineContent.fragments` in order.

For each `InlineFragment::Element { slot }`:

1. Build `InlineElementContext` with:
   - `max_width: wrap_width`,
   - `line_height`, `font`, `font_size`.
2. Call `slot.builder(window, cx, inline_ctx)` → `InlineElementSpec`.  
   The spec only describes the element and its layout preferences; it never contains measured geometry.
3. Call `layout_as_root` (or equivalent) on `spec.element` with `spec.requested_space` to get `let measured_size = ...;` and treat that result as the single source of truth.
4. Resolve `baseline_offset` (fallback if `None`: treat baseline as element bottom).
5. Store:

   ```rust
   layout_state.fragment_layouts.push(
       InlineFragmentLayout::Element {
           instance: InlineElementInstance {
               element: spec.element,
               size: measured_size,
               bounds: Bounds::zero(), // to be filled when lines are positioned
               baseline_offset: spec.baseline_offset.unwrap_or(measured_size.height),
           },
       },
   );
   ```

For each `InlineFragment::Text { .. }` push:

```rust
layout_state.fragment_layouts.push(InlineFragmentLayout::Text);
```

(no shaping here).

### 3. Build logical `LineFragment` list (no shaping yet)

Goals:

- Provide `LineWrapper` with:
  - raw text segments,
  - element widths and their logical “cell” counts.

Algorithm:

```rust
let mut line_fragments: Vec<LineFragment<'_>> = Vec::new();

// per-fragment logical ranges (in global logical index space)
let mut fragment_logical_ranges: Vec<FragmentLogicalRange> = Vec::new();

// per-fragment text cell data (None for elements)
let mut text_fragment_cells: Vec<Option<TextFragmentCells>> = Vec::new();

let mut logical_index = 0; // global logical index (chars + element slots)

for (frag_index, frag) in content.fragments.iter().enumerate() {
    match frag {
        InlineFragment::Text { text, .. } => {
            let text_str = text.as_ref();
            line_fragments.push(LineFragment::text(text_str));

            let logical_start = logical_index;

            // offsets[k] = byte offset of boundary k in this fragment,
            // so len == number_of_chars + 1
            let mut offsets = Vec::new();
            offsets.push(0); // boundary before first char

            for (byte_offset, ch) in text_str.char_indices() {
                offsets.push(byte_offset + ch.len_utf8());
                logical_index += 1;
            }

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

            line_fragments.push(LineFragment::element(width, 1));

            let logical_start = logical_index;
            logical_index += 1;

            fragment_logical_ranges.push(FragmentLogicalRange {
                fragment_index: frag_index,
                logical_start,
                logical_end: logical_index,
            });

            text_fragment_cells.push(None); // elements have no byte offsets
        }
    }
}
```

Key points:

- Every text character contributes one logical index.
- Every element contributes one logical index.
- For text, `offsets` gives you `start_byte = offsets[local_start]`, `end_byte = offsets[local_end]`.

### 4. Run `LineWrapper` to get boundaries

```rust
let mut wrapper: LineWrapper = // acquired from text_system;
let boundaries: Vec<Boundary> =
    wrapper.wrap_line(&line_fragments, wrap_width).collect();
```

- `Boundary { ix, next_indent }` gives you the index of the **last logical “cell”** in each line, where “cell” here means:
  - a Unicode scalar for text,
  - or one synthetic “slot” for each element.

We treat these as indices into the logical stream.

### 5. Convert boundaries into line ranges

Given `boundaries: [b0, b1, b2, ...]`:

- Define line ranges:

  ```rust
  let mut start_ix = 0;
  for boundary in &boundaries {
      let end_ix = boundary.ix; // inclusive
      let line_range = (start_ix, end_ix + 1); // half-open [start, end)
      // process line_range...
      start_ix = end_ix + 1;
  }
  ```

Now, for each `line_range: (start_ix, end_ix)`:

- Intersect `[start_ix, end_ix)` with each `FragmentLogicalRange` and convert that to `LogicalSlice`s using `TextFragmentCells` for text fragments.
- Elements use synthetic byte offsets; their logical spans are what matter.

```rust
fn compute_logical_slices(
    start_ix: usize,
    end_ix: usize,
    content: &InlineContent,
    ranges: &[FragmentLogicalRange],
    cells: &[Option<TextFragmentCells>],
) -> Vec<LogicalSlice> {
    let mut slices = Vec::new();

    for range in ranges {
        let frag_start = range.logical_start;
        let frag_end = range.logical_end;

        // intersect [start_ix, end_ix) with [frag_start, frag_end)
        let line_start = start_ix.max(frag_start);
        let line_end = end_ix.min(frag_end);

        if line_start >= line_end {
            continue;
        }

        let frag_index = range.fragment_index;
        let local_start = line_start - frag_start;
        let local_end = line_end - frag_start;

        match &content.fragments[frag_index] {
            InlineFragment::Text { text, .. } => {
                let cells_for_frag = cells[frag_index]
                    .as_ref()
                    .expect("expected text cells for text fragment");

                let offsets = &cells_for_frag.offsets;
                debug_assert!(local_end < offsets.len());

                let start_byte = offsets[local_start];
                let end_byte = offsets[local_end];

                slices.push(LogicalSlice {
                    fragment_index: frag_index,
                    start_in_fragment: start_byte,
                    end_in_fragment: end_byte,
                    logical_start: line_start,
                    logical_end: line_end,
                });
            }
            InlineFragment::Element { .. } => {
                // byte offsets are synthetic for elements
                slices.push(LogicalSlice {
                    fragment_index: frag_index,
                    start_in_fragment: 0,
                    end_in_fragment: 0,
                    logical_start: line_start,
                    logical_end: line_end,
                });
            }
        }
    }

    slices
}
```

Each `LogicalSlice` now captures all the data the line needs later.

### 6. Shape text per line and build `InlineWrapLine`s

For each computed line:

1. Build `LogicalSlice`s:

   ```rust
   let logical_slices = compute_logical_slices(
       line_start_ix,
       line_end_ix,
       &content,
       &fragment_logical_ranges,
       &text_fragment_cells,
   );
   ```

2. Build `shaped_segments` from `logical_slices`:

   - For each text slice:

     - Extract `&str` slice: `&fragment.text[start_in_fragment..end_in_fragment]`.
     - Slice `Vec<TextRun>` down to that byte range (as in the editor).
     - Call:

       ```rust
       let shaped = text_system.layout_line(text_slice, font_size, &runs);
       ```

     - Push:

       ```rust
       ShapedSegment::Text {
           fragment_index,
           start_in_fragment,
           end_in_fragment,
           shaped,
       }
       ```

   - For each element slice:

     ```rust
     ShapedSegment::Element { fragment_index };
     ```

3. Compute line width and baseline:

   ```rust
   let mut x = px(0.0);
   let mut max_ascent = px(0.0);
   let mut max_descent = px(0.0);

   for seg in &shaped_segments {
       match seg {
           ShapedSegment::Text { shaped, .. } => {
               let width = shaped.width;
               let ascent = shaped.ascent;   // from ShapedLine
               let descent = shaped.descent; // from ShapedLine

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

4. Create and push the line:

   ```rust
   layout_state.lines.push(InlineWrapLine {
       logical_slices,
       shaped_segments,
       width: line_width,
       height: line_height,
       baseline_above_top,
       line_origin: Point::zero(), // filled when we stack lines
   });
   ```

### 7. Stack lines vertically and compute `measured_size`

After all lines are built:

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

Return `layout_state.measured_size` from `Element::layout`.

---

## ✂️ Text Runs, Hit Testing & Decorations

1. **Run slicing is per line using `LogicalSlice`**

   - Each text fragment owns its `Vec<TextRun>`.
   - For each line, `InlineWrapLine.logical_slices` tells you which byte slices of which fragments (and which logical indices) belong on that line.
   - You slice runs accordingly before calling `layout_line`.

2. **Hit testing / caret movement**

   - `InlineWrapLine` can expose:

     ```rust
     fn x_for_index(&self, logical_index: usize) -> Pixels;
     fn index_for_x(&self, x: Pixels) -> usize;
     ```

   - Implementation sketch:
     - Use `self.logical_slices` to find the slice whose `[logical_start, logical_end)` contains the `logical_index`.
     - If it’s a text slice:
       - Convert `logical_index` to a local char index inside that slice.
       - Use `ShapedLine::x_for_index` / `index_for_x` and add the running `x` offset from previous segments.
     - If it’s an element slice:
       - Treat the element as occupying one logical position.
       - If `x` falls inside its width, snap to that logical index.

3. **Selections & decorations**

   - A selection is a `[start_index, end_index)` in the logical index space.
   - For each line:
     - Determine the overlap between the selection range and this line’s logical index range.
     - For each `ShapedSegment::Text` affected:
       - Convert to local indices and let `ShapedLine` compute decoration rects.
     - For elements inside the selection range:
       - Decide policy (highlight the bounding box, treat as atomic, etc.).

---

## 🔧 Supporting Utilities

These are “sketch signatures” – the real ones may take more context, but the shapes are:

```rust
fn build_line_fragments(
    content: &InlineContent,
    layout_state: &InlineLayoutState,
) -> (
    Vec<LineFragment<'_>>,
    Vec<FragmentLogicalRange>,
    Vec<Option<TextFragmentCells>>,
);
```

```rust
fn compute_logical_slices(
    start_ix: usize,
    end_ix: usize,
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
fn baseline_for_element(
    instance: &InlineElementInstance,
) -> (Pixels /* ascent */, Pixels /* descent */);
```

---

## 🖼️ Painting Strategy

1. **Prepaint / layout-time positioning**

   - As you stack lines (in `layout`), compute:
     - `line.line_origin.x` (usually 0),
     - `line.line_origin.y` (stacked vertically),
     - and for each `InlineElementInstance`, update `bounds.origin` based on the line’s baseline:

     ```rust
     let baseline_y = line.line_origin.y + line.baseline_above_top;
     let mut x = line.line_origin.x;

     for seg in &line.shaped_segments {
         match seg {
             ShapedSegment::Text { shaped, .. } => {
                 // text is painted relative to (x, baseline_y)
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

   In `Element::paint(origin, size, scene)`:

   - For each line in `layout_state.lines`:
     - Let `line_origin = origin + line.line_origin`.
     - Let `baseline_y = line_origin.y + line.baseline_above_top`.
     - Maintain a local `x` starting at `line_origin.x`.

     For each segment:

     - `ShapedSegment::Text { shaped, .. }`:
       - Ask `shaped` to paint text and decorations at `(x, baseline_y)` (whatever the text API expects).
       - Increment `x += shaped.width`.
     - `ShapedSegment::Element { fragment_index }`:
       - Use `InlineFragmentLayout::Element.instance.bounds` (already pre-positioned) plus `origin` to call `element.prepaint` / `element.paint`.

3. **Cleanup**

   - `InlineLayoutState` persists until next invalidation (`dirty` set).
   - Inline element instances are kept as long as the layout is valid.
   - When content or style changes, mark `layout_state.dirty = true` and rebuild on next `layout`.

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
    .element(|window, cx, inline_ctx| {
        build_kbd_inline(window, cx, inline_ctx, "Esc")
    })
    .text(" to cancel", style_normal)
    .build();
```

---

## 📌 Result

With this plan:

- `LineWrapper` is used exactly as designed:
  - it sees raw text + element widths (`LineFragment::Text` / `LineFragment::Element`),
  - it returns logical wrap boundaries in a unified index space.
- Shaping is done **per visual line**, giving you:
  - correct glyph positions,
  - correct ascent/descent from `ShapedLine`.
- Inline elements:
  - participate in wrapping as fixed-width atomic boxes,
  - align with text baselines using `baseline_offset`.
- Hit testing and selections:
  - operate on the same logical index space that `LineWrapper` uses,
  - map back to `(fragment_index, byte_range)` and then into per-line `ShapedLine`s.

This should give you a solid `InlineSpan` / `InlineBlock` primitive that behaves like “rich inline text with widgets” while staying aligned with GPUI’s existing text and layout systems.
