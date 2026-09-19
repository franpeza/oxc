//! Class string utilities for Tailwind CSS class sorting (`sort_tailwindcss`)
//! and class string wrapping (`wrap_class_names`).
//!
//! Both features share one class context and collect class strings within:
//! - JSX attributes (`className`, `class`, or custom attributes)
//! - Function calls (`clsx()`, `cn()`, `tw()`, or custom functions)
//! - Template literals with expressions
//!
//! Sorting is based on [prettier-plugin-tailwindcss](https://github.com/tailwindlabs/prettier-plugin-tailwindcss),
//! wrapping on [prettier-plugin-classnames](https://github.com/ony3000/prettier-plugin-classnames).

use oxc_ast::ast::*;
use oxc_formatter_core::FormatElement;
use oxc_span::{GetSpan, Span};

use crate::{
    Buffer,
    ast_nodes::{AstNode, AstNodes},
    best_fitting,
    formatter::{ClassContext, prelude::*},
    options::JsFormatOptions,
    write,
};

use super::string::{FormatLiteralStringToken, StringLiteralParentKind};

// ============================================================================
// Detection Functions
// ============================================================================

/// Returns the class context if the given string literal holds classes to sort or wrap.
///
/// Returns `Some(ctx)` when:
/// - We're inside a class context (attribute or function call)
/// - The context is not disabled (e.g., not inside a nested non-class call)
/// - The string contains whitespace (indicating multiple classes)
///
/// Wrapping is turned off outside a class position, see [`narrow_class_context`].
pub fn class_context_for_string_literal<'a>(
    string: &AstNode<'a, StringLiteral<'a>>,
    f: &JsFormatter<'_, 'a>,
) -> Option<ClassContext> {
    let ctx = f.context().class_context().copied().filter(|ctx| {
        let text = f.source_text().text_for(string);

        if ctx.disabled {
            return false;
        }

        text.as_bytes().iter().any(|&b| b.is_ascii_whitespace())
    })?;
    Some(narrow_class_context(ctx, string.span, string.ancestors(), f.options()))
        .filter(ClassContext::is_active)
}

/// Returns `ctx` for a string or template at `span`, with wrapping turned off
/// unless it sits in a class position.
///
/// In a class position, every node from the string up to the class
/// attribute, call or tag passes the value through unchanged (call argument,
/// conditional branch, logical or `+` operand, array element, object value,
/// `${}` of a template), like prettier-plugin-classnames. Anywhere else
/// (a comparison, `in`, a computed key, a `new` argument, a type, ...)
/// the string is not a class list, and adding newlines would change it.
pub fn narrow_class_context<'a, 'b>(
    ctx: ClassContext,
    mut span: Span,
    ancestors: impl Iterator<Item = &'b AstNodes<'a>>,
    options: &JsFormatOptions,
) -> ClassContext
where
    'a: 'b,
{
    if !ctx.wrap {
        return ctx;
    }
    for ancestor in ancestors {
        let passes_through = match ancestor {
            // The enclosing attribute or tag must be a class one itself: an element
            // or tagged template nested in a class call (`clsx(<div id="..." />)`)
            // holds no class list, and wrapping would change its value.
            AstNodes::JSXAttribute(attribute) => {
                return if class_attribute_context(&attribute.name, options).is_some() {
                    ctx
                } else {
                    ctx.without_wrap()
                };
            }
            AstNodes::TaggedTemplateExpression(tagged) => {
                return if class_function_context(&tagged.tag, options).is_some() {
                    ctx
                } else {
                    ctx.without_wrap()
                };
            }
            // Only an argument of a class call is a class position: in
            // `clsx(lookup(`a b`))` the template belongs to `lookup`.
            AstNodes::CallExpression(call) => {
                return if !call.callee.span().contains_inclusive(span)
                    && class_function_context(&call.callee, options).is_some()
                {
                    ctx
                } else {
                    ctx.without_wrap()
                };
            }
            AstNodes::ConditionalExpression(conditional) => {
                !conditional.test.span().contains_inclusive(span)
            }
            AstNodes::BinaryExpression(binary) => binary.operator == BinaryOperator::Addition,
            AstNodes::ObjectProperty(property) => {
                !property.computed && !property.key.span().contains_inclusive(span)
            }
            AstNodes::JSXExpressionContainer(_)
            | AstNodes::ParenthesizedExpression(_)
            | AstNodes::LogicalExpression(_)
            | AstNodes::ArrayExpression(_)
            | AstNodes::ObjectExpression(_)
            | AstNodes::TemplateLiteral(_) => true,
            _ => false,
        };
        if !passes_through {
            return ctx.without_wrap();
        }
        span = ancestor.span();
    }
    ctx.without_wrap()
}

