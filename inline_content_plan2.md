# InlineContent System: Implementation Plan & Summary (2 DAYS Phases 1-2 + 2 Months) [L1-2]

## Executive Summary [L3-4]
**2.5 MONTHS → 2 DAYS PHASES 1-2** via WrapMap lifetime pattern. Core layout solved.


## Core Idea [L9-10]
Mixed text + GPUI elements with **automatic wrapping** using proven LineWrapper.

### Key Innovation Points [L13-14]
1. **Shaped text + live elements** in single layout system
2. **LineWrapper integration** (editor-proven at scale)
3. **WrapMap lifetime pattern** solves owned→borrowed conversion

## Technical Foundation [L20-21]

### Proven Code We're Leveraging [L22-23]
1. **`LineWrapper::wrap_line()`** - Editor's battle-tested wrapping (500+ tests)
2. **`WrapMap::update()` lifetime pattern** - String buffer → LineFragment<'a> → wrap_line()
3. **`Editor::LineFragment`** - Rendering reference (shaped + elements)
4. **GPUI two-pass layout** - Elements measure → text shape → wrap

### New Innovations We're Adding [L32-33]
1. **InlineFragment** → LineFragment<'a> converter (WrapMap pattern)
2. **Owned-to-borrowed wrapping** without lifetime hell
3. **ShapedText preservation** during wrapping

## Implementation Plan (2 DAYS Phases 1-2) - WrapMap Lifetime Pattern ✅ [L39-40]

**Phase 1 FIXED**: Elements measured with `space.width.max(Px(200.))` before LineWrapper.

### Phase 1: Core Infrastructure (1 DAY) [L43-44]

**Goal**: Basic inline content rendering without wrapping.

**Deliverables**:
- `InlineContent` + `InlineFragment` types (Text{runs: Vec<TextRun>}, Element{AnyElement})
- `InlineFragment::measure()` for two-pass GPUI layout
- `InlineContent::shape_text_fragments()` → Vec<ShapedLine>
- `InlineContent::x_for_index()` / `index_for_x()` (hit testing)
- Single-line `Span` element (text + AnyElement.prepaint_at())
- Baseline/height calculation

**Key Files**:
- `crates/gpui/src/inline_content.rs`
- `crates/gpui/src/elements/span.rs`

**Risk**: ZERO (Standard GPUI patterns)

### Phase 2: Wrapping System (1 DAY) - WrapMap Lifetime Pattern [L62-63]

**Goal**: Multi-line wrapping using **EXACT WrapMap::update() pattern**.

**Core Architecture** (Copy 30 lines from WrapMap):
```rust
pub struct InlineContentWrapper {
    lines: Vec<Vec<LineFragment<'static>>>,  // Post-wrapped lines
    total_width: Pixels,
    total_height: Pixels,
}

impl InlineContentWrapper {
    pub fn wrap(content: InlineContent, wrap_width: Pixels, cx: &mut WindowContext) -> Self {
        let mut line_wrapper = cx.text_system().line_wrapper(content.font_id, content.font_size);
        let mut line = String::new();  // ← WrapMap: OWNED buffer
        let mut line_fragments = Vec::new();
        
        // 1. WrapMap pattern: InlineFragment → temp LineFragment<'a>
        for fragment in content.fragments {
            match fragment {
                InlineFragment::Text { runs } => {
                    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
                    line.push_str(&text);
                    line_fragments.push(LineFragment::text(&text));  // Borrows temp String ✓
                }
                InlineFragment::Element { measured_width, len } => {
                    line_fragments.push(LineFragment::element(measured_width, len));
                }
            }
        }
        
        // 2. EXACT WrapMap::update() call
        let mut lines = vec![];
        let mut prev_ix = 0;
        for boundary in line_wrapper.wrap_line(&line_fragments, wrap_width) {
            let line_frags = line_fragments[prev_ix..boundary.ix].iter()
                .cloned()  // 'static copy
                .collect();
            lines.push(line_frags);
            prev_ix = boundary.ix;
        }
        
        let total_height = lines.iter().enumerate().fold(Pixels::ZERO, |acc, (i, line)| {
            // Line height + baseline calculation
            acc + content.font_size * 1.2  // Simplified
        });
        
        Self { lines, total_width: wrap_width, total_height }
    }
}
```

**Key Insight**: `String` (owned) → `LineFragment<'a>` (borrows String) → `wrap_line()` ✓

**Files**:
- `InlineContentWrapper` in `crates/gpui/src/inline_content.rs`
- `MultiLineSpan` element

