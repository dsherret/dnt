// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

use deno_ast::swc::common::Spanned;
use deno_ast::view::*;

// todo(dsherret): unit tests

pub fn is_directly_in_condition(node: Node) -> bool {
  match node.parent() {
    Some(Node::BinExpr(_)) => true,
    Some(Node::IfStmt(_)) => true,
    Some(Node::UnaryExpr(expr)) => expr.op() == UnaryOp::TypeOf,
    Some(Node::CondExpr(cond_expr)) => {
      cond_expr.test.span().contains(node.span())
    }
    _ => false,
  }
}

pub fn is_in_left_hand_assignment(node: Node) -> bool {
  for ancestor in node.ancestors() {
    if let Node::AssignExpr(expr) = ancestor {
      return expr.left.span().contains(node.span());
    }
  }
  false
}

pub fn is_in_type(mut node: Node) -> bool {
  // todo: add unit tests and investigate if there's something in swc that does this?
  while let Some(parent) = node.parent() {
    let is_type = match parent {
      Node::ArrayLit(_)
      | Node::ArrayPat(_)
      | Node::ArrowExpr(_)
      | Node::AssignExpr(_)
      | Node::AssignPat(_)
      | Node::AssignPatProp(_)
      | Node::AssignProp(_)
      | Node::AwaitExpr(_)
      | Node::BinExpr(_)
      | Node::BindingIdent(_)
      | Node::BlockStmt(_)
      | Node::BreakStmt(_)
      | Node::CallExpr(_)
      | Node::CatchClause(_)
      | Node::Class(_)
      | Node::ClassDecl(_)
      | Node::ClassExpr(_)
      | Node::ClassMethod(_)
      | Node::ClassProp(_)
      | Node::ComputedPropName(_)
      | Node::CondExpr(_)
      | Node::Constructor(_)
      | Node::ContinueStmt(_)
      | Node::DebuggerStmt(_)
      | Node::Decorator(_)
      | Node::DoWhileStmt(_)
      | Node::EmptyStmt(_)
      | Node::ExportAll(_)
      | Node::ExportDecl(_)
      | Node::ExportDefaultDecl(_)
      | Node::ExportDefaultExpr(_)
      | Node::ExportDefaultSpecifier(_)
      | Node::ExportNamedSpecifier(_)
      | Node::ExportNamespaceSpecifier(_)
      | Node::ExprOrSpread(_)
      | Node::ExprStmt(_)
      | Node::FnDecl(_)
      | Node::FnExpr(_)
      | Node::ForInStmt(_)
      | Node::ForOfStmt(_)
      | Node::ForStmt(_)
      | Node::Function(_)
      | Node::GetterProp(_)
      | Node::IfStmt(_)
      | Node::ImportDecl(_)
      | Node::ImportDefaultSpecifier(_)
      | Node::ImportNamedSpecifier(_)
      | Node::ImportStarAsSpecifier(_)
      | Node::Invalid(_)
      | Node::UnaryExpr(_)
      | Node::UpdateExpr(_)
      | Node::VarDecl(_)
      | Node::VarDeclarator(_)
      | Node::WhileStmt(_)
      | Node::WithStmt(_)
      | Node::YieldExpr(_)
      | Node::JSXAttr(_)
      | Node::JSXClosingElement(_)
      | Node::JSXClosingFragment(_)
      | Node::JSXElement(_)
      | Node::JSXEmptyExpr(_)
      | Node::JSXExprContainer(_)
      | Node::JSXFragment(_)
      | Node::JSXMemberExpr(_)
      | Node::JSXNamespacedName(_)
      | Node::JSXOpeningElement(_)
      | Node::JSXOpeningFragment(_)
      | Node::JSXSpreadChild(_)
      | Node::JSXText(_)
      | Node::KeyValuePatProp(_)
      | Node::KeyValueProp(_)
      | Node::LabeledStmt(_)
      | Node::MetaPropExpr(_)
      | Node::MethodProp(_)
      | Node::Module(_)
      | Node::NamedExport(_)
      | Node::NewExpr(_)
      | Node::ObjectLit(_)
      | Node::ObjectPat(_)
      | Node::OptChainExpr(_)
      | Node::Param(_)
      | Node::ParenExpr(_)
      | Node::PrivateMethod(_)
      | Node::PrivateName(_)
      | Node::PrivateProp(_)
      | Node::Regex(_)
      | Node::RestPat(_)
      | Node::ReturnStmt(_)
      | Node::Script(_)
      | Node::SeqExpr(_)
      | Node::SetterProp(_)
      | Node::SpreadElement(_)
      | Node::StaticBlock(_)
      | Node::Super(_)
      | Node::SwitchCase(_)
      | Node::SwitchStmt(_)
      | Node::TaggedTpl(_)
      | Node::ThisExpr(_)
      | Node::ThrowStmt(_)
      | Node::Tpl(_)
      | Node::TplElement(_)
      | Node::TryStmt(_)
      | Node::TsEnumDecl(_)
      | Node::TsEnumMember(_)
      | Node::TsExportAssignment(_)
      | Node::TsExternalModuleRef(_)
      | Node::TsImportEqualsDecl(_)
      | Node::TsModuleBlock(_)
      | Node::TsModuleDecl(_)
      | Node::TsNamespaceDecl(_)
      | Node::TsNamespaceExportDecl(_) => Some(false),

      Node::TsArrayType(_)
      | Node::TsCallSignatureDecl(_)
      | Node::TsConditionalType(_)
      | Node::TsConstAssertion(_)
      | Node::TsConstructSignatureDecl(_)
      | Node::TsConstructorType(_)
      | Node::TsExprWithTypeArgs(_)
      | Node::TsFnType(_)
      | Node::TsGetterSignature(_)
      | Node::TsImportType(_)
      | Node::TsIndexSignature(_)
      | Node::TsIndexedAccessType(_)
      | Node::TsInferType(_)
      | Node::TsInterfaceBody(_)
      | Node::TsInterfaceDecl(_)
      | Node::TsIntersectionType(_)
      | Node::TsKeywordType(_)
      | Node::TsLitType(_)
      | Node::TsMappedType(_)
      | Node::TsMethodSignature(_)
      | Node::TsNonNullExpr(_)
      | Node::TsOptionalType(_)
      | Node::TsParamProp(_)
      | Node::TsParenthesizedType(_)
      | Node::TsPropertySignature(_)
      | Node::TsQualifiedName(_)
      | Node::TsRestType(_)
      | Node::TsSetterSignature(_)
      | Node::TsThisType(_)
      | Node::TsTplLitType(_)
      | Node::TsTupleElement(_)
      | Node::TsTupleType(_)
      | Node::TsTypeAliasDecl(_)
      | Node::TsTypeAnn(_)
      | Node::TsTypeLit(_)
      | Node::TsTypeOperator(_)
      | Node::TsTypeParam(_)
      | Node::TsTypeParamDecl(_)
      | Node::TsTypeParamInstantiation(_)
      | Node::TsTypePredicate(_)
      | Node::TsTypeQuery(_)
      | Node::TsTypeRef(_)
      | Node::TsUnionType(_) => Some(true),

      // may be a type
      Node::TsTypeAssertion(expr) => {
        Some(expr.type_ann.span().contains(node.span()))
      }
      Node::TsAsExpr(expr) => Some(expr.type_ann.span().contains(node.span())),

      // still need more info
      Node::BigInt(_)
      | Node::Bool(_)
      | Node::Null(_)
      | Node::Number(_)
      | Node::MemberExpr(_)
      | Node::Str(_)
      | Node::Ident(_) => None,
    };
    if let Some(is_type) = is_type {
      return is_type;
    }
    node = parent;
  }

  false
}