/// Checks if a JSX attribute is a class attribute.
///
/// Returns `true` for:
/// - `class` and `className` (default attributes)
/// - Custom attributes specified in the feature's `attributes` option
pub fn is_class_attribute(attr_name: &JSXAttributeName<'_>, extra_attributes: &[String]) -> bool {
    let JSXAttributeName::Identifier(ident) = attr_name else {
        return false;
    };
    let name = ident.name.as_str();

    // Default: `class` and `className`
    if name == "class" || name == "className" {
        return true;
    }

    // Custom attributes from options
    extra_attributes.iter().any(|a| a == name)
}

/// Builds the class context for a JSX attribute targeted by
/// `sort_tailwindcss` and/or `wrap_class_names`.
pub fn class_attribute_context(
    attr_name: &JSXAttributeName<'_>,
    options: &JsFormatOptions,
) -> Option<ClassContext> {
    ClassContext::new(
        options
            .sort_tailwindcss
            .as_ref()
            .filter(|opts| is_class_attribute(attr_name, &opts.attributes)),
        options
            .wrap_class_names
            .as_ref()
            .is_some_and(|opts| is_class_attribute(attr_name, &opts.attributes)),
    )
}

/// Builds the class context for a call expression or tagged template
/// targeted by `sort_tailwindcss` and/or `wrap_class_names`.
pub fn class_function_context(
    callee: &Expression<'_>,
    options: &JsFormatOptions,
) -> Option<ClassContext> {
    ClassContext::new(
        options
            .sort_tailwindcss
            .as_ref()
            .filter(|opts| is_class_function_call(callee, &opts.functions)),
        options
            .wrap_class_names
            .as_ref()
            .is_some_and(|opts| is_class_function_call(callee, &opts.functions)),
    )
}

/// Checks if a callee expression is a class function.
///
/// Traverses through `CallExpression` and `MemberExpression` nodes to find the
/// root identifier, then checks if it matches any function in `functions`.
///
/// This matches patterns like:
/// - `clsx(...)` - direct call
/// - `clsx.foo(...)` - member expression
/// - `obj.clsx(...)` - object member
/// - `foo().clsx(...)` - chained calls
///
/// Based on [prettier-plugin-tailwindcss's `isSortableExpression`](https://github.com/tailwindlabs/prettier-plugin-tailwindcss/blob/28beb4e008b913414562addec4abb8ab261f3828/src/index.ts#L584-L605).
pub fn is_class_function_call(callee: &Expression<'_>, functions: &[String]) -> bool {
    if functions.is_empty() {
        return false;
    }

    // Traverse property accesses and function calls to find the leading identifier
    let mut node = callee;

    loop {
        match node {
            Expression::CallExpression(call) => {
                node = &call.callee;
            }
            Expression::StaticMemberExpression(member) => {
                node = &member.object;
            }
            Expression::ComputedMemberExpression(member) => {
                node = &member.object;
            }
            Expression::Identifier(ident) => {
                return functions.iter().any(|f| f == ident.name.as_str());
            }
            _ => return false,
        }
    }
}

// ============================================================================
// Whitespace Collapse Logic
// ============================================================================

