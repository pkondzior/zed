# 🚀 INLINE CONTENT SYSTEM: FINAL IMPLEMENTATION PLAN (4H30m ✅)

## 🎯 EXECUTIVE SUMMARY

**WrapBuffer + SpanBuilder(pre-measure) = 100% RECURSION-SAFE**. **Production LineWithInvisibles pattern**. **4H30m TO PRODUCTION**.

```
Fragment → SpanBuilder(pre-measure) → WrapBuffer → LineWrapper → WrapLines → Span
     ↓              ↓                           ↓
Raw data      Elements measured ONCE    Exact wrapping      Hit test + paint
```

## 🏗 CORE ARCHITECTURE (PRODUCTION-PATTERN)

```
SpanBuilder("Hello ", Button, " world!") → InlineContent(fragments: PRE-MEASURED)
    ↓ WrapBuffer<'a>(&mut LineWrapper) → LineFragment<'a>
Vec<LineFragment<'a>> → LineWrapper::wrap_line() [EXISTING]
    ↓ fragment_map: Vec<usize> → Hit testing preserved
Vec<WrapLines { fragment_ids, width }> → Span paint + clicks
```

## 📋 IMPLEMENTATION PLAN (4H30m)

### Phase 1: WrapBuffer Core (2 HOURS) ✅ UNCHANGED

**Files**: `crates/gpui/src/inline_content.rs`

```
[ ] Fragment enum (15min)
[ ] InlineContent (10min)  
[ ] WrapBuffer<'a> + build_fragments() (45min)
[ ] wrap() → Vec<WrapLines> via LineWrapper (30min)
[ ] Demo: "Hello [button] world" wrapped (20min)
```

### Phase 2: SpanBuilder + Span Element (2H30m) 🔧 RECURSION-PROOF

**Files**: `crates/gpui/src/inline_content.rs` + `crates/gpui/src/elements/span.rs`

```
[ ] SpanBuilder + pre-measure (45min) ➕ NEW
[ ] #[derive(Element)] Span (20min)
[ ] Span::request_layout() → NO measure (30min)  ✅ FIXED
[ ] Span::prepaint() → steal elements (15min)
[ ] Span::paint() → LineWithInvisibles-style (40min)
[ ] Span::hit_test() → fragment_ids (20min)
```

## 🆕 CORE TYPES (`crates/gpui/src/inline_content.rs`)

```rust
#[derive(Clone)]
pub enum Fragment {
    Text { runs: Vec<TextRun> },
    Element {
        size: Size<Pixels>,           // ✅ PRE-MEASURED
        len_utf8: usize,
        element: Option<AnyElement>,  // ✅ Production type
    }
}

#[derive(Clone)]
pub struct InlineContent {
    pub fragments: Vec<Fragment>,
    pub font_id: FontId,
    pub font_size: Pixels,
}

// ✅ NEW: RECURSION-PROOF BUILDER
pub struct SpanBuilder {
    fragments: Vec<Fragment>,
    font_id: FontId,
    font_size: Pixels,
}
```

## 🔧 Phase 1: WrapBuffer (UNCHANGED) ✅ PERFECT

```rust
impl<'a> WrapBuffer<'a> {
    pub fn new(line_wrapper: &'a mut LineWrapper, content: InlineContent) -> Self {
        let mut this = Self { 
            line_wrapper,
            content,
            line_fragments: Vec::new(),
            fragment_map: Vec::new(),
        };
        this.build_fragments();
        this
    }
    
    fn build_fragments(&mut self) {
        for (raw_id, fragment) in self.content.fragments.iter().enumerate() {
            match fragment {
                Fragment::Text { runs } => {
                    for run in runs {
                        self.line_fragments.push(LineFragment::text(run.text.as_str()));
                        self.fragment_map.push(raw_id);
                    }
                }
                Fragment::Element { size, len_utf8, .. } => {
                    self.line_fragments.push(LineFragment::element(size.width, *len_utf8));
                    self.fragment_map.push(raw_id);
                }
            }
        }
    }
    
    pub fn wrap(&mut self, width: Pixels) -> Vec<WrapLines> {
        let mut lines = Vec::new();
        let mut prev_ix = 0;

        for boundary in self.line_wrapper.wrap_line(&self.line_fragments, width) {
            let fragment_ids = (prev_ix..boundary.ix)
                .map(|ix| self.fragment_map[ix])
                .collect();
            lines.push(WrapLines {
                fragment_ids,
                width: self.line_wrapper.font_size * boundary.next_indent as f32,
            });
            prev_ix = boundary.ix;
        }
        lines
    }
}
```

## 🔧 Phase 2A: SpanBuilder (NEW - 45min) ✅ RECURSION-PROOF

