//! The recursive walk that rebuilds the document around wrapping markers.
//!
//! Each shared slice is scanned and rebuilt once (see the module docs of the
//! parent module for why that matters).

use std::cell::RefCell;

use oxc_allocator::{Allocator, Vec as ArenaVec};
use oxc_formatter_core::{BestFittingElement, FormatElement, IndentWidth, Interned, Tag};
use rustc_hash::FxHashMap;

/// Identifies a shared slice by its address and length.
pub(super) type SliceKey = (usize, usize);

fn slice_key(slice: &[FormatElement<'_>]) -> SliceKey {
    (slice.as_ptr() as usize, slice.len())
}

pub(super) struct Expander<'a, 'b> {
    pub(super) sorted_classes: &'b [String],
    pub(super) wraps: &'b [bool],
    pub(super) indent_width: IndentWidth,
    pub(super) allocator: &'a Allocator,
    /// Whether a nested slice contains a wrapping marker.
    pub(super) contains: RefCell<FxHashMap<SliceKey, bool>>,
    /// The rebuilt version of each nested slice that contains one.
    pub(super) expanded: RefCell<FxHashMap<SliceKey, &'a [FormatElement<'a>]>>,
}

impl<'a> Expander<'a, '_> {
    fn is_wrap_marker(&self, element: &FormatElement<'_>) -> bool {
        matches!(element, FormatElement::TailwindClass(index) if self.wraps.get(*index) == Some(&true))
    }

    fn contains_wrap_marker(&self, slice: &[FormatElement<'_>]) -> bool {
        let key = slice_key(slice);
        if let Some(&contains) = self.contains.borrow().get(&key) {
            return contains;
        }
        let contains = slice.iter().any(|element| match element {
            FormatElement::Interned(interned) => self.contains_wrap_marker(interned),
            FormatElement::BestFitting(best_fitting) => {
                best_fitting.variants().iter().any(|variant| self.contains_wrap_marker(variant))
            }
            _ => self.is_wrap_marker(element),
        });
        self.contains.borrow_mut().insert(key, contains);
        contains
    }

    pub(super) fn expand_into(
        &self,
        elements: impl Iterator<Item = FormatElement<'a>>,
        out: &mut ArenaVec<'a, FormatElement<'a>>,
    ) {
        let mut elements = elements.peekable();
        while let Some(element) = elements.next() {
            match element {
                FormatElement::TailwindClass(index) if self.is_wrap_marker(&element) => {
                    // What closes the string belongs to the last line, so the
                    // fill must count its width: a preserved boundary space and
                    // the delimiter. The writer may wrap the marker alone in an
                    // indent, so look past one `EndIndent`.
                    let end_indent =
                        elements.next_if(|next| matches!(next, FormatElement::Tag(Tag::EndIndent)));
                    let space = elements
                        .next_if(|next| matches!(next, FormatElement::Text { text: " ", .. }));
                    let closing = elements.next_if(|next| {
                        matches!(next, FormatElement::Token { text: "\"" | "'" | "`" | "`}" })
                    });
                    // A space with no delimiter after it stays outside the fill,
                    // where it cannot end a line.
                    let trailing = match (space, closing) {
                        (space, Some(closing)) => {
                            let mut trailing = Vec::with_capacity(2);
                            trailing.extend(space);
                            trailing.push(closing);
                            trailing
                        }
                        (space, None) => {
                            self.expand_marker_into(index, Vec::new(), out);
                            out.extend(end_indent);
                            out.extend(space);
                            continue;
                        }
                    };
                    self.expand_marker_into(index, trailing, out);
                    out.extend(end_indent);
                }
                FormatElement::Interned(interned) => {
                    let expanded = self.expand_slice(interned.as_slice());
                    out.push(FormatElement::Interned(Interned::from_slice(expanded)));
                }
                FormatElement::BestFitting(best_fitting) => {
                    let variants = best_fitting.variants();
                    if variants.iter().any(|variant| self.contains_wrap_marker(variant)) {
                        let mut expanded =
                            ArenaVec::with_capacity_in(variants.len(), &self.allocator);
                        for variant in variants {
                            expanded.push(self.expand_slice(variant));
                        }
                        // SAFETY: `expanded` has as many variants as the original.
                        let element = unsafe { BestFittingElement::from_vec_unchecked(expanded) };
                        out.push(FormatElement::BestFitting(element));
                    } else {
                        out.push(FormatElement::BestFitting(best_fitting));
                    }
                }
                other => out.push(other),
            }
        }
    }

    /// Returns the slice unchanged when it contains no wrapping marker,
    /// otherwise rebuilds it with the markers expanded.
    fn expand_slice(&self, slice: &'a [FormatElement<'a>]) -> &'a [FormatElement<'a>] {
        if !self.contains_wrap_marker(slice) {
            return slice;
        }
        let key = slice_key(slice);
        if let Some(&expanded) = self.expanded.borrow().get(&key) {
            return expanded;
        }
        let mut out = ArenaVec::with_capacity_in(slice.len() + 16, &self.allocator);
        self.expand_into(slice.iter().cloned(), &mut out);
        let expanded = out.into_arena_slice();
        self.expanded.borrow_mut().insert(key, expanded);
        expanded
    }
}
