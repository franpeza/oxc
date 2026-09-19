//! Post-sort expansion of wrapping class-string markers (`wrap_class_names`).
//!
//! The class string writers mark some `FormatElement::TailwindClass` indices
//! as wrapping. Their final content is only known after the host's batched
//! class sort, so `formatter::format` calls [`expand_wrap_markers`] right after
//! sorting to replace each wrapping marker with a `fill` of the post-sort
//! classes, separated by `soft_line_break_or_space`. The printer's fill
//! algorithm then packs as many classes per line as fit.
//!
//! The pass is recursive: markers may sit inside [`Interned`] or
//! [`BestFittingElement`] payloads (e.g. JSX children interning and
//! best-fitting layouts). Unchanged nested slices are reused as-is, and each
//! shared slice is scanned and rebuilt once: the same `Interned` content is
//! referenced from several best-fitting variants at every nesting level, so
//! rebuilding it per reference would grow exponentially with nesting depth.
//!
//! Splitting after the sort also makes re-formatting idempotent: a
//! previously wrapped string contains newlines and indentation, and
//! `split_ascii_whitespace` collapses them back into single classes.

mod expander;
mod fill;

use std::cell::RefCell;

use oxc_allocator::{Allocator, Vec as ArenaVec};
use oxc_formatter_core::FormatElement;

use crate::options::JsFormatOptions;

use self::expander::Expander;

/// Replaces every `TailwindClass(index)` marker whose `wraps[index]` is `true`
/// with a `fill` of the post-sort classes.
///
/// Other markers pass through unchanged: the printer resolves them against
/// `sorted_classes`, which stays index-compatible because this pass never
/// adds or removes classes.
pub fn expand_wrap_markers<'a>(
    elements: ArenaVec<'a, FormatElement<'a>>,
    sorted_classes: &[String],
    wraps: &[bool],
    options: &JsFormatOptions,
    allocator: &'a Allocator,
) -> ArenaVec<'a, FormatElement<'a>> {
    let expander = Expander {
        sorted_classes,
        wraps,
        indent_width: options.indent_width,
        allocator,
        contains: RefCell::default(),
        expanded: RefCell::default(),
    };
    let mut out = ArenaVec::with_capacity_in(elements.len() + 16, &allocator);
    expander.expand_into(elements.into_iter(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use oxc_formatter_core::{BestFittingElement, Interned};

    use super::*;

    fn run(elements: Vec<FormatElement<'_>>, classes: &[&str], allocator: &Allocator) -> String {
        let mut arena = ArenaVec::new_in(&allocator);
        arena.extend(elements);
        let classes: Vec<String> = classes.iter().map(ToString::to_string).collect();
        let wraps = vec![true; classes.len()];
        let result =
            expand_wrap_markers(arena, &classes, &wraps, &JsFormatOptions::default(), allocator);
        format!("{result:?}")
    }

    #[test]
    fn passthrough_non_wrap_marker() {
        let owned_allocator = Allocator::default();
        let allocator = &owned_allocator;
        let mut arena = ArenaVec::new_in(&allocator);
        arena.push(FormatElement::TailwindClass(0));
        let classes = vec!["b a".to_string()];
        let out =
            expand_wrap_markers(arena, &classes, &[false], &JsFormatOptions::default(), allocator);
        assert_eq!(format!("{out:?}"), "Vec([TailwindClass(0)])");
    }

    #[test]
    fn single_class_becomes_plain_text() {
        let owned_allocator = Allocator::default();
        let allocator = &owned_allocator;
        let out = run(vec![FormatElement::TailwindClass(0)], &["only-class"], allocator);
        assert_eq!(out, "Vec([Text(\"only-class\")])");
    }

    #[test]
    fn multiple_classes_become_fill() {
        let owned_allocator = Allocator::default();
        let allocator = &owned_allocator;
        let out = run(vec![FormatElement::TailwindClass(0)], &["a b c"], allocator);
        assert_eq!(
            out,
            "Vec([Tag(StartFill), \
             Tag(StartEntry), Text(\"a\"), Tag(EndEntry), \
             Tag(StartEntry), Line(SoftOrSpace), Tag(EndEntry), \
             Tag(StartEntry), Text(\"b\"), Tag(EndEntry), \
             Tag(StartEntry), Line(SoftOrSpace), Tag(EndEntry), \
             Tag(StartEntry), Text(\"c\"), Tag(EndEntry), \
             Tag(EndFill)])"
        );
    }

    #[test]
    fn closing_delimiter_joins_last_entry() {
        let owned_allocator = Allocator::default();
        let allocator = &owned_allocator;
        let out = run(
            vec![FormatElement::TailwindClass(0), FormatElement::Token { text: "\"" }],
            &["a b"],
            allocator,
        );
        assert!(
            out.ends_with("Text(\"b\"), Token(\"\\\"\"), Tag(EndEntry), Tag(EndFill)])"),
            "{out}"
        );
    }

    #[test]
    fn collapses_internal_newlines() {
        // Idempotency: previously wrapped output re-parses with embedded
        // newlines and indentation; the split collapses them.
        let owned_allocator = Allocator::default();
        let allocator = &owned_allocator;
        let out = run(vec![FormatElement::TailwindClass(0)], &["a\n  b"], allocator);
        assert!(out.contains("Text(\"a\")") && out.contains("Text(\"b\")"), "{out}");
    }

    #[test]
    fn shared_interned_content_is_expanded_once() {
        let owned_allocator = Allocator::default();
        let allocator = &owned_allocator;

        let mut inner = ArenaVec::new_in(&allocator);
        inner.push(FormatElement::TailwindClass(0));
        let interned = Interned::from_slice(inner.into_arena_slice());

        let mut elements = ArenaVec::new_in(&allocator);
        elements.push(FormatElement::Interned(interned.clone()));
        elements.push(FormatElement::Interned(interned));
        let classes = vec!["a b".to_string()];
        let out = expand_wrap_markers(
            elements,
            &classes,
            &[true],
            &JsFormatOptions::default(),
            allocator,
        );

        let [FormatElement::Interned(first), FormatElement::Interned(second)] = out.as_slice()
        else {
            panic!("expected two interned elements: {out:?}");
        };
        assert!(std::ptr::eq(first.as_slice(), second.as_slice()), "{out:?}");
    }

    #[test]
    fn expands_inside_interned_and_best_fitting() {
        let owned_allocator = Allocator::default();
        let allocator = &owned_allocator;

        let mut inner = ArenaVec::new_in(&allocator);
        inner.push(FormatElement::TailwindClass(0));
        let interned = FormatElement::Interned(Interned::from_slice(inner.into_arena_slice()));

        let mut flat = ArenaVec::new_in(&allocator);
        flat.push(FormatElement::TailwindClass(0));
        let mut expanded = ArenaVec::new_in(&allocator);
        expanded.push(FormatElement::TailwindClass(0));
        let mut variants = ArenaVec::new_in(&allocator);
        variants.push(flat.into_arena_slice() as &[_]);
        variants.push(expanded.into_arena_slice() as &[_]);
        // SAFETY: two variants.
        let best_fitting =
            FormatElement::BestFitting(unsafe { BestFittingElement::from_vec_unchecked(variants) });

        let out = run(vec![interned, best_fitting], &["a b"], allocator);
        assert!(!out.contains("TailwindClass"), "all markers must expand: {out}");
        assert_eq!(out.matches("Tag(StartFill)").count(), 3, "{out}");
    }
}