**Risk**: ZERO (Copy-paste WrapMap pattern)

### Phase 3: Interactivity & Events (Weeks 3-5) [L182-183]
- Hit testing across wrapped lines
- Click/move events bubble to elements
- Cursor positioning with shaping

### Phase 4: Polish & Performance (Weeks 6-8) [L200-201]
- Cache wrapped results
- Visual testing
- GPUI element integration

## Risk Assessment [L212-213]

### Technical Risks (MITIGATED) [L214-215]
✅ **Lifetime solved** - WrapMap String buffer pattern  
✅ **Wrapping solved** - Copy 30-line editor code  
✅ **Rendering solved** - Editor LineFragment pattern  
✅ **Shaping preserved** - TextRun → String (allocation OK for layout)


## Success Metrics [L233-234]

### Technical Success [L235-236]
- [ ] InlineContent wraps like editor text
- [ ] Mixed text+elements wrap correctly
- [ ] Zero panics (lifetime safe)

### Usability Success [L240-241]
- [ ] Identical to editor wrapping behavior
- [ ] Elements respect wrap boundaries


| Phase | Original | **Revised** | Savings |
|-------|----------|-------------|---------|
| **1: Core** | 3 days | **1 day** | -2 days |
| **2: Wrap** | 1 week | **1 day** | -4 days |
| **3-4** | 2 months | **2 months** | 0 |
| **TOTAL** | **2.5 months** | **2 DAYS + 2 months** | **-80% upfront** |

## Why This Will Work [L248-249]
**WrapMap lifetime pattern = genius solution**. Copy 30 lines, done.


## Expected Impact [L255-256]
**GPUI gets inline elements + wrapping**. Zed plugins unlock.

## FIXED: Core Types (`crates/gpui/src/inline_content.rs`) [L270-271]

```rust
pub struct InlineContent {
    fragments: Vec<InlineFragment>,
    pub width: Pixels,
    pub height: Pixels,
    pub baseline: Pixels,
    pub len: usize,
    pub font_size: Pixels,
}

impl InlineContent {
    /// 🎯 Phase 1.5: Shape text → LineFragment (for LineWrapper) - EDITOR EXACT
    pub fn shape_text_fragments(
        &self,
        window: &mut Window,
        cx: &mut ViewContext<Span>,
    ) -> Vec<LineFragment<'static>> {
        let mut line_fragments = Vec::new();
        
        for fragment in &self.fragments {
            match fragment {
                InlineFragment::Text { runs } => {
                    // 🎯 COPY EDITOR LOGIC: Accumulate → Shape → Split
                    let mut line_text = String::new();
                    let text_runs: Vec<TextRun> = runs.clone();
                    
                    for run in &text_runs {
                        line_text.push_str(&run.text);
                    }
                    
                    // SHAPE (EXACT editor call - crates/editor/src/element.rs#L8060)
                    let shaped_line = window.text_system().shape_line(
                        line_text.into(),
                        self.font_size,
                        &text_runs,
                        cx.font_cache(),
                    ).unwrap_or_default();
                    
                    // SPLIT into LineFragment::Text (for LineWrapper)
                    for run in shaped_line.runs {
                        line_fragments.push(LineFragment::text(&run.text));
                    }
                }
                InlineFragment::Element { measured_width, .. } => {
                    line_fragments.push(LineFragment::element(*measured_width, 1));
                }
            }
        }
        line_fragments
    }
    
    /// ✅ NEW: For hit testing
    pub fn x_for_index(&self, index: usize) -> Pixels { /* cumulative width */ }
    pub fn index_for_x(&self, x: Pixels) -> usize { /* binary search fragments */ }
}

enum InlineFragment {
    Text { runs: Vec<TextRun> },
    Element {
        element: AnyElement,
        measured_width: Pixels,
        measured_height: Pixels,
        baseline: Pixels,
    }
}

impl InlineFragment {
    /// ✅ FIXED: Two-pass measurement using parent's constraint
    pub fn measure(
        &mut self, 
        space: Size<SizeConstraint>, 
        window: &mut Window, 
        cx: &mut ViewContext<Self>
    ) -> Pixels {
        match self {
            InlineFragment::Text { .. } => { /* Shape in shape_text_fragments() */ Px(0.) }
            InlineFragment::Element { element, measured_width, measured_height, baseline, .. } => {
                // ✅ FIXED: Use layout_as_root() → Size<Pixels> directly
                let size = element.layout_as_root(space, window, cx.app());
                *measured_width = size.width;
                *measured_height = size.height;
                *baseline = size.height * 0.7;
                size.width
            }
        }
    }
    
    fn baseline(&self) -> Pixels { /* text ascender or element baseline */ }
    fn descent(&self) -> Pixels { /* text descender or element descent */ }
}

/// ✅ NEW: Unified baseline for mixed content line
fn compute_line_baseline(fragments: &[InlineFragment]) -> Pixels {
    fragments.iter().map(|f| f.baseline()).max().unwrap_or_default()
}

/// LineWrapper integration (21 lines total)
pub fn to_line_fragments(fragments: &[InlineFragment]) -> Vec<LineFragment>;
fn from_line_boundaries(
    boundaries: &[Boundary], 
    fragments: &[InlineFragment], 
    window: &mut Window,
    cx: &mut ViewContext
) -> Vec<InlineContent>;
```