/// Controls whether whitespace can be trimmed at string boundaries.
///
/// When `start` or `end` is `false`, a single space must be preserved
/// at that boundary to maintain proper class separation.
///
/// Based on [prettier-plugin-tailwindcss's `canCollapseWhitespaceIn`](https://github.com/tailwindlabs/prettier-plugin-tailwindcss/blob/28beb4e008b913414562addec4abb8ab261f3828/src/index.ts#L607-L648).
#[derive(Debug, Clone, Copy, Default)]
pub struct CollapseWhitespace {
    /// `true` = can trim leading whitespace, `false` = preserve one space
    pub start: bool,
    /// `true` = can trim trailing whitespace, `false` = preserve one space
    pub end: bool,
}

impl CollapseWhitespace {
    fn new() -> Self {
        Self { start: true, end: true }
    }
}

/// Determines whitespace collapse rules for a string/template literal based on context.
///
/// # Rules
///
/// 1. **Template expression context** (`${...}`):
///    - If quasi before doesn't end with whitespace → preserve leading space
///    - If quasi after doesn't start with whitespace → preserve trailing space
///
/// 2. **Binary concat context** (`a + "..." + b`):
///    - On left side of `+` → preserve trailing space (need separation from `+ right`)
///    - On right side of `+` → preserve leading space (need separation from `left +`)
///
/// # Examples
///
/// ```text
/// // Template expression - quasi "header" has no trailing whitespace
/// `header${x ? " active" : ""}`
/// //           ^ preserve leading space
///
/// // Binary concat - string in middle needs both spaces preserved
/// className={a + " p-4 " + b}
/// //             ^     ^ preserve both
///
/// // Binary concat - template on right side only
/// a + ` flex p-4`     // leading space preserved (from `a +`)
///     ^
///
/// // Binary concat - template on left side only
/// `flex p-4 ` + b     // trailing space preserved (for `+ b`)
///          ^
/// ```
pub fn can_collapse_whitespace<'a, 'b>(
    span: Span,
    ancestors: impl Iterator<Item = &'b AstNodes<'a>>,
    f: &JsFormatter<'_, 'a>,
) -> CollapseWhitespace
where
    'a: 'b,
{
    let mut collapse = CollapseWhitespace::new();

    // 1. Check template expression context (O(1) via context stack)
    if let Some(ctx) = f.context().class_context()
        && ctx.in_template_expression
    {
        if !ctx.quasi_before_has_trailing_ws {
            collapse.start = false;
        }
        if !ctx.quasi_after_has_leading_ws {
            collapse.end = false;
        }
    }

    // 2. Check binary concat context (walk parent chain)
    for ancestor in ancestors {
        match ancestor {
            AstNodes::BinaryExpression(binary) if binary.operator() == BinaryOperator::Addition => {
                let left = binary.left().span();
                let right = binary.right().span();

                // Left operand needs trailing space for separation from `+ right`
                if left.contains_inclusive(span) {
                    collapse.end = false;
                }
                // Right operand needs leading space for separation from `left +`
                if right.contains_inclusive(span) {
                    collapse.start = false;
                }

                // Both flags are one-way latches; no need to continue once both are set.
                if !collapse.start && !collapse.end {
                    break;
                }
            }
            // Transparent nodes: skip through to find outer BinaryExpression(+)
            AstNodes::ConditionalExpression(_)
            | AstNodes::ParenthesizedExpression(_)
            | AstNodes::TSAsExpression(_)
            | AstNodes::TSSatisfiesExpression(_)
            | AstNodes::TSNonNullExpression(_)
            | AstNodes::TSTypeAssertion(_) => {}
            _ => break,
        }
    }

    collapse
}

// ============================================================================
// Write Functions
// ============================================================================

