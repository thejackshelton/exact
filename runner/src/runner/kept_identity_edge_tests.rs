//! Kept answers at the edges of admission: a source whose kind changes
//! across a reload, and a refusal that names an identity admitted after
//! readiness.
use super::*;

/// One store reader of `feeds`, identified by the `identity` state; with
/// `declared`, `feeds` is a Contract-declared HTTP source (LLP 1162.001).
fn feeds_plan(declared: bool) -> Plan {
    let mut b = PlanBuilder::new(exact_kernel::SCHEMA_DIGEST, 1);
    let string = b.primitive(TypeKind::String);
    let number = b.primitive(TypeKind::Number);
    let empty = b.constant(&Value::str(""));
    let identity = b.slot("identity", string, empty);
    if declared {
        let source = b.source("feeds", &[number, string], string);
        b.fetch(
            source,
            "GET",
            "https://example.test/feeds",
            "https://example.test",
            &["page", "account"],
            &[],
            "",
            "",
            "",
        );
    }
    let mut zero = Asm::new();
    zero.number(0.);
    let zero = b.code(zero);
    let mut key = Asm::new();
    key.load_slot(identity);
    let key = b.code(key);
    let feeds = b.resource(
        "feeds",
        "feeds",
        &[zero, key],
        string,
        Some(&Value::str("loading")),
    );
    b.set_resource_reader(feeds, true);
    b.set_resource_initial_args(feeds, &[Value::Number(0.), Value::str("")]);
    let mut set = Asm::new();
    set.load_param(0).store_slot(identity);
    let set = b.code(set);
    b.action("identify", &[("value", string)], &[identity], set);
    b.node(NodeType::View as u8, None, None, 0, &[], &[], None);
    b.finish().unwrap()
}

#[test]
fn a_waiting_candidate_is_dropped_when_its_source_becomes_declared() {
    let r = launch_plan(
        feeds_plan(false),
        vec![feeds_kept(
            &[Value::Number(0.), Value::str("alice")],
            "private",
        )],
    );
    assert!(r.resources[0].kept_candidate.is_some());
    let mut next = Runner::boot_carrying(
        feeds_plan(true),
        Source::default(),
        Kernel::with_monospace(),
        &r.carry(),
        Default::default(),
        "/",
    )
    .unwrap();
    assert!(next.resources[0].kept_candidate.is_none());
    next.act("identify", vec![Value::str("alice")]).unwrap();
    assert_ne!(next.resource("feeds"), Some(&Value::str("private")));
}

/// `account` is identified by the `identity` state and `feeds` by
/// `account`; both are store readers of the data module.
fn chained() -> Plan {
    let mut b = PlanBuilder::new(exact_kernel::SCHEMA_DIGEST, 1);
    let string = b.primitive(TypeKind::String);
    let empty = b.constant(&Value::str(""));
    let identity = b.slot("identity", string, empty);
    let mut zero = Asm::new();
    zero.number(0.);
    let zero = b.code(zero);
    let mut by_identity = Asm::new();
    by_identity.load_slot(identity);
    let by_identity = b.code(by_identity);
    let account = b.resource(
        "account",
        "account",
        &[zero, by_identity],
        string,
        Some(&Value::str("nobody")),
    );
    b.set_resource_reader(account, true);
    b.set_resource_initial_args(account, &[Value::Number(0.), Value::str("")]);
    let mut by_account = Asm::new();
    by_account.load_resource(account);
    let by_account = b.code(by_account);
    let feeds = b.resource(
        "feeds",
        "feeds",
        &[zero, by_account],
        string,
        Some(&Value::str("loading")),
    );
    b.set_resource_reader(feeds, true);
    b.set_resource_initial_args(feeds, &[Value::Number(0.), Value::str("nobody")]);
    let mut set = Asm::new();
    set.load_param(0).store_slot(identity);
    let set = b.code(set);
    b.action("identify", &[("value", string)], &[identity], set);
    b.node(NodeType::View as u8, None, None, 0, &[], &[], None);
    b.finish().unwrap()
}

#[test]
fn a_refusal_names_an_identity_admitted_after_readiness_as_kept() {
    let mut r = launch_plan(
        chained(),
        vec![
            (
                kept::kept_name("account"),
                kept::encode(
                    &[Value::Number(0.), Value::str("carol")],
                    &Value::str("bob"),
                ),
            ),
            feeds_kept(&[Value::Number(0.), Value::str("alice")], "private"),
        ],
    );
    r.data.ready = true;
    r.data.delayed_identity = true;
    r.data_ready().unwrap();
    r.act("identify", vec![Value::str("carol")]).unwrap();
    shows(&r, "account", "bob");
    let line = r
        .journal()
        .find(|l| l.contains("feeds did not use"))
        .expect("feeds is refused");
    assert!(
        line.contains("resource `account` is \"bob\" (admitted kept answer)"),
        "{line}"
    );
}