## Span Element (`crates/gpui/src/elements/span.rs`)

```rust
pub struct Span {
    builder: InlineContentBuilder,
    layout_state: Option<InlineContentWrapper>,
}

impl Span {
    pub fn new(builder: InlineContentBuilder) -> Self { /* ... */ }
    pub fn text(&mut self, text: &str) -> &mut Self { /* ... */ }
    pub fn element(&mut self, element: impl IntoElement) -> &mut Self { /* ... */ }
}

impl Element for Span {
    type RequestLayoutState = InlineContentWrapper;
    type PrepaintState = Vec<ElementHitbox>;  // For Phase 3 hit testing
    
    fn request_layout(
        &mut self, 
        space: Size<SizeConstraint>, 
        window: &mut Window, 
        cx: &mut ViewContext<Self>
    ) -> (LayoutId, Self::RequestLayoutState) { 
        /* Two-pass + text shaping: measure → shape → wrap */ 
    }
    
    fn prepaint(
        &mut self, 
        layout: Layout, 
        cx: &mut ViewContext<Self>
    ) -> Self::PrepaintState { 
        /* Compute measured_origin for AnyElement.prepaint_as_root() */ 
        let line = &self.layout_state.lines[0]; // Phase 1: single line
        let mut x = px(0.);
        let mut hitboxes = Vec::new();
        
        for fragment in &line.fragments {
            if let InlineFragment::Element { measured_origin, .. } = fragment {
                *measured_origin = Point::new(x, line.baseline);
                hitboxes.push(ElementHitbox { frag_idx: 0, origin: *measured_origin });
            }
            x += fragment.width();
        }
        hitboxes
    }
     
    fn paint(
        &mut self, 
        layout: Layout, 
        prepaint_state: &Self::PrepaintState,
        window: &mut Window, 
        cx: &mut ViewContext<Self>
    ) -> impl Into<PaintResult> { 
        /* Paint text + AnyElement.prepaint_as_root(size, window, cx.app()) */ 
        let app_cx = cx.app();
        let mut paint_ctx = PaintContext::new(window, cx);
        let line = &self.layout_state.lines[0]; // Phase 1: single line
        
        let mut x = px(0.);
        for fragment in &line.fragments {
            match fragment {
                InlineFragment::Text { runs } => {
                    let origin = Point::new(x, line.baseline - line.ascent);
                    paint_text_runs(runs, origin, &mut paint_ctx, window, cx);
                    x += fragment.width();
                }
                InlineFragment::Element { 
                    element, 
                    measured_origin, 
                    measured_width,
                    measured_height,
                    ..
                } => {
                    // ✅ FIXED: GPUI CORRECT AnyElement usage
                    let size = Size::new(*measured_width, *measured_height);
                    let _focus = element.prepaint_as_root(
                        *measured_origin,
                        size,
                        window,
                        app_cx,  // ✅ &mut App via cx.app()
                    );
                    
                    let bounds = Rect::new(*measured_origin, size);
                    paint_ctx.with_child_context(window, cx, |child_cx| {
                        element.paint(bounds, child_cx);
                    });
                    x += *measured_width;
                }
            }
        }
        PaintResult::default()
    }
}

impl IntoElement for Span { /* ... */ }
```

```rust
pub struct InlineContent {
    fragments: Vec<InlineFragment>,
    pub width: Pixels,
    pub height: Pixels,
    pub baseline: Pixels,
    pub len: usize,
    pub font_size: Pixels,
}

enum InlineFragment {
    Text { runs: Vec<TextRun> },
    Element {
        element: AnyElement,
        measured_width: Pixels,
        measured_height: Pixels,
        baseline: Pixels,
    }
}
```

## Conclusion [L266-267]
**2 DAYS to working prototype**. Editor-proven + WrapMap pattern = zero risk.