/// Writes a class marker element. A wrapping marker is recorded on the
/// context, so `format()` expands it into a fill after sorting.
///
/// With `indent`, wrapped lines sit one level deeper than the current
/// indentation; otherwise they align with it.
fn write_class_marker(f: &mut JsFormatter<'_, '_>, index: usize, wrap: bool, indent: bool) {
    if !wrap {
        f.write_element(FormatElement::TailwindClass(index));
        return;
    }
    f.context_mut().mark_class_wrapped(index);
    let marker = format_with(|f| f.write_element(FormatElement::TailwindClass(index)));
    if indent {
        write!(f, [crate::formatter::prelude::indent(&marker)]);
    } else {
        write!(f, [marker]);
    }
}

/// Whether the wrapped lines of a class string (`span`, with this parent)
/// sit one level deeper than the line the string starts on.
///
/// Like prettier-plugin-classnames, that is the case for an attribute's own
/// value (`className="..."`, `` className={`...`} ``) and for any string
/// inside a ternary, even one nested in a call there. Elsewhere (`clsx(...)`
/// arguments, tagged templates) the wrapped lines keep the start line's
/// indentation.
fn wraps_one_level_deeper(span: Span, parent: &AstNodes<'_>) -> bool {
    if matches!(parent, AstNodes::JSXAttribute(_) | AstNodes::JSXExpressionContainer(_)) {
        return true;
    }
    let mut span = span;
    let mut direct = true;
    for ancestor in parent.ancestors() {
        match ancestor {
            // A branch printed right after `? `/`: ` already gets the extra
            // level from the ternary's own alignment.
            AstNodes::ConditionalExpression(conditional) => {
                return !direct || conditional.test.span().contains_inclusive(span);
            }
            AstNodes::ParenthesizedExpression(_) => {}
            _ => direct = false,
        }
        span = ancestor.span();
    }
    false
}

/// Collects a class string for a marker and returns its index.
///
/// In wrapping contexts, whitespace runs (including newlines left by a
/// previous wrap) collapse to single spaces, so the unwrapped form is always
/// a legal single-line string. Only sorting contexts are sent to the sorter.
fn add_class_content(f: &mut JsFormatter<'_, '_>, trimmed: &str, ctx: ClassContext) -> usize {
    let content =
        if ctx.wrap && trimmed.as_bytes().iter().any(|b| b.is_ascii_whitespace() && *b != b' ') {
            let mut out = String::with_capacity(trimmed.len());
            for class in trimmed.split_ascii_whitespace() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(class);
            }
            out
        } else {
            trimmed.to_string()
        };
    if ctx.sort {
        f.context_mut().add_tailwind_class(content)
    } else {
        f.context_mut().add_unsorted_class(content)
    }
}

/// Whether a class string's content can safely swap delimiters
/// (quote <-> backtick) without any re-escaping analysis.
///
/// Backslashes rule out escape sequences, backticks and `${` rule out
/// template collisions. Real-world class lists contain none of these.
fn is_convertible_class_content(content: &str) -> bool {
    !content.contains(['`', '\\']) && !content.contains("${")
}

/// Whether a JSX attribute string may hold an HTML character reference
/// (`&amp;`, `&#38;`, `&#x26;`). JSX decodes those in an attribute string but
/// not in a template literal, so converting such a string would change its value.
///
/// Conservative: any `&` followed by `;` before the next whitespace counts.
/// Tailwind's arbitrary variants (`[&_svg]:size-4`) have no `;` and still convert.
fn has_html_character_reference(content: &str) -> bool {
    content.match_indices('&').any(|(index, _)| {
        content[index + 1..]
            .bytes()
            .take_while(|byte| !byte.is_ascii_whitespace() && *byte != b'&')
            .any(|byte| byte == b';')
    })
}

