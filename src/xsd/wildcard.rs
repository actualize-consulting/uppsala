//! Wildcard namespace constraint operations.
//!
//! Provides functions for checking namespace membership against wildcard
//! constraints, computing the intersection and union of namespace constraints,
//! and determining the stricter of two processContents values.
//!
//! These are used by `AttributeWildcard` methods
//! and by element wildcard validation in the validation module.

use super::types::{NamespaceConstraint, ProcessContents};

/// Check if a namespace URI matches a wildcard namespace constraint.
///
/// Works for both attribute and element wildcards. Returns `true` if the
/// given namespace (or absence thereof) is allowed by the constraint.
pub(crate) fn wildcard_allows_namespace(
    constraint: &NamespaceConstraint,
    ns: Option<&str>,
) -> bool {
    match constraint {
        NamespaceConstraint::Any => true,
        NamespaceConstraint::Other(target_ns) => {
            match ns {
                None => false, // ##other excludes no-namespace
                Some(uri) => match target_ns {
                    Some(tns) => uri != tns,
                    None => true,
                },
            }
        }
        NamespaceConstraint::Local => ns.is_none(),
        NamespaceConstraint::TargetNamespace(target_ns) => ns == target_ns.as_deref(),
        NamespaceConstraint::List(uris) => match ns {
            None => uris.iter().any(|u| u == "##local"),
            Some(uri) => uris.iter().any(|u| u == uri),
        },
        NamespaceConstraint::NotLocal => ns.is_some(),
        NamespaceConstraint::Not(excluded) => {
            matches!(ns, Some(uri) if !excluded.iter().any(|u| u == uri))
        }
    }
}

/// Return the stricter of two processContents values.
///
/// Ordering: Strict > Lax > Skip. Used when intersecting wildcards to
/// ensure the most restrictive validation mode is preserved.
pub(super) fn stricter_process_contents(
    a: &ProcessContents,
    b: &ProcessContents,
) -> ProcessContents {
    match (a, b) {
        (ProcessContents::Strict, _) | (_, ProcessContents::Strict) => ProcessContents::Strict,
        (ProcessContents::Lax, _) | (_, ProcessContents::Lax) => ProcessContents::Lax,
        _ => ProcessContents::Skip,
    }
}

/// Compute the intersection of two namespace constraints.
///
/// Returns `None` if the intersection is empty (no namespace allowed by both).
/// Used when merging attribute groups that both define wildcards — the result
/// only allows namespaces permitted by both wildcards.
pub(super) fn intersect_namespace_constraints(
    a: &NamespaceConstraint,
    b: &NamespaceConstraint,
) -> Option<NamespaceConstraint> {
    if let Some(finite) = finite_namespaces(a) {
        return intersection_from_finite(finite, b);
    }
    if let Some(finite) = finite_namespaces(b) {
        return intersection_from_finite(finite, a);
    }

    match (a, b) {
        // Any intersected with anything is that thing
        (NamespaceConstraint::Any, other) | (other, NamespaceConstraint::Any) => {
            Some(other.clone())
        }
        (NamespaceConstraint::Other(a), NamespaceConstraint::Other(b)) => {
            Some(excluding_namespaces([a.as_ref(), b.as_ref()]))
        }
        (NamespaceConstraint::Other(excluded), NamespaceConstraint::Not(other_excluded))
        | (NamespaceConstraint::Not(other_excluded), NamespaceConstraint::Other(excluded)) => {
            let mut excluded_uris = other_excluded.clone();
            if let Some(uri) = excluded {
                push_unique(&mut excluded_uris, uri.clone());
            }
            Some(not_constraint(excluded_uris))
        }
        (NamespaceConstraint::Other(excluded), NamespaceConstraint::NotLocal)
        | (NamespaceConstraint::NotLocal, NamespaceConstraint::Other(excluded)) => {
            Some(NamespaceConstraint::Other(excluded.clone()))
        }
        (NamespaceConstraint::Not(a), NamespaceConstraint::Not(b)) => {
            let mut excluded = a.clone();
            for uri in b {
                push_unique(&mut excluded, uri.clone());
            }
            Some(not_constraint(excluded))
        }
        (NamespaceConstraint::Not(excluded), NamespaceConstraint::NotLocal)
        | (NamespaceConstraint::NotLocal, NamespaceConstraint::Not(excluded)) => {
            Some(not_constraint(excluded.clone()))
        }
        (NamespaceConstraint::NotLocal, NamespaceConstraint::NotLocal) => {
            Some(NamespaceConstraint::NotLocal)
        }
        _ => None,
    }
}

