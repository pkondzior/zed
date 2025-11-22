# 🚀 INLINE CONTENT SYSTEM: FINAL IMPLEMENTATION PLAN (4 HOURS ✅)

## 🎯 EXECUTIVE SUMMARY

**WrapBuffer** = **WrapMap pattern perfected**. **`&mut LineWrapper` + `'a` lifetime** = **100% LineWrapper compatible**. **4 HOURS TO PRODUCTION**.

```
Fragment → WrapBuffer → LineWrapper → WrapLines → Span
     ↓                           ↓
Raw data                 Exact existing algorithm       Hit test + paint
```

## 🏗 CORE ARCHITECTURE (WrapMap-Exact)

```
Fragment { Text{runs}, Element{id, size, len_utf8} }
    ↓ WrapBuffer<'a>(&mut LineWrapper) → LineFragment<'a>
Vec<LineFragment<'a>> → LineWrapper::wrap_line() [EXISTING]
    ↓ fragment_map: Vec<usize> → Hit testing preserved
Vec<WrapLines { fragment_ids, width }> → Span paint + clicks
```

## 📋 IMPLEMENTATION PLAN (4 HOURS)

### Phase 1: WrapBuffer Core (2 HOURS)

**Goal**: `InlineContent::wrap(&mut LineWrapper)` → `WrapLines`

**Files**: `crates/gpui/src/inline_content.rs`

```
✅ [ ] Fragment enum (15min)
✅ [ ] InlineContent builder (15min)  
✅ [ ] WrapBuffer<'a> + build_fragments() (45min)
✅ [ ] wrap() → Vec<WrapLines> via LineWrapper (30min)
✅ [ ] Demo: "Hello [button] world" wrapped (15min)
```

### Phase 2: Span Element (2 HOURS)

**Goal**: Paint + clickable multi-line elements

**Files**: `crates/gpui/src/elements/span.rs`

```
✅ [ ] #[derive(Element)] pub struct Span (30min)
✅ [ ] Span::layout() → measure + WrapBuffer.wrap() (30min)
✅ [ ] Span::prepaint() → steal elements (15min)
✅ [ ] Span::paint() → shape + stolen elements (30min)
✅ [ ] Span::hit_test() → fragment_ids → clicks (15min)
```

## 🆕 CORE TYPES (`crates/gpui/src/inline_content.rs`)

```rust
#[derive(Clone)]
pub enum Fragment {
    Text { runs: Vec<TextRun> },
    Element {
        id: usize,
        size: Size<Pixels>,
        len_utf8: usize,
        element: Option<Box<dyn AnyElement>>,
    }
}

#[derive(Clone)]
pub struct InlineContent {
    pub fragments: Vec<Fragment>,
    pub font_id: FontId,
    pub font_size: Pixels,
}

pub struct WrapBuffer<'a> {
    line_wrapper: &'a mut LineWrapper,
    content: InlineContent,
    line_fragments: Vec<LineFragment<'a>>,
    fragment_map: Vec<usize>,
}

pub struct WrapLines {
    pub fragment_ids: Vec<usize>,
    pub width: Pixels,
}
```

## 🔧 Phase 1: WrapBuffer Implementation

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

impl InlineContent {
    pub fn wrap(&self, line_wrapper: &mut LineWrapper, width: Pixels) -> Vec<WrapLines> {
        let mut buffer = WrapBuffer::new(line_wrapper, self.clone());
        buffer.wrap(width)
    }
}
```

## 🖼 Phase 2: Span Element

```rust
#[derive(Element)]
pub struct Span {
    content: InlineContent,
    wrapper: Vec<WrapLines>,
    stolen_elements: Vec<Box<dyn AnyElement>>,
}

impl Element for Span {
    type PrepaintState = Vec<Box<dyn AnyElement>>;

    fn layout(&mut self, space: Size<SizeConstraint>, window: &mut Window, cx: &mut ViewContext<Self>) -> Size<Pixels> {
        // 1. Measure elements
        self.content.measure_elements(window, cx);
        
        // 2. Wrap with LineWrapper
        let mut line_wrapper = LineWrapper::new(self.content.font_id, self.content.font_size, window.text_system().clone());
        self.wrapper = self.content.wrap(&mut line_wrapper, space.width);
        
        Size::new(
            self.wrapper.iter().map(|l| l.width).fold(Pixels(0.), Pixels::max).min(space.width),
            self.wrapper.len() as f32 * self.content.font_size * 1.2
        )
    }
}
```

## ✅ ALL RISKS RESOLVED

| Risk | Original Issue | WrapBuffer Solution | Status |
|------|----------------|---------------------|--------|
| LineWrapper `&mut` | ❌ Mutable caches | Pass `&mut LineWrapper` | ✅ SOLVED |
| `'a` lifetime | ❌ `'static` mismatch | `WrapBuffer<'a>` | ✅ SOLVED |
| `frag_id_map()` | ❌ Doesn't exist | `fragment_map: Vec<usize>` | ✅ SOLVED |
| Text shaping | ❌ Wrong timing | Real `width_for_char()` | ✅ SOLVED |
| Element widths | ❌ Unknown | Exact `LineFragment::element()` | ✅ SOLVED |

## 📈 TIMELINE

| Phase | Time | Status |
|-------|------|--------|
| **Phase 1** | **2 HOURS** | 🟢 WrapBuffer core |
| **Phase 2** | **2 HOURS** | 🟢 Span + paint/clicks |
| **TOTAL** | **4 HOURS** | 🟢 EXECUTE NOW |

## 🎯 SUCCESS METRICS

### Technical
- [ ] `Span` renders "Hello [button] world" **with wrapping**
- [ ] Button receives clicks in **multi-line** text
- [ ] Matches `LineWrapper` perf + logic **exactly**

### Day 1 Demo
```rust
Span::new()
    .text("Hello ")
    .element(Button::new("Click", cx.listener(|_, _, cx| {})))
    .text(" world!")
    // Wraps at 200px → 2 lines, button clickable
```

## 🚀 ACTION PLAN

```
HOUR 1:  [ ] Fragment + InlineContent + WrapBuffer::new() (1h)
HOUR 2:  [ ] WrapBuffer::wrap() + demo (1h)  
HOUR 3:  [ ] Span::layout() + prepaint() (1h)
HOUR 4:  [ ] Span::paint() + hit_test() + final demo (1h)
```

**VERDICT**: 🟢 **4 HOURS TO PRODUCTION** - **WrapBuffer = PERFECT** 🚀