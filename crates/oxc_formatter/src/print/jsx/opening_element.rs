use std::ops::Deref;

use oxc_ast::ast::{JSXAttributeItem, JSXAttributeValue, JSXOpeningElement, StringLiteral};
use oxc_span::GetSpan;

use crate::{
    ast_nodes::AstNode,
    best_fitting, format_args,
    formatter::{prelude::*, trivia::FormatTrailingComments},
    options::JsFormatOptions,
    utils::tailwindcss::class_attribute_context,
    write,
};

pub struct FormatOpeningElement<'a, 'b> {
    element: &'b AstNode<'a, JSXOpeningElement<'a>>,
    is_self_closing: bool,
}

impl<'a> Deref for FormatOpeningElement<'a, '_> {
    type Target = AstNode<'a, JSXOpeningElement<'a>>;

    fn deref(&self) -> &Self::Target {
        self.element
    }
}

impl<'a, 'b> FormatOpeningElement<'a, 'b> {
    pub fn new(element: &'b AstNode<'a, JSXOpeningElement<'a>>, is_self_closing: bool) -> Self {
        Self { element, is_self_closing }
    }

    fn compute_layout(&self, f: &JsFormatter<'_, 'a>) -> OpeningElementLayout {
        let attributes = self.element.attributes();

        let comments = f.context().comments();

        let last_attribute_has_comment = self
            .attributes
            .last()
            .is_some_and(|a| comments.has_comment_in_range(a.span().end, self.span.end));

        let type_arguments_or_name_end =
            self.type_arguments().map_or_else(|| self.name.span().end, |t| t.span.end);
        let first_attribute_start_or_element_end =
            self.attributes.first().map_or_else(|| self.span.end, |a| a.span().start);
        let name_has_comment = comments
            .has_comment_in_range(type_arguments_or_name_end, first_attribute_start_or_element_end);

        if self.is_self_closing && attributes.is_empty() && !name_has_comment {
            OpeningElementLayout::Inline
        } else if attributes.len() == 1 && !name_has_comment && !last_attribute_has_comment {
            if is_single_line_string_literal_attribute(&attributes[0], f.options()) {
                OpeningElementLayout::SingleStringAttribute
            } else if is_wrapped_class_attribute(&attributes[0], f.options())
                && as_string_literal_attribute_value(&attributes[0]).is_some()
            {
                OpeningElementLayout::SingleWrappedClassAttribute
            } else {
                OpeningElementLayout::IndentAttributes {
                    name_has_comment,
                    last_attribute_has_comment,
                }
            }
        } else {
            OpeningElementLayout::IndentAttributes { name_has_comment, last_attribute_has_comment }
        }
    }
}

/// Returns `true` if this is an attribute with a [`StringLiteral`] initializer that contains at least one new line character.
///
/// A wrapped class attribute (`wrap_class_names`) is never multiline: its
/// whitespace, including newlines from a previous wrap, is collapsed before
/// printing, so the source newlines must not decide the layout.
fn is_multiline_string_literal_attribute(
    attribute: &JSXAttributeItem<'_>,
    options: &JsFormatOptions,
) -> bool {
    let JSXAttributeItem::Attribute(attr) = attribute else {
        return false;
    };
    // The value check comes first: it is what this layout check has always
    // done, and it is almost always false, so the class lookup rarely runs.
    attr.value.as_ref().is_some_and(|value| matches!(value, JSXAttributeValue::StringLiteral(string) if string.value.contains('\n')))
        && !is_wrapped_class_attribute(attribute, options)
}

/// Returns `true` if `wrap_class_names` wraps this attribute's value
/// (strings kept verbatim by `sortTailwindcss.preserveWhitespace` are not wrapped).
fn is_wrapped_class_attribute(attribute: &JSXAttributeItem<'_>, options: &JsFormatOptions) -> bool {
    // With wrapping off, the cheapest check already decides it.
    if options.wrap_class_names.is_none() {
        return false;
    }
    let JSXAttributeItem::Attribute(attr) = attribute else {
        return false;
    };
    class_attribute_context(&attr.name, options)
        .is_some_and(|ctx| ctx.wrap && !ctx.preserve_whitespace)
}