/// Which boundary spaces a class string keeps: `(leading, trailing)`.
///
/// Sorting trims them unless the surrounding concatenation needs the space
/// (prettier-plugin-tailwindcss); with `keep`, a wrap-only context in an
/// expression keeps them (prettier-plugin-classnames).
fn boundary_spaces(content: &str, collapse: CollapseWhitespace, keep: bool) -> (bool, bool) {
    let leading = content.starts_with(|c: char| c.is_ascii_whitespace());
    let trailing = content.ends_with(|c: char| c.is_ascii_whitespace());
    (leading && (!collapse.start || keep), trailing && (!collapse.end || keep))
}

/// Writes a class string outside a JSX attribute value, where a quoted string
/// cannot contain a newline: a `best_fitting` pair of the quoted string and a
/// backtick template whose classes wrap to the print width.
///
/// The printer picks the quoted form whenever it fits, so `` `a b` ``
/// normalizes to `"a b"` and a long `"..."` becomes a wrapped template,
/// like prettier-plugin-classnames' delimiter conversion.
///
/// The quoted form gets its own class entry that never wraps: a buffer that
/// removes soft line breaks (e.g. inside `${}`) prints it without flat mode,
/// so a shared, wrapping entry could break inside the quotes.
fn write_convertible_class_string(
    delimiters: ConvertibleDelimiters,
    trimmed: &str,
    ctx: ClassContext,
    spaces: (bool, bool),
    indent: bool,
    f: &mut JsFormatter<'_, '_>,
) {
    let ConvertibleDelimiters { quote, wrapped_open, wrapped_close } = delimiters;
    let (leading_space, trailing_space) = spaces;
    let flat_index = add_class_content(f, trimmed, ctx);
    let index = add_class_content(f, trimmed, ctx);
    let flat = format_with(move |f| {
        write!(f, quote);
        if leading_space {
            write!(f, text(" "));
        }
        f.write_element(FormatElement::TailwindClass(flat_index));
        if trailing_space {
            write!(f, text(" "));
        }
        write!(f, quote);
        // Like the plugin, the quoted form fits when the string itself does,
        // whatever follows it on the line (`,`, `)`).
        f.write_element(FormatElement::MeasureAlone);
    });
    let wrapped = format_with(move |f| {
        write!(f, wrapped_open);
        if leading_space {
            write!(f, text(" "));
        }
        write_class_marker(f, index, true, indent);
        if trailing_space {
            write!(f, text(" "));
        }
        write!(f, wrapped_close);
    });
    write!(f, [best_fitting!(flat, wrapped)]);
}

/// The delimiters of [`write_convertible_class_string`]'s two forms.
#[derive(Clone, Copy)]
struct ConvertibleDelimiters {
    /// The quote of the form that fits on one line.
    quote: &'static str,
    /// Opens the wrapped form: a template, or a JSX expression container
    /// holding one (`syntax_transformation`).
    wrapped_open: &'static str,
    /// Closes the wrapped form. The expansion glues it onto the last class,
    /// so its width counts toward that line.
    wrapped_close: &'static str,
}

impl ConvertibleDelimiters {
    /// `"a b"` <-> `` `a b` ``
    fn template(quote: &'static str) -> Self {
        Self { quote, wrapped_open: "`", wrapped_close: "`" }
    }

    /// `class="a b"` <-> `` class={`a b`} `` (`syntax_transformation`)
    fn jsx_expression(quote: &'static str) -> Self {
        Self { quote, wrapped_open: "{`", wrapped_close: "`}" }
    }
}

