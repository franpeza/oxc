use std::mem;

use oxc_ast::Comment;
use oxc_formatter_core::{FormatElement, SourceText};
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::FxHashMap;

use crate::{
    options::{JsFormatOptions, SortTailwindcssOptions},
    utils::assignment_like::AssignmentLikeLayout,
};

use super::Comments;

/// Entry in the class context stack, tracking whether we're inside a class
/// string context targeted by `sort_tailwindcss` and/or `wrap_class_names`.
#[derive(Clone, Copy, Debug)]
pub struct ClassContext {
    /// Whether to preserve whitespace (newlines) in template literals.
    pub preserve_whitespace: bool,
    /// Whether we're inside a template literal expression (between `${` and `}`).
    /// If true, we need to consider whitespace in adjacent quasis.
    pub in_template_expression: bool,
    /// Whether the quasi before this expression ends with whitespace.
    /// Only relevant when `in_template_expression` is true.
    pub quasi_before_has_trailing_ws: bool,
    /// Whether the quasi after this expression starts with whitespace.
    /// Only relevant when `in_template_expression` is true.
    pub quasi_after_has_leading_ws: bool,
    /// Whether this is the first quasi in a template literal.
    /// Used for template element boundary detection.
    pub is_first_quasi: bool,
    /// Whether this is the last quasi in a template literal.
    /// Used for template element boundary detection.
    pub is_last_quasi: bool,
    /// Whether class handling is disabled in this context.
    /// Used to prevent touching strings inside nested non-class call expressions.
    /// For example, in `classNames("a", x.includes("\n") ? "b" : "c")`, the `"\n"`
    /// inside `includes()` is NOT a class string.
    pub disabled: bool,
    /// Whether `wrap_class_names` targets this context.
    pub wrap: bool,
    /// Whether `sort_tailwindcss` targets this context.
    pub sort: bool,
}

impl ClassContext {
    /// Create a new context entry for JSX attributes or function calls,
    /// given the `sort_tailwindcss` options when sorting targets it and whether
    /// wrapping targets it. Returns `None` when neither does.
    pub fn new(sort: Option<&SortTailwindcssOptions>, wrap: bool) -> Option<Self> {
        if sort.is_none() && !wrap {
            return None;
        }
        Some(Self {
            preserve_whitespace: sort.is_some_and(|options| options.preserve_whitespace),
            in_template_expression: false,
            quasi_before_has_trailing_ws: true, // Default: can collapse
            quasi_after_has_leading_ws: true,   // Default: can collapse
            is_first_quasi: true,
            is_last_quasi: true,
            disabled: false,
            wrap,
            sort: sort.is_some(),
        })
    }

    /// Returns this context with wrapping turned off.
    #[must_use]
    pub fn without_wrap(mut self) -> Self {
        self.wrap = false;
        self
    }

    /// Whether sorting or wrapping applies in this context.
    pub fn is_active(&self) -> bool {
        self.sort || self.wrap
    }

    /// Create a new context entry for template literal expressions.
    /// Inherits `preserve_whitespace`, `wrap` and `sort` from the parent context.
    pub fn template_expression(
        parent: ClassContext,
        quasi_before_has_trailing_ws: bool,
        quasi_after_has_leading_ws: bool,
    ) -> Self {
        Self {
            preserve_whitespace: parent.preserve_whitespace,
            in_template_expression: true,
            quasi_before_has_trailing_ws,
            quasi_after_has_leading_ws,
            is_first_quasi: true,
            is_last_quasi: true,
            disabled: false,
            wrap: parent.wrap,
            sort: parent.sort,
        }
    }

    /// Create a new context entry with updated quasi position.
    /// Used when formatting individual quasis to track their position in the template.
    #[must_use]
    pub fn with_quasi_position(mut self, is_first: bool, is_last: bool) -> Self {
        self.is_first_quasi = is_first;
        self.is_last_quasi = is_last;
        self
    }
}