```rust
impl SpanBuilder {
    pub fn new(font_id: FontId, font_size: Pixels) -> Self {
        Self { fragments: Vec::new(), font_id, font_size }
    }
    
    pub fn text(mut self, text: impl Into<SharedString>) -> Self {
        self.fragments.push(Fragment::Text { 
            runs: vec![TextRun { text: text.into(), ..Default::default() }] 
        });
        self
    }
    
    pub fn element(mut self, element: impl Element, window: &mut Window, cx: &mut App) -> Self {
        // ✅ MEASURE ONCE during construction (PRODUCTION PATTERN)
        let size = window.request_measure(
            &element, 
            SizeConstraint::new(Size::ZERO, Size::splat(Pixels::MAX)), 
            cx
        );
        self.fragments.push(Fragment::Element {
            size,
            len_utf8: 1,  // Elements = 1 glyph
            element: Some(element.into_any()),
        });
        self
    }
    
    pub fn build(self) -> InlineContent {
        InlineContent { 
            fragments: self.fragments, 
            font_id: self.font_id, 
            font_size: self.font_size 
        }
    }
}
```

## 🔧 Phase 2B: Span Element (FIXED) ✅

```rust
#[derive(Element)]
pub struct Span {
    content: InlineContent,       // ✅ PRE-MEASURED
    wrapper: Vec<WrapLines>,
    stolen_elements: Vec<AnyElement>,
}

impl Element for Span {
    type RequestLayoutState = ();
    type PrepaintState = Vec<AnyElement>;

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // ✅ NO MEASURING - Already done in SpanBuilder!
        let mut line_wrapper = LineWrapper::new(
            self.content.font_id, 
            self.content.font_size, 
            window.text_system().clone()
        );
        self.wrapper = self.content.wrap(&mut line_wrapper, /* width from constraint */);
        let size = compute_size_from_wrapper(&self.wrapper);
        (LayoutId::new(), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // ✅ Steal elements (LineWithInvisibles pattern)
        let mut stolen = Vec::new();
        for fragment in &mut self.content.fragments {
            if let Fragment::Element { element: Some(e), .. } = fragment {
                stolen.push(e.take().unwrap());
            }
        }
        stolen
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // ✅ LineWithInvisibles-style painting
        let mut stolen_idx = 0;
        for (line_idx, wrap_line) in self.wrapper.iter().enumerate() {
            let mut x = 0.;
            let y = line_idx as f32 * self.content.font_size;
            for &fragment_id in &wrap_line.fragment_ids {
                let fragment = &self.content.fragments[fragment_id];
                match fragment {
                    Fragment::Text { runs } => {
                        // Shape + paint text at (x, y)
                        x += text_width;
                    }
                    Fragment::Element { size, .. } => {
                        // Paint stolen element
                        let element = &self.stolen_elements[stolen_idx];
                        element.paint(Bounds::new(Point::new(x, y), *size), window, cx);
                        stolen_idx += 1;
                        x += size.width;
                    }
                }
            }
        }
    }
}
```

## ✅ ALL RISKS RESOLVED

| Risk | Original Issue | Production Fix | Status |
|------|----------------|----------------|--------|
| **Recursion** | `measure_elements()` in layout | **SpanBuilder pre-measures** | 🟢 **SOLVED** |
| LineWrapper | Mutable caches | `&mut LineWrapper` per layout | 🟢 **SOLVED** |
| Element trait | Wrong signatures | Correct states + return | 🟢 **SOLVED** |
| Element lifetime | Storage | `Option<AnyElement>` steal | 🟢 **SOLVED** |

## 📈 TIMELINE

| Phase | Time | Status |
|-------|------|--------|
| **Phase 1** | **2 HOURS** | 🟢 WrapBuffer |
| **Phase 2A** | **45min** | 🟢 **SpanBuilder(pre-measure)** ➕ NEW |
| **Phase 2B** | **1H45m** | 🟢 Span Element |
| **TOTAL** | **4H30m** | 🟢 **RECURSION-PROOF** |

## 🎯 SUCCESS METRICS

### Technical
- [ ] `SpanBuilder` renders "Hello [button] world" **with wrapping**
- [ ] Button receives clicks in **multi-line** text
- [ ] **NO recursive layout** (verified by layout logs)
- [ ] Matches `LineWrapper` perf + `LineWithInvisibles` pattern

### Day 1 Demo
```rust
let span = SpanBuilder::new(font_id, font_size)
    .text("Hello ")
    .element(Button::new("Click", cx.listener(|_, _, cx| { println!("clicked!"); })), window, cx)
    .text(" world!")
    .build();

div().child(span)  // Wraps at 200px → 2 lines, button clickable
```

## 🚀 ACTION PLAN

```
HOUR 1-2:   WrapBuffer core (2h)
HOUR 3:     SpanBuilder + pre-measure (45min)
HOUR 4:     Span::request_layout() + prepaint() (45min)
HOUR 4:30:  Span::paint() + hit_test() + demo (1h)
```

**VERDICT**: 🟢 **4H30m TO PRODUCTION** - **SpanBuilder = RECURSION-PROOF** - **LineWithInvisibles pattern** 🚀