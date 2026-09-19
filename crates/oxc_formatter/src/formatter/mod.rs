//! Infrastructure for code formatting
//!
//! This module defines [FormatElement], an IR to format code documents and provides a mean to print
//! such a document to a string. Objects that know how to format themselves implement the [Format] trait.
//!
//! ## Formatting Traits
//!
//! * [Format]: Implemented by objects that can be formatted.
//!
//! ## Formatting Macros
//!
//! This crate defines two macros to construct the IR. These are inspired by Rust's `fmt` macros
//! * [`format!`]: Formats a formattable object
//! * [`format_args!`]: Concatenates a sequence of Format objects.
//! * [`write!`]: Writes a sequence of formattable objects into an output buffer.

// FIXME
#![allow(rustdoc::broken_intra_doc_links)]

mod builders;
pub mod comments;
mod context;
pub mod formatter_js;
pub mod jsdoc;
pub mod prelude;
pub mod separated;
pub mod token;
pub mod trivia;

pub use self::builders::JoinBuilderJsExt;
pub use self::comments::Comments;
pub use self::{
    context::{ClassContext, JsFormatContext},
    formatter_js::{JsFormatter, JsFormatterExt},
};
use oxc_formatter_core::{
    Arguments, Buffer as _, Document, FormatContext as _, FormatSession, FormatState, Formatted,
    VecBuffer,
};

/// The `format` function takes an [`Arguments`] struct and returns the resulting formatting IR.
///
/// The [`Arguments`] instance can be created with the [`format_args!`].
pub fn format<'ast>(
    context: JsFormatContext<'ast>,
    session: &FormatSession<'ast>,
    arguments: Arguments<'_, 'ast, JsFormatContext<'ast>>,
) -> Formatted<'ast, JsFormatContext<'ast>> {
    // Pre-allocate buffer at 40% of source length (source_len * 2 / 5).
    // Analysis of 4,891 VSCode files shows FormatElement buffer length is typically 19% of source (median),
    // with 95th percentile at 30-38% across all file sizes. This 0.4x multiplier avoids reallocation for 95%+ of files.
    let capacity = (context.source_text().len() * 2) / 5;

    let mut state = FormatState::new_with_session(context, session.clone());
    let mut buffer = VecBuffer::with_capacity(capacity, &mut state);

    buffer.write_fmt(arguments);

    let elements = buffer.into_vec();
    let allocator = state.allocator();
    let mut context = state.into_context();

    let tailwind_classes = context.take_tailwind_classes();
    let unsorted_indices = context.take_unsorted_class_indices();
    let sorted_tailwind_classes = sort_classes(session, tailwind_classes, &unsorted_indices);

    // Expand the wrapping class markers into fills now that the sorted content is known.
    let wrapped_indices = context.take_wrapped_class_indices();
    let elements = if wrapped_indices.is_empty() {
        elements
    } else {
        let mut wraps = vec![false; sorted_tailwind_classes.len()];
        for index in wrapped_indices {
            // A host sorter that returned fewer classes than it was given leaves
            // the tail out of range; the printer reports that on its own.
            if let Some(wrap) = wraps.get_mut(index) {
                *wrap = true;
            }
        }
        crate::ir_transform::expand_wrap_markers(
            elements,
            &sorted_tailwind_classes,
            &wraps,
            context.options(),
            allocator,
        )
    };

    let ir = Document::new(elements, sorted_tailwind_classes);

    Formatted::new(ir, context)
}

/// Sorts the collected class strings, except those at `unsorted_indices`,
/// which keep their content and position.
///
/// The sorter reorders classes within each string, never the vector itself,
/// so the sorted strings are put back into the slots they were taken from.
/// A host that breaks that contract and returns fewer strings leaves the
/// remaining slots with their collected content instead of emptying them.
fn sort_classes(
    session: &FormatSession<'_>,
    mut classes: Vec<String>,
    unsorted_indices: &[usize],
) -> Vec<String> {
    if unsorted_indices.is_empty() {
        return session.sort_tailwind_classes(classes);
    }
    let mut is_unsorted = vec![false; classes.len()];
    for &index in unsorted_indices {
        is_unsorted[index] = true;
    }
    let sortable: Vec<String> = classes
        .iter()
        .zip(&is_unsorted)
        .filter(|(_, unsorted)| !**unsorted)
        .map(|(class, _)| class.clone())
        .collect();
    let sortable_len = sortable.len();
    let sorted = session.sort_tailwind_classes(sortable);
    debug_assert_eq!(
        sorted.len(),
        sortable_len,
        "the Tailwind sorter must return one string per collected class string"
    );
    let mut sorted = sorted.into_iter();
    for (class, unsorted) in classes.iter_mut().zip(&is_unsorted) {
        if !unsorted && let Some(sorted_class) = sorted.next() {
            *class = sorted_class;
        }
    }
    classes
}
