//! Post-sort expansion of wrapping Tailwind class markers.
//!
//! A consumer emits `FormatElement::TailwindClass { wrap: true }` for class
//! strings that should wrap to the print width (e.g. `oxc_formatter`'s
//! `wrap_class_names` option). Their final content is only known after the
//! host's batched class sort, so the consumer calls [`expand_wrap_markers`]
//! right after sorting — and before [`super::document::Document`]'s
//! `propagate_expand` — to replace each wrapping marker with
//! `indent(fill(classes, soft_line_break_or_space))` built from the
//! post-sort string. The printer's fill algorithm then packs as many
//! classes per line as fit.
//!
//! The pass is recursive: markers may sit inside [`Interned`] or
//! [`BestFittingElement`] payloads (e.g. JSX children interning and
//! best-fitting layouts). Unchanged nested slices are reused as-is.
//!
//! Splitting after the sort also makes re-formatting idempotent: a
//! previously wrapped string contains newlines and indentation, and
//! `split_ascii_whitespace` collapses them back into single classes.

use oxc_allocator::{Allocator, Vec as ArenaVec};

use super::{BestFittingElement, FormatElement, Interned, LineMode, TextWidth, tag::Tag};
use crate::IndentWidth;

/// Replaces every `TailwindClass { wrap: true }` marker with an
/// `indent(fill(...))` sequence of the post-sort classes.
///
/// `wrap: false` markers pass through unchanged (they are resolved by the
/// printer against `sorted_classes`, which stays index-compatible because
/// this pass never adds or removes classes).
pub fn expand_wrap_markers<'a>(
    elements: ArenaVec<'a, FormatElement<'a>>,
    sorted_classes: &[String],
    indent_width: IndentWidth,
    allocator: &'a Allocator,
) -> ArenaVec<'a, FormatElement<'a>> {
    let mut result = ArenaVec::with_capacity_in(elements.len() + 16, &allocator);
    for element in elements {
        expand_element_into(element, &mut result, sorted_classes, indent_width, allocator);
    }
    result
}

fn slice_contains_wrap_marker(slice: &[FormatElement<'_>]) -> bool {
    slice.iter().any(|element| match element {
        FormatElement::TailwindClass { wrap, .. } => *wrap,
        FormatElement::Interned(interned) => slice_contains_wrap_marker(interned),
        FormatElement::BestFitting(best_fitting) => {
            best_fitting.variants().iter().any(|variant| slice_contains_wrap_marker(variant))
        }
        _ => false,
    })
}

/// Returns the slice unchanged when it contains no wrapping marker,
/// otherwise rebuilds it with the markers expanded.
fn expand_slice<'a>(
    slice: &'a [FormatElement<'a>],
    sorted_classes: &[String],
    indent_width: IndentWidth,
    allocator: &'a Allocator,
) -> &'a [FormatElement<'a>] {
    if !slice_contains_wrap_marker(slice) {
        return slice;
    }
    let mut out = ArenaVec::with_capacity_in(slice.len() + 16, &allocator);
    for element in slice {
        expand_element_into(element.clone(), &mut out, sorted_classes, indent_width, allocator);
    }
    out.into_arena_slice()
}