/// Context object storing data relevant when formatting an object.
pub struct JsFormatContext<'ast> {
    options: JsFormatOptions,

    source_text: SourceText<'ast>,

    source_type: SourceType,

    comments: Comments<'ast>,

    cached_elements: FxHashMap<Span, FormatElement<'ast>>,

    /// One-shot handoff of the assignment layout to the arrow expression on the RHS of an assignment-like,
    /// keyed by the arrow's span so no other node can consume it.
    /// Set (and cleared) by `WithAssignmentLayout` around formatting the arrow, taken by the arrow's `write`.
    arrow_assignment_layout: Option<(Span, AssignmentLikeLayout)>,

    /// Tracks whether quotes are needed for properties in the current object-like node.
    ///
    /// When [`JsFormatOptions::quote_properties`] is [`crate::QuoteProperties::Consistent`], each entry indicates
    /// whether at least one property key requires quotes. A stack is used to handle nested object-like
    /// structures (e.g., `{ a: { "b-c": 1 } }` where only the inner object needs quoted keys).
    quote_needed_stack: Vec<bool>,

    /// Collected Tailwind CSS class strings from JSX attributes.
    /// These will be sorted by an external callback and replaced during printing.
    tailwind_classes: Vec<String>,

    /// Stack tracking whether we're inside a class context.
    /// When non-empty, string literals hold classes to sort and/or wrap.
    class_context_stack: Vec<ClassContext>,

    /// Indices into `tailwind_classes` that are excluded from sorting,
    /// because only `wrap_class_names` targets their context.
    unsorted_class_indices: Vec<usize>,

    /// Indices into `tailwind_classes` whose marker wraps to the print width
    /// (`wrap_class_names`), expanded after sorting by `format()`.
    wrapped_class_indices: Vec<usize>,

    /// Whether `sort_tailwindcss` or `wrap_class_names` is configured, computed
    /// once so the nodes that could open a class context test a single `bool`.
    has_class_features: bool,
}

impl std::fmt::Debug for JsFormatContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsFormatContext")
            .field("options", &self.options)
            .field("source_text", &self.source_text)
            .field("source_type", &self.source_type)
            .field("comments", &self.comments)
            .field("cached_elements", &self.cached_elements)
            .field("quote_needed_stack", &self.quote_needed_stack)
            .field("tailwind_classes", &self.tailwind_classes)
            .finish()
    }
}

/// Lets embedded children's classes merge into this context's index space
/// (`DispatchPayload::into_doc` at each embed site).
impl oxc_formatter_core::TailwindCollector for JsFormatContext<'_> {
    fn add_class(&mut self, class: String) -> usize {
        self.add_tailwind_class(class)
    }
}

impl oxc_formatter_core::FormatContext for JsFormatContext<'_> {
    type Options = JsFormatOptions;

    fn options(&self) -> &JsFormatOptions {
        &self.options
    }

    fn source_code(&self) -> &str {
        &self.source_text
    }

    fn get_tailwind_class(&self, idx: usize) -> Option<&str> {
        self.tailwind_classes.get(idx).map(String::as_str)
    }
}

impl<'ast> JsFormatContext<'ast> {
    pub fn new(
        source_text: &'ast str,
        source_type: SourceType,
        comments: &'ast [Comment],
        options: JsFormatOptions,
    ) -> Self {
        let source_text = SourceText::new(source_text);
        let has_class_features =
            options.sort_tailwindcss.is_some() || options.wrap_class_names.is_some();
        Self {
            options,
            source_text,
            source_type,
            comments: Comments::new(source_text, comments),
            cached_elements: FxHashMap::default(),
            arrow_assignment_layout: None,
            quote_needed_stack: Vec::new(),
            tailwind_classes: Vec::new(),
            class_context_stack: Vec::new(),
            unsorted_class_indices: Vec::new(),
            wrapped_class_indices: Vec::new(),
            has_class_features,
        }
    }

