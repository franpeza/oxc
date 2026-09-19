//! Building the `fill` that packs the post-sort classes across lines.

use oxc_allocator::Vec as ArenaVec;
use oxc_formatter_core::{FormatElement, LineMode, Tag, TextWidth};

use super::expander::Expander;

impl<'a> Expander<'a, '_> {
    /// Continuation lines start at the current indentation; the writer
    /// wraps the marker in an indent where it wants one more level.
    /// The tag/entry shape mirrors `FillBuilder`.
    pub(super) fn expand_marker_into(
        &self,
        index: usize,
        mut closing: Vec<FormatElement<'a>>,
        out: &mut ArenaVec<'a, FormatElement<'a>>,
    ) {
        let text = |s: &str| FormatElement::Text {
            text: self.allocator.alloc_str(s),
            width: TextWidth::from_text(s, self.indent_width),
        };

        let Some(class_list) = self.sorted_classes.get(index) else {
            // Dangling index: keep the marker so the printer's own
            // debug_assert reports it.
            out.push(FormatElement::TailwindClass(index));
            out.extend(std::mem::take(&mut closing));
            return;
        };
        let mut classes = class_list.split_ascii_whitespace().peekable();
        let Some(first) = classes.next() else {
            // Whitespace-only content: nothing to wrap.
            out.extend(std::mem::take(&mut closing));
            return;
        };
        if classes.peek().is_none() {
            out.push(text(first));
            out.extend(std::mem::take(&mut closing));
            return;
        }

        out.push(FormatElement::Tag(Tag::StartFill));
        out.push(FormatElement::Tag(Tag::StartEntry));
        out.push(text(first));
        out.push(FormatElement::Tag(Tag::EndEntry));
        while let Some(class) = classes.next() {
            out.push(FormatElement::Tag(Tag::StartEntry));
            out.push(FormatElement::Line(LineMode::SoftOrSpace));
            out.push(FormatElement::Tag(Tag::EndEntry));
            out.push(FormatElement::Tag(Tag::StartEntry));
            out.push(text(class));
            if classes.peek().is_none() {
                out.extend(std::mem::take(&mut closing));
            }
            out.push(FormatElement::Tag(Tag::EndEntry));
        }
        out.push(FormatElement::Tag(Tag::EndFill));
    }
}