/// Compute the union of two namespace constraints.
///
/// Used when computing the effective wildcard for complex type extensions —
/// the derived type's wildcard is unioned with the base type's wildcard.
/// Falls back to `Any` for combinations that don't have a more specific result.
pub(super) fn union_namespace_constraints(
    a: &NamespaceConstraint,
    b: &NamespaceConstraint,
) -> NamespaceConstraint {
    if let (Some(mut finite_a), Some(finite_b)) = (finite_namespaces(a), finite_namespaces(b)) {
        for ns in finite_b {
            if !finite_a.contains(&ns) {
                finite_a.push(ns);
            }
        }
        return constraint_from_finite(finite_a).unwrap_or(NamespaceConstraint::Any);
    }

    match (a, b) {
        // Any union anything = Any
        (NamespaceConstraint::Any, _) | (_, NamespaceConstraint::Any) => NamespaceConstraint::Any,
        (NamespaceConstraint::Other(a), NamespaceConstraint::Other(b)) => match (a, b) {
            (Some(left), Some(right)) if left == right => NamespaceConstraint::Other(a.clone()),
            (None, None) => NamespaceConstraint::Other(None),
            _ => NamespaceConstraint::NotLocal,
        },
        (NamespaceConstraint::NotLocal, NamespaceConstraint::Local)
        | (NamespaceConstraint::Local, NamespaceConstraint::NotLocal) => NamespaceConstraint::Any,
        // `TargetNamespace(None)` is the no-namespace case (≡ `Local`). Its
        // union with `NotLocal` (every non-empty namespace) covers all names,
        // so the result is `Any` — not `NotLocal`, which would wrongly exclude
        // no-namespace names.
        (NamespaceConstraint::NotLocal, NamespaceConstraint::TargetNamespace(None))
        | (NamespaceConstraint::TargetNamespace(None), NamespaceConstraint::NotLocal) => {
            NamespaceConstraint::Any
        }
        (NamespaceConstraint::NotLocal, _) | (_, NamespaceConstraint::NotLocal) => {
            NamespaceConstraint::NotLocal
        }
        (NamespaceConstraint::Other(excluded), NamespaceConstraint::TargetNamespace(Some(ns)))
        | (NamespaceConstraint::TargetNamespace(Some(ns)), NamespaceConstraint::Other(excluded)) => {
            if excluded.as_deref() == Some(ns.as_str()) {
                NamespaceConstraint::NotLocal
            } else {
                NamespaceConstraint::Other(excluded.clone())
            }
        }
        (NamespaceConstraint::Other(_), NamespaceConstraint::Local)
        | (NamespaceConstraint::Local, NamespaceConstraint::Other(_)) => NamespaceConstraint::Any,
        (NamespaceConstraint::Not(excluded), NamespaceConstraint::TargetNamespace(Some(ns)))
        | (NamespaceConstraint::TargetNamespace(Some(ns)), NamespaceConstraint::Not(excluded)) => {
            let reduced: Vec<String> = excluded.iter().filter(|u| *u != ns).cloned().collect();
            not_constraint(reduced)
        }
        (NamespaceConstraint::Not(_), NamespaceConstraint::Local)
        | (NamespaceConstraint::Local, NamespaceConstraint::Not(_)) => NamespaceConstraint::Any,
        (NamespaceConstraint::Not(a), NamespaceConstraint::Not(b)) => {
            let common: Vec<String> = a.iter().filter(|u| b.contains(u)).cloned().collect();
            not_constraint(common)
        }
        _ => NamespaceConstraint::Any,
    }
}