    /// Returns a reference to the program's comments.
    pub fn comments(&self) -> &Comments<'ast> {
        &self.comments
    }

    /// Returns a reference to the program's comments.
    pub fn comments_mut(&mut self) -> &mut Comments<'ast> {
        &mut self.comments
    }

    /// Returns the source text wrapper
    pub fn source_text(&self) -> SourceText<'ast> {
        self.source_text
    }

    /// Returns the source type
    pub fn source_type(&self) -> SourceType {
        self.source_type
    }

    /// Returns the cached formatted element for the given key.
    pub(crate) fn get_cached_element<T: GetSpan>(&self, key: &T) -> Option<FormatElement<'ast>> {
        self.cached_elements.get(&key.span()).cloned()
    }

    /// Caches the formatted element for the given key.
    pub(crate) fn cache_element<T: GetSpan>(&mut self, key: &T, formatted: FormatElement<'ast>) {
        self.cached_elements.insert(key.span(), formatted);
    }

    /// See the [`Self::arrow_assignment_layout`] field.
    pub(crate) fn set_arrow_assignment_layout(&mut self, span: Span, layout: AssignmentLikeLayout) {
        debug_assert!(
            self.arrow_assignment_layout.is_none(),
            "a previous arrow assignment layout was neither taken nor cleared"
        );
        self.arrow_assignment_layout = Some((span, layout));
    }

    /// See the [`Self::arrow_assignment_layout`] field.
    pub(crate) fn take_arrow_assignment_layout(
        &mut self,
        span: Span,
    ) -> Option<AssignmentLikeLayout> {
        match self.arrow_assignment_layout {
            Some((key, layout)) if key == span => {
                self.arrow_assignment_layout = None;
                Some(layout)
            }
            _ => None,
        }
    }

    /// See the [`Self::arrow_assignment_layout`] field.
    /// Clears a layout left behind when the arrow was printed without running
    /// `write` (a suppressed arrow prints its source verbatim instead).
    pub(crate) fn clear_arrow_assignment_layout(&mut self) {
        self.arrow_assignment_layout = None;
    }

    /// Pushes a new quote needed state onto the stack.
    pub fn push_quote_needed(&mut self, needed: bool) {
        debug_assert!(
            self.options.quote_properties.is_consistent(),
            "`push_quote_needed` should only be used when `self.options.quote_properties.is_consistent()` is true"
        );
        self.quote_needed_stack.push(needed);
    }

    /// Pops the top quote needed state from the stack.
    pub fn pop_quote_needed(&mut self) {
        debug_assert!(
            self.options.quote_properties.is_consistent(),
            "`pop_quote_needed` should only be used when `self.options.quote_properties.is_consistent()` is true"
        );
        self.quote_needed_stack.pop();
    }

    pub fn is_quote_needed(&self) -> bool {
        *self.quote_needed_stack.last().unwrap_or(&false)
    }

    /// Add a Tailwind CSS class string found in JSX attributes.
    /// Returns the index where the class was stored.
    pub fn add_tailwind_class(&mut self, class: String) -> usize {
        let index = self.tailwind_classes.len();
        self.tailwind_classes.push(class);
        index
    }

    /// Add a class string that must NOT be sorted (a `wrap_class_names`-only context).
    /// Returns the index where the class was stored.
    pub fn add_unsorted_class(&mut self, class: String) -> usize {
        let index = self.add_tailwind_class(class);
        self.unsorted_class_indices.push(index);
        index
    }

    /// Take all collected Tailwind classes, clearing the internal storage.
    pub fn take_tailwind_classes(&mut self) -> Vec<String> {
        mem::take(&mut self.tailwind_classes)
    }

    /// Take the indices of classes added with [`Self::add_unsorted_class`].
    pub fn take_unsorted_class_indices(&mut self) -> Vec<usize> {
        mem::take(&mut self.unsorted_class_indices)
    }

    /// Set the collected Tailwind CSS classes.
    pub fn set_tailwind_classes(&mut self, classes: Vec<String>) {
        self.tailwind_classes = classes;
    }

    /// Push a class context entry onto the stack.
    /// Call this when entering a JSXAttribute or CallExpression with a class context.
    pub fn push_class_context(&mut self, entry: ClassContext) {
        self.class_context_stack.push(entry);
    }

    /// Pop a class context entry from the stack.
    /// Call this when leaving a JSXAttribute or CallExpression with a class context.
    pub fn pop_class_context(&mut self) {
        self.class_context_stack.pop();
    }

    /// Get the current class context, if any.
    /// Returns `Some` if we're inside a class context (JSXAttribute or CallExpression).
    pub fn class_context(&self) -> Option<&ClassContext> {
        self.class_context_stack.last()
    }

    /// Get a mutable reference to the current class context, if any.
    pub fn class_context_mut(&mut self) -> Option<&mut ClassContext> {
        self.class_context_stack.last_mut()
    }

    /// Whether any class feature (`sort_tailwindcss`, `wrap_class_names`) is on.
    /// With both off, no node opens a class context.
    pub fn has_class_features(&self) -> bool {
        self.has_class_features
    }

    /// Marks the class at `index` as wrapping to the print width.
    pub fn mark_class_wrapped(&mut self, index: usize) {
        self.wrapped_class_indices.push(index);
    }

    /// Take the indices marked with [`Self::mark_class_wrapped`].
    pub fn take_wrapped_class_indices(&mut self) -> Vec<usize> {
        mem::take(&mut self.wrapped_class_indices)
    }
}