impl<'a> Format<'a, JsFormatContext<'a>> for FormatOpeningElement<'a, '_> {
    fn fmt(&self, f: &mut JsFormatter<'_, 'a>) {
        let layout = self.compute_layout(f);

        let attributes = self.attributes();

        let format_open = format_with(|f| write!(f, ["<", self.name(), self.type_arguments(),]));
        let format_close = format_with(|f| write!(f, [self.is_self_closing.then_some("/"), ">"]));

        match layout {
            OpeningElementLayout::Inline => {
                write!(f, [format_open, space(), format_close]);
            }
            OpeningElementLayout::SingleStringAttribute => {
                let attribute_spacing = if self.is_self_closing { Some(space()) } else { None };
                write!(
                    f,
                    [format_open, space(), self.attributes(), attribute_spacing, format_close]
                );
            }
            OpeningElementLayout::SingleWrappedClassAttribute => {
                // Like Prettier with prettier-plugin-classnames: the attribute
                // stays on the tag line when the class string itself fits there,
                // even if the closing `>` or `/>` then overflows.
                let hugged = format_with(|f| {
                    write!(f, [format_open, space(), self.attributes()]);
                    if self.is_self_closing {
                        write!(f, [space()]);
                    }
                    f.write_element(FormatElement::MeasureAlone);
                });
                let broken = format_with(|f| {
                    write!(
                        f,
                        [format_open, indent(&format_args!(hard_line_break(), self.attributes()))]
                    );
                    if self.is_self_closing || !f.options().bracket_same_line.value() {
                        write!(f, [hard_line_break()]);
                    }
                });
                write!(f, [best_fitting!(hugged, broken), format_close]);
            }
            OpeningElementLayout::IndentAttributes {
                name_has_comment,
                last_attribute_has_comment,
            } => {
                let format_inner = format_with(|f| {
                    write!(f, [format_open]);

                    let attributes = self.attributes();
                    if !attributes.is_empty() {
                        write!(f, [soft_line_indent_or_space(&attributes)]);
                    }

                    let comments = f.context().comments().comments_before(self.span.end);
                    FormatTrailingComments::Comments(comments).fmt(f);

                    let force_bracket_same_line = f.options().bracket_same_line.value();
                    let wants_bracket_same_line = attributes.is_empty() && !name_has_comment;

                    if self.is_self_closing {
                        write!(f, [soft_line_break_or_space(), format_close]);
                    } else if last_attribute_has_comment {
                        write!(f, [soft_line_break(), format_close]);
                    } else if (force_bracket_same_line && !self.attributes.is_empty())
                        || wants_bracket_same_line
                    {
                        write!(f, [format_close]);
                    } else {
                        write!(f, [soft_line_break(), format_close]);
                    }
                });

                let has_multiline_string_attribute = attributes
                    .iter()
                    .any(|attribute| is_multiline_string_literal_attribute(attribute, f.options()));
                write!(f, [group(&format_inner).should_expand(has_multiline_string_attribute)]);
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum OpeningElementLayout {
    /// Don't create a group around the element to avoid it breaking ever.
    ///
    /// Applied for elements that have no attributes nor any comment attached to their name.
    ///
    /// ```javascript
    /// <ASuperLongComponentNameThatWouldBreakButDoesntSinceTheComponent<DonTBreakThis>></ASuperLongComponentNameThatWouldBreakButDoesntSinceTheComponent>
    /// ```
    Inline,

    /// Opening element with a single attribute that contains no line breaks, nor has comments.
    ///
    /// ```javascript
    /// <div tooltip="A very long tooltip text that would otherwise make the attribute break onto the same line but it is not because of the single string layout" more></div>;
    /// ```
    SingleStringAttribute,

    /// Opening element with a single class attribute that `wrap_class_names` wraps.
    ///
    /// The attribute stays on the tag line while its class string fits there;
    /// otherwise it moves onto its own line, where the classes wrap.
    SingleWrappedClassAttribute,

    /// Default layout that indents the attributes and formats each attribute on its own line.
    ///
    /// ```javascript
    /// <div
    ///   oneAttribute
    ///   another="with value"
    ///   moreAttributes={withSomeExpression}
    /// ></div>;
    /// ```
    IndentAttributes { name_has_comment: bool, last_attribute_has_comment: bool },
}

/// Returns `true` if this is an attribute with a string literal initializer that does not contain any new line characters.
///
/// A wrapped class attribute with several classes is excluded: it takes
/// [`OpeningElementLayout::SingleWrappedClassAttribute`], which can move it
/// onto its own line.
fn is_single_line_string_literal_attribute(
    attribute: &JSXAttributeItem<'_>,
    options: &JsFormatOptions,
) -> bool {
    as_string_literal_attribute_value(attribute).is_some_and(|string| {
        if is_wrapped_class_attribute(attribute, options) {
            string.value.split_ascii_whitespace().nth(1).is_none()
        } else {
            !string.value.contains('\n')
        }
    })
}

/// Returns `Some` if the initializer value of this attribute is a string literal.
/// Returns [None] otherwise.
fn as_string_literal_attribute_value<'a>(
    attribute: &'a JSXAttributeItem<'a>,
) -> Option<&'a StringLiteral<'a>> {
    match attribute {
        JSXAttributeItem::Attribute(attr) => {
            if let Some(JSXAttributeValue::StringLiteral(string)) = &attr.value {
                Some(string.as_ref())
            } else {
                None
            }
        }
        JSXAttributeItem::SpreadAttribute(_) => None,
    }
}