/// Writes a string literal holding class names (sorted and/or wrapped).
///
/// Handles whitespace based on context:
/// - Trims and normalizes whitespace by default
/// - Preserves boundary spaces when required by concat/template context
/// - With `preserve_whitespace`, outputs content unchanged
pub fn write_class_string_literal<'a>(
    string_literal: &AstNode<'a, StringLiteral<'a>>,
    ctx: ClassContext,
    f: &mut JsFormatter<'_, 'a>,
) {
    debug_assert!(
        !string_literal.value.is_empty(),
        "Empty string literals should be skipped for class handling"
    );

    let normalized_string = FormatLiteralStringToken::new(
        f.source_text().text_for(&string_literal),
        // `className="string"`
        //            ^^^^^^^^
        matches!(string_literal.parent(), AstNodes::JSXAttribute(_)),
        StringLiteralParentKind::Expression,
    )
    .clean_text(f);

    let quote = normalized_string.as_bytes()[0];
    let quote = match quote {
        b'\'' => "\'",
        b'"' => "\"",
        _ => unreachable!("Unexpected quote character in string literal"),
    };

    // At least three characters: opening quote, content, closing quote
    let content = &normalized_string[1..normalized_string.len() - 1];

    if ctx.preserve_whitespace {
        write!(f, quote);
        let index = f.context_mut().add_tailwind_class(content.to_string());
        f.write_element(FormatElement::TailwindClass(index));
        write!(f, quote);
        return;
    }

    let trimmed = content.trim();

    // Whitespace-only → normalize to single space
    if trimmed.is_empty() {
        write!(f, quote);
        if !content.is_empty() {
            write!(f, text(" "));
        }
        write!(f, quote);
        return;
    }

    let is_jsx_attribute = matches!(string_literal.parent(), AstNodes::JSXAttribute(_));

    let collapse = can_collapse_whitespace(string_literal.span, string_literal.ancestors(), f);
    // JSX attribute values always trim their boundary whitespace.
    let (leading_space, trailing_space) =
        boundary_spaces(content, collapse, !ctx.sort && !is_jsx_attribute);

    // With `syntax_transformation`, a JSX attribute string that has to wrap
    // becomes an expression holding a template instead of wrapping in place.
    if ctx.wrap
        && is_jsx_attribute
        && f.options().wrap_class_names.as_ref().is_some_and(|opts| opts.syntax_transformation)
        && is_convertible_class_content(content)
        && !has_html_character_reference(content)
    {
        write_convertible_class_string(
            ConvertibleDelimiters::jsx_expression(quote),
            trimmed,
            ctx,
            (leading_space, trailing_space),
            true,
            f,
        );
        return;
    }

    // A JSX attribute string may contain newlines and wraps in place;
    // elsewhere wrapping goes through the delimiter conversion.
    if ctx.wrap && !is_jsx_attribute && is_convertible_class_content(content) {
        let indent = wraps_one_level_deeper(string_literal.span, string_literal.parent());
        write_convertible_class_string(
            ConvertibleDelimiters::template(quote),
            trimmed,
            ctx,
            (leading_space, trailing_space),
            indent,
            f,
        );
        return;
    }

    let index = add_class_content(f, trimmed, ctx);

    let string = format_with(|f| {
        write!(f, quote);

        // Leading space
        if leading_space {
            write!(f, text(" "));
        }

        // Class content
        write_class_marker(f, index, ctx.wrap && is_jsx_attribute, true);

        // Trailing space
        if trailing_space {
            write!(f, text(" "));
        }
        write!(f, quote);
    });
    write!(f, [string]);
}

/// Whether a JSX expression container holds a string literal that
/// `write_class_string_literal` writes with [`write_convertible_class_string`].
///
/// Such containers hug their braces like template literals do
/// (`` className={`...`} ``), since the wrapped form is a template.
pub fn is_wrap_convertible_string_container<'a>(
    container: &AstNode<'a, JSXExpressionContainer<'a>>,
    f: &JsFormatter<'_, 'a>,
) -> bool {
    let JSXExpression::StringLiteral(string) = &container.expression else {
        return false;
    };
    let Some(ctx) = f.context().class_context() else {
        return false;
    };
    if !ctx.wrap || ctx.disabled || ctx.preserve_whitespace {
        return false;
    }
    // The container must be a class attribute's own value: an attribute of an
    // element nested in a class call (`clsx(<div id={"a b"} />)`) never wraps.
    let AstNodes::JSXAttribute(attribute) = container.parent() else {
        return false;
    };
    if class_attribute_context(&attribute.name, f.options()).is_none_or(|ctx| !ctx.wrap) {
        return false;
    }
    let text = f.source_text().text_for(string.as_ref());
    text.as_bytes().iter().any(u8::is_ascii_whitespace)
        && is_convertible_class_content(&text[1..text.len() - 1])
}

