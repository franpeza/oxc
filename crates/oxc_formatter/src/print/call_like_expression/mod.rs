mod arguments;

use oxc_ast::ast::*;

use crate::{
    ast_nodes::AstNode,
    formatter::prelude::*,
    utils::{
        call_expression::{callee_opener, is_test_call_expression},
        member_chain::MemberChain,
        statement_body::FormatBeforeOpener,
        tailwindcss::class_function_context,
    },
    write,
};
use arguments::{is_simple_module_import, is_verbatim_multiline_template_sole_arg};

use super::FormatWrite;

impl<'a> FormatWrite<'a> for AstNode<'a, CallExpression<'a>> {
    fn write(&self, f: &mut JsFormatter<'_, 'a>) {
        let callee = self.callee();
        let type_arguments = self.type_arguments();
        let arguments = self.arguments();

        // Check if this is a class-context function call (e.g., clsx, cn, tw)
        // for Tailwind sorting and/or class wrapping
        let class_call_ctx = if f.context().has_class_features() {
            class_function_context(&self.callee, f.options())
        } else {
            None
        };

        // For nested non-class calls inside a class context, disable class handling
        // to prevent sorting strings inside the nested call's arguments.
        // (e.g., `classNames("a", x.includes("\n") ? "b" : "c")` - don't sort "\n")
        let was_disabled = if class_call_ctx.is_none()
            && let Some(ctx) = f.context_mut().class_context_mut()
        {
            let was = ctx.disabled;
            ctx.disabled = true;
            Some(was)
        } else {
            None
        };

        if matches!(
            callee.as_ref(),
            Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_)
        ) && !is_verbatim_multiline_template_sole_arg(arguments.as_slice(), Some(self), f)
            && !is_simple_module_import(self.arguments(), f.comments())
            && !is_test_call_expression(self, f.comments())
        {
            MemberChain::from_call_expression(self, f).fmt(f);
        } else {
            let format_inner = format_with(|f| {
                write!(
                    f,
                    [
                        FormatBeforeOpener(callee, callee_opener(self)),
                        self.optional.then_some("?."),
                        type_arguments
                    ]
                );

                // If this IS a class-context call, push the class context
                // before formatting arguments
                if let Some(ctx) = class_call_ctx {
                    f.context_mut().push_class_context(ctx);
                }

                write!(f, arguments);

                // Pop class context after formatting
                if class_call_ctx.is_some() {
                    f.context_mut().pop_class_context();
                }
            });
            if matches!(callee.as_ref(), Expression::CallExpression(_)) {
                write!(f, [group(&format_inner)]);
            } else {
                write!(f, [format_inner]);
            }
        }

        // Restore the previous disabled state
        if let Some(was) = was_disabled
            && let Some(ctx) = f.context_mut().class_context_mut()
        {
            ctx.disabled = was;
        }
    }
}

impl<'a> FormatWrite<'a> for AstNode<'a, NewExpression<'a>> {
    fn write(&self, f: &mut JsFormatter<'_, 'a>) {
        write!(f, ["new", space()]);
        if let Some(type_arguments) = self.type_arguments() {
            write!(f, [FormatBeforeOpener(self.callee(), b'<'), type_arguments]);
        } else {
            write!(f, [self.callee()]);
        }
        write!(f, self.arguments());
    }
}

impl<'a> FormatWrite<'a> for AstNode<'a, ImportExpression<'a>> {
    fn write(&self, f: &mut JsFormatter<'_, 'a>) {
        write!(f, ["import"]);
        if let Some(phase) = &self.phase() {
            write!(f, [".", phase.as_str()]);
        }

        // Use the same logic as CallExpression arguments formatting
        write!(f, self.to_arguments());
    }
}