fn expand_element_into<'a>(
    element: FormatElement<'a>,
    out: &mut ArenaVec<'a, FormatElement<'a>>,
    sorted_classes: &[String],
    indent_width: IndentWidth,
    allocator: &'a Allocator,
) {
    match element {
        FormatElement::TailwindClass { index, wrap: true } => {
            expand_marker_into(index, out, sorted_classes, indent_width, allocator);
        }
        FormatElement::Interned(interned) => {
            let expanded =
                expand_slice(interned.as_slice(), sorted_classes, indent_width, allocator);
            out.push(FormatElement::Interned(Interned::from_slice(expanded)));
        }
        FormatElement::BestFitting(best_fitting) => {
            let variants = best_fitting.variants();
            if variants.iter().any(|variant| slice_contains_wrap_marker(variant)) {
                let mut expanded = ArenaVec::with_capacity_in(variants.len(), &allocator);
                for variant in variants {
                    expanded.push(expand_slice(variant, sorted_classes, indent_width, allocator));
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

fn expand_marker_into<'a>(
    index: usize,
    out: &mut ArenaVec<'a, FormatElement<'a>>,
    sorted_classes: &[String],
    indent_width: IndentWidth,
    allocator: &'a Allocator,
) {
    let Some(class_list) = sorted_classes.get(index) else {
        // Dangling index: keep the marker so the printer's own
        // debug_assert reports it.
        out.push(FormatElement::TailwindClass { index, wrap: false });
        return;
    };

    let mut classes = class_list.split_ascii_whitespace();
    let Some(first) = classes.next() else {
        // Whitespace-only content should have been normalized by the
        // writer; nothing to emit.
        return;
    };

    let text = |s: &str| FormatElement::Text {
        text: allocator.alloc_str(s),
        width: TextWidth::from_text(s, indent_width),
    };

    let Some(second) = classes.next() else {
        // Single class: nothing to wrap.
        out.push(text(first));
        return;
    };

    // Continuation lines sit one level deeper than the line the class
    // string starts on. The tag/entry shape mirrors `FillBuilder`.
    out.push(FormatElement::Tag(Tag::StartIndent));
    out.push(FormatElement::Tag(Tag::StartFill));
    out.push(FormatElement::Tag(Tag::StartEntry));
    out.push(text(first));
    out.push(FormatElement::Tag(Tag::EndEntry));
    for class in std::iter::once(second).chain(classes) {
        out.push(FormatElement::Tag(Tag::StartEntry));
        out.push(FormatElement::Line(LineMode::SoftOrSpace));
        out.push(FormatElement::Tag(Tag::EndEntry));
        out.push(FormatElement::Tag(Tag::StartEntry));
        out.push(text(class));
        out.push(FormatElement::Tag(Tag::EndEntry));
    }
    out.push(FormatElement::Tag(Tag::EndFill));
    out.push(FormatElement::Tag(Tag::EndIndent));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(elements: Vec<FormatElement<'_>>, classes: &[&str], allocator: &Allocator) -> String {
        let mut arena = ArenaVec::new_in(&allocator);
        arena.extend(elements);
        let classes: Vec<String> = classes.iter().map(ToString::to_string).collect();
        let result = expand_wrap_markers(arena, &classes, IndentWidth::default(), allocator);
        format!("{result:?}")
    }

    #[test]
    fn passthrough_non_wrap_marker() {
        let allocator = Allocator::default();
        let out =
            run(vec![FormatElement::TailwindClass { index: 0, wrap: false }], &["b a"], &allocator);
        assert_eq!(out, "Vec([TailwindClass { index: 0, wrap: false }])");
    }

    #[test]
    fn single_class_becomes_plain_text() {
        let allocator = Allocator::default();
        let out = run(
            vec![FormatElement::TailwindClass { index: 0, wrap: true }],
            &["only-class"],
            &allocator,
        );
        assert_eq!(out, "Vec([Text(\"only-class\")])");
    }

    #[test]
    fn multiple_classes_become_fill() {
        let allocator = Allocator::default();
        let out = run(
            vec![FormatElement::TailwindClass { index: 0, wrap: true }],
            &["a b c"],
            &allocator,
        );
        assert_eq!(
            out,
            "Vec([Tag(StartIndent), Tag(StartFill), \
             Tag(StartEntry), Text(\"a\"), Tag(EndEntry), \
             Tag(StartEntry), Line(SoftOrSpace), Tag(EndEntry), \
             Tag(StartEntry), Text(\"b\"), Tag(EndEntry), \
             Tag(StartEntry), Line(SoftOrSpace), Tag(EndEntry), \
             Tag(StartEntry), Text(\"c\"), Tag(EndEntry), \
             Tag(EndFill), Tag(EndIndent)])"
        );
    }

    #[test]
    fn collapses_internal_newlines() {
        // Idempotency: previously wrapped output re-parses with embedded
        // newlines and indentation; the split collapses them.
        let allocator = Allocator::default();
        let out = run(
            vec![FormatElement::TailwindClass { index: 0, wrap: true }],
            &["a\n  b"],
            &allocator,
        );
        assert!(out.contains("Text(\"a\")") && out.contains("Text(\"b\")"), "{out}");
    }

    #[test]
    fn expands_inside_interned_and_best_fitting() {
        let owned_allocator = Allocator::default();
        let allocator = &owned_allocator;

        let mut inner = ArenaVec::new_in(&allocator);
        inner.push(FormatElement::TailwindClass { index: 0, wrap: true });
        let interned = FormatElement::Interned(Interned::new(inner));

        let mut flat = ArenaVec::new_in(&allocator);
        flat.push(FormatElement::TailwindClass { index: 0, wrap: true });
        let mut expanded = ArenaVec::new_in(&allocator);
        expanded.push(FormatElement::TailwindClass { index: 0, wrap: true });
        let mut variants = ArenaVec::new_in(&allocator);
        variants.push(flat.into_arena_slice() as &[_]);
        variants.push(expanded.into_arena_slice() as &[_]);
        // SAFETY: two variants.
        let best_fitting =
            FormatElement::BestFitting(unsafe { BestFittingElement::from_vec_unchecked(variants) });

        let out = run(vec![interned, best_fitting], &["a b"], &allocator);
        assert!(!out.contains("TailwindClass"), "all markers must expand: {out}");
        assert_eq!(out.matches("Tag(StartFill)").count(), 3, "{out}");
    }
}