/// Writes an untagged, expression-free template literal in a wrapping class
/// context with [`write_convertible_class_string`].
///
/// Returns `false` (writing nothing) when the template is not eligible, so
/// the caller falls through to the regular template formatting.
pub fn try_wrap_convert_template<'a>(
    template: &AstNode<'a, TemplateLiteral<'a>>,
    f: &mut JsFormatter<'_, 'a>,
) -> bool {
    let Some(ctx) = f.context().class_context().copied() else {
        return false;
    };
    let ctx = narrow_class_context(ctx, template.span, template.ancestors(), f.options());
    if !ctx.wrap || ctx.disabled || ctx.preserve_whitespace {
        return false;
    }
    if !template.expressions.is_empty() || template.quasis.len() != 1 {
        return false;
    }

    let content = template.quasis[0].value.raw.as_str();
    // Multiple classes only (single class strings keep their source form),
    // and the quoted variant must need no re-escaping analysis at all.
    if !content.as_bytes().iter().any(u8::is_ascii_whitespace) {
        return false;
    }
    if !is_convertible_class_content(content) || content.contains(['"', '\'']) {
        return false;
    }
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }

    let collapse = can_collapse_whitespace(template.span, template.ancestors(), f);
    let (leading_space, trailing_space) = boundary_spaces(content, collapse, !ctx.sort);

    let quote = f.options().quote_style.as_str();
    let indent = wraps_one_level_deeper(template.span, template.parent());
    write_convertible_class_string(
        ConvertibleDelimiters::template(quote),
        trimmed,
        ctx,
        (leading_space, trailing_space),
        indent,
        f,
    );
    true
}

/// Writes a template element (quasi) holding class names (sorted and/or wrapped).
///
/// Template elements need special handling because classes can "touch"
/// adjacent expressions without whitespace separation:
///
/// ```text
/// `${variant}items-center p-4`
/// //         ^^^^^^^^^^^^ "items-center" touches ${variant}, not sorted
/// //                      ^^^ "p-4" is separated by space, will be sorted
/// ```
///
/// # Content Structure
///
/// Content is split into three parts:
/// - **prefix**: Class touching previous expression (not sorted)
/// - **sortable**: Classes separated by whitespace (sorted)
/// - **suffix**: Class touching next expression (not sorted)
///
/// Based on [prettier-plugin-tailwindcss](https://github.com/tailwindlabs/prettier-plugin-tailwindcss/blob/28beb4e008b913414562addec4abb8ab261f3828/src/index.ts#L511-L566).
pub fn write_class_template_element<'a>(
    element: &AstNode<'a, TemplateElement<'a>>,
    ctx: ClassContext,
    f: &mut JsFormatter<'_, 'a>,
) {
    let content = f.source_text().text_for(element);

    // Get quasi position from context (set when the quasi was written in template.rs)
    let is_first = ctx.is_first_quasi;
    let is_last = ctx.is_last_quasi;

    // Split into prefix/sortable/suffix.
    // Classes glued to an adjacent `${...}` expression must stay in place even when preserving whitespace;
    let (prefix, sortable, suffix) = split_template_content(content, is_first, is_last);

    // Write prefix (unsorted class touching previous expression)
    if !prefix.is_empty() {
        write!(f, text(prefix));
    }

    // Write sortable content
    if ctx.preserve_whitespace {
        // Whitespace (including whitespace-only content) round-trips through the sorter unchanged
        let index = f.context_mut().add_tailwind_class(sortable.to_string());
        f.write_element(FormatElement::TailwindClass(index));
    } else if sortable.trim().is_empty() {
        // Whitespace-only → normalize to single space
        if !sortable.is_empty() {
            write!(f, text(" "));
        }
    } else {
        // Check if binary expression context requires preserving boundary whitespace
        let collapse = can_collapse_whitespace_template(element, is_first, is_last, f);
        let has_leading_ws = sortable.starts_with(|c: char| c.is_ascii_whitespace());
        let has_trailing_ws = sortable.ends_with(|c: char| c.is_ascii_whitespace());

        // Leading space: required if not at start of template, or if binary context requires it
        let need_leading = !is_first || !prefix.is_empty() || (has_leading_ws && !collapse.start);
        if need_leading {
            write!(f, text(" "));
        }

        // Template literals may contain literal newlines, so the sortable
        // middle section can wrap; the boundary prefix/suffix and the
        // separating spaces stay as literal text outside the marker.
        let index = add_class_content(f, sortable.trim(), ctx);
        let indent = match element.parent() {
            AstNodes::TemplateLiteral(template) => {
                wraps_one_level_deeper(template.span(), template.parent())
            }
            _ => false,
        };
        write_class_marker(f, index, ctx.wrap, indent);

        // Trailing space: required if not at end of template, or if binary context requires it
        let need_trailing = !is_last || !suffix.is_empty() || (has_trailing_ws && !collapse.end);
        if need_trailing {
            write!(f, text(" "));
        }
    }

    // Write suffix (unsorted class touching next expression)
    if !suffix.is_empty() {
        write!(f, text(suffix));
    }
}