fn finite_namespaces(constraint: &NamespaceConstraint) -> Option<Vec<Option<String>>> {
    match constraint {
        NamespaceConstraint::Local => Some(vec![None]),
        NamespaceConstraint::TargetNamespace(ns) => Some(vec![ns.clone()]),
        NamespaceConstraint::List(uris) => {
            let mut result = Vec::new();
            for uri in uris {
                let ns = if uri == "##local" {
                    None
                } else {
                    Some(uri.clone())
                };
                if !result.contains(&ns) {
                    result.push(ns);
                }
            }
            Some(result)
        }
        _ => None,
    }
}

fn intersection_from_finite(
    finite: Vec<Option<String>>,
    other: &NamespaceConstraint,
) -> Option<NamespaceConstraint> {
    let kept: Vec<Option<String>> = finite
        .into_iter()
        .filter(|ns| wildcard_allows_namespace(other, ns.as_deref()))
        .collect();
    constraint_from_finite(kept)
}

fn constraint_from_finite(values: Vec<Option<String>>) -> Option<NamespaceConstraint> {
    let mut values = values;
    values.dedup();
    match values.as_slice() {
        [] => None,
        [None] => Some(NamespaceConstraint::Local),
        [Some(uri)] => Some(NamespaceConstraint::TargetNamespace(Some(uri.clone()))),
        _ => {
            let mut uris = Vec::new();
            for value in values {
                match value {
                    None => push_unique(&mut uris, "##local".to_string()),
                    Some(uri) => push_unique(&mut uris, uri),
                }
            }
            Some(NamespaceConstraint::List(uris))
        }
    }
}

fn excluding_namespaces<'a>(
    exclusions: impl IntoIterator<Item = Option<&'a String>>,
) -> NamespaceConstraint {
    let mut excluded = Vec::new();
    for uri in exclusions.into_iter().flatten() {
        push_unique(&mut excluded, uri.clone());
    }
    not_constraint(excluded)
}

fn not_constraint(excluded: Vec<String>) -> NamespaceConstraint {
    if excluded.is_empty() {
        NamespaceConstraint::NotLocal
    } else {
        NamespaceConstraint::Not(excluded)
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_notlocal_with_no_target_namespace_is_any() {
        // `TargetNamespace(None)` is the no-namespace case (≡ Local). Its
        // union with NotLocal must cover every name → Any, not NotLocal
        // (which would wrongly exclude no-namespace names).
        let a = NamespaceConstraint::NotLocal;
        let b = NamespaceConstraint::TargetNamespace(None);
        assert!(matches!(
            union_namespace_constraints(&a, &b),
            NamespaceConstraint::Any
        ));
        assert!(matches!(
            union_namespace_constraints(&b, &a),
            NamespaceConstraint::Any
        ));
        // The resulting constraint must admit both no-namespace and any URI.
        let result = union_namespace_constraints(&a, &b);
        assert!(wildcard_allows_namespace(&result, None));
        assert!(wildcard_allows_namespace(&result, Some("urn:x")));
    }

    #[test]
    fn union_notlocal_with_specific_target_namespace_stays_notlocal() {
        // A specific (non-empty) target namespace is itself non-local, so the
        // union with NotLocal contributes nothing new and remains NotLocal.
        let a = NamespaceConstraint::NotLocal;
        let b = NamespaceConstraint::TargetNamespace(Some("urn:x".into()));
        assert!(matches!(
            union_namespace_constraints(&a, &b),
            NamespaceConstraint::NotLocal
        ));
    }
}
