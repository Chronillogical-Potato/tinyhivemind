//! Memory identities, bindings and reaches against the constitution.
//!
//! Findings name `memory.json`, except the protected namespaces and
//! read-only pins, which the constitution declares.
use super::{identities, reference, validate_namespace};
use crate::{
    Error, Package, Result,
    diagnostic::{CONSTITUTION, Findings, MEMORY},
};
use std::collections::BTreeSet;

fn protected(package: &Package, namespace: &str, inherit: bool, descendants: bool) -> Result<()> {
    validate_namespace(namespace)?;
    if package.constitution.protected_namespaces.iter().any(|p| {
        namespace == p
            || namespace
                .strip_prefix(p)
                .is_some_and(|tail| tail.starts_with('/'))
            || (descendants
                && p.strip_prefix(namespace)
                    .is_some_and(|tail| tail.starts_with('/')))
            || (inherit && namespace.starts_with(&format!("{p}/")))
    }) {
        return Err(Error::MemoryAccess {
            namespace: namespace.into(),
        });
    }
    Ok(())
}

fn access(namespace: &str) -> Error {
    Error::MemoryAccess {
        namespace: namespace.into(),
    }
}

/// Whether `namespace` is `root` or sits below it.
fn within(namespace: &str, root: &str) -> bool {
    namespace == root || namespace.starts_with(&format!("{root}/"))
}

pub(super) fn memory(package: &Package, seats: &BTreeSet<&str>, found: &mut Findings) {
    let spec = &package.memory;
    found.check(validate_namespace(&spec.root), MEMORY, "/root".to_owned());
    let ids = identities(
        found,
        MEMORY,
        "memory",
        spec.identities.iter().map(|v| &v.id),
        |i| format!("/identities/{i}/id"),
    );
    identities(
        found,
        MEMORY,
        "memory namespace",
        spec.identities.iter().map(|v| &v.namespace),
        |i| format!("/identities/{i}/namespace"),
    );
    identities(
        found,
        MEMORY,
        "memory binding",
        spec.bindings.iter().map(|v| &v.seat),
        |i| format!("/bindings/{i}/seat"),
    );
    for (i, namespace) in package.constitution.protected_namespaces.iter().enumerate() {
        found.check(
            validate_namespace(namespace),
            CONSTITUTION,
            format!("/protected_namespaces/{i}"),
        );
    }
    for (i, id) in package.constitution.read_only_memories.iter().enumerate() {
        found.check(
            reference("memory", id, &ids),
            CONSTITUTION,
            format!("/read_only_memories/{i}"),
        );
    }
    declared(package, &ids, found);
    bound_seats(package, seats, &ids, found);
}

/// Each identity's namespace, kind, fork parent and read-only pin.
fn declared(package: &Package, ids: &BTreeSet<&str>, found: &mut Findings) {
    let spec = &package.memory;
    for (i, identity) in spec.identities.iter().enumerate() {
        let at = format!("/identities/{i}/namespace");
        let namespace = &identity.namespace;
        if !found.check(
            protected(package, namespace, false, false),
            MEMORY,
            at.clone(),
        ) {
            continue;
        }
        if identity.kind == crate::MemoryKind::Agent
            && (!namespace
                .rsplit('/')
                .next()
                .is_some_and(|part| part.starts_with("agent:"))
                || *namespace == spec.root)
        {
            found.push(access(namespace), MEMORY, at.clone());
        }
        if !within(namespace, &spec.root) {
            found.push(access(namespace), MEMORY, at.clone());
        }
        if let Some(parent) = &identity.fork_parent {
            let at = format!("/identities/{i}/fork_parent");
            if found.check(reference("memory", parent, ids), MEMORY, at.clone())
                && let Some(declared) = spec.identities.iter().find(|v| &v.id == parent)
                && !namespace.starts_with(&format!("{}/", declared.namespace))
            {
                found.push(access(namespace), MEMORY, at);
            }
        }
        if package
            .constitution
            .read_only_memories
            .contains(&identity.id)
            && !identity.read_only
        {
            found.push(
                access(namespace),
                MEMORY,
                format!("/identities/{i}/read_only"),
            );
        }
    }
}

/// Each binding's seat, identity, budget and read reaches.
fn bound_seats(
    package: &Package,
    seats: &BTreeSet<&str>,
    ids: &BTreeSet<&str>,
    found: &mut Findings,
) {
    let spec = &package.memory;
    for (i, binding) in spec.bindings.iter().enumerate() {
        found.check(
            reference("seat", &binding.seat, seats),
            MEMORY,
            format!("/bindings/{i}/seat"),
        );
        found.check(
            reference("memory", &binding.identity, ids),
            MEMORY,
            format!("/bindings/{i}/identity"),
        );
        if binding.budget_chars == 0 {
            found.push(
                Error::Bound {
                    field: "memory.budget_chars".into(),
                },
                MEMORY,
                format!("/bindings/{i}/budget_chars"),
            );
        }
        for (k, reach) in binding.reads.iter().enumerate() {
            let at = format!("/bindings/{i}/reads/{k}");
            if reach.inherit {
                found.push(access(&reach.at), MEMORY, format!("{at}/inherit"));
                continue;
            }
            let at = format!("{at}/at");
            if found.check(
                protected(package, &reach.at, reach.inherit, reach.descendants),
                MEMORY,
                at.clone(),
            ) && !within(&reach.at, &spec.root)
            {
                found.push(access(&reach.at), MEMORY, at);
            }
        }
    }
}