/// Determines whitespace collapse rules for a template element based on binary expression context.
fn can_collapse_whitespace_template<'a>(
    element: &AstNode<'a, TemplateElement<'a>>,
    is_first: bool,
    is_last: bool,
    f: &JsFormatter<'_, 'a>,
) -> CollapseWhitespace {
    // Only first/last quasis can be affected by binary expression context
    if !is_first && !is_last {
        return CollapseWhitespace::new();
    }

    can_collapse_whitespace(element.span, element.ancestors().skip(1), f)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Splits template content into (prefix, sortable, suffix).
///
/// - **prefix**: First class if it touches previous expression (no leading whitespace)
/// - **suffix**: Last class if it touches next expression (no trailing whitespace)
/// - **sortable**: Everything in between
///
/// # Examples
///
/// ```text
/// // Input: "items-center p-4 m-2", is_first=false, is_last=true
/// // "items-center" touches prev expr → prefix
/// // "p-4 m-2" will be sorted → sortable
/// // No suffix (is_last=true)
/// Result: ("items-center", " p-4 m-2", "")
///
/// // Input: " flex p-4", is_first=true, is_last=true
/// // Leading space → no prefix
/// // Everything is sortable
/// Result: ("", " flex p-4", "")
/// ```
fn split_template_content(content: &str, is_first: bool, is_last: bool) -> (&str, &str, &str) {
    let has_leading_ws = content.starts_with(|c: char| c.is_ascii_whitespace());
    let has_trailing_ws = content.ends_with(|c: char| c.is_ascii_whitespace());

    // Determine what to ignore (not sort)
    let has_prefix = !is_first && !has_leading_ws;
    let has_suffix = !is_last && !has_trailing_ws;

    // Find split points
    let prefix_end =
        if has_prefix { content.find(|c: char| c.is_ascii_whitespace()) } else { None };
    let suffix_start = if has_suffix {
        content.rfind(|c: char| c.is_ascii_whitespace()).map(|i| i + 1)
    } else {
        None
    };

    match (prefix_end, suffix_start) {
        // Both prefix and suffix
        (Some(pe), Some(ss)) if pe < ss => (&content[..pe], &content[pe..ss], &content[ss..]),
        // Only prefix (suffix_start overlaps or doesn't exist)
        (Some(pe), _) => (&content[..pe], &content[pe..], ""),
        // Only suffix
        (None, Some(ss)) => ("", &content[..ss], &content[ss..]),
        // Neither
        (None, None) => ("", content, ""),
    }
}
