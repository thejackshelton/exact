//! A kept answer's identity read from another resource (LLP 1027.005 D3,
//! 2026-10-10): `feeds(gen, account)`, where `account` reads the store.
use super::*;
use crate::{Answer, Outcome, Request, Store};
use exact_kernel::{Kernel, NodeType};
use exact_plan::{asm::Asm, builder::PlanBuilder, Plan, TypeKind, Value};

#[derive(Default)]
struct Source {
    ready: bool,
    delayed_identity: bool,
    asks: Vec<(String, Vec<Value>)>,
}

impl DataSource for Source {
    fn query(&mut self, _: &str, _: &[Value]) -> Result<Value, DataError> {
        unreachable!("the fixture implements answer")
    }

    fn answer(
        &mut self,
        store: &mut Store,
        source: &str,
        args: &[Value],
    ) -> Result<Answer, DataError> {
        assert!(
            self.ready,
            "a deferred source must not run before readiness"
        );
        self.asks.push((source.to_string(), args.to_vec()));
        if source == "account" && self.delayed_identity {
            return Ok(Answer::Later(Request::get("https://example.test/identity")));
        }
        let key = store.get("token").unwrap_or("public").to_string();
        Ok(match source {
            "account" => Answer::Now(Value::str(&key)),
            _ => Answer::Later(Request::get(&format!(
                "https://example.test/{}",
                identity_text(&args[1])
            ))),
        })
    }

    fn parse(
        &mut self,
        store: &mut Store,
        source: &str,
        args: &[Value],
        _: Outcome,
    ) -> Result<Answer, DataError> {
        if source == "account" && self.delayed_identity {
            return Ok(Answer::Now(Value::str(
                store.get("token").unwrap_or("public"),
            )));
        }
        Ok(Answer::Now(Value::str(&format!(
            "feeds of {}",
            identity_text(&args[1])
        ))))
    }

    fn ready(&self) -> bool {
        self.ready
    }

    fn grants(&self) -> &str {
        "secret.keep token\nnet.fetch https://example.test"
    }
}

fn identity_text(value: &Value) -> &str {
    match value {
        Value::Option(Some(v)) => v.text(),
        v => v.text(),
    }
}

/// `account(0)` (placeholder "nobody") and `feeds(gen, key)`, the key read
/// straight from `account` or, with `through_derive`, through a derive.
fn plan(through_derive: bool) -> Plan {
    plan_with(through_derive, false, true)
}

/// As [`plan`]; `key_is_context` makes the key `feeds`' `with` context, and
/// `account_reads_store` false makes `account` a plain compiled resource.
fn plan_with(through_derive: bool, key_is_context: bool, account_reads_store: bool) -> Plan {
    plan_with_initial(
        through_derive,
        key_is_context,
        account_reads_store,
        "nobody",
    )
}

fn plan_with_initial(
    through_derive: bool,
    key_is_context: bool,
    account_reads_store: bool,
    initial: &str,
) -> Plan {
    let mut b = PlanBuilder::new(exact_kernel::SCHEMA_DIGEST, 1);
    let string = b.primitive(TypeKind::String);
    let number = b.primitive(TypeKind::Number);
    let zero = b.constant(&Value::Number(0.));
    let gen = b.slot("gen", number, zero);
    let mut a = Asm::new();
    a.number(0.);
    let a = b.code(a);
    let account = b.resource(
        "account",
        "account",
        &[a],
        string,
        Some(&Value::str(initial)),
    );
    b.set_resource_reader(account, account_reads_store);
    b.set_resource_initial_args(account, &[Value::Number(0.)]);
    let mut g = Asm::new();
    g.load_slot(gen);
    let g = b.code(g);
    let mut k = Asm::new();
    if through_derive {
        let mut d = Asm::new();
        d.load_resource(account);
        let d = b.code(d);
        let key = b.derive("key", string, d);
        k.load_derive(key);
    } else {
        k.load_resource(account);
    }
    let k = b.code(k);
    let feeds = b.resource(
        "feeds",
        "feeds",
        &[g, k],
        string,
        Some(&Value::str("loading")),
    );
    b.set_resource_reader(feeds, true);
    b.set_resource_context(feeds, u16::from(key_is_context));
    b.set_resource_initial_args(feeds, &[Value::Number(0.), Value::str(initial)]);
    let mut body = Asm::new();
    body.load_param(0).store_slot(gen);
    let body = b.code(body);
    b.action("gen", &[("value", number)], &[gen], body);
    b.node(NodeType::View as u8, None, None, 0, &[], &[], None);
    b.finish().unwrap()
}

fn launch(through_derive: bool, snapshot: Vec<(String, Vec<u8>)>) -> Runner<Source> {
    launch_plan(plan(through_derive), snapshot)
}

fn launch_plan(plan: Plan, snapshot: Vec<(String, Vec<u8>)>) -> Runner<Source> {
    Runner::boot_stored(
        plan,
        Source::default(),
        Kernel::with_monospace(),
        snapshot,
        Default::default(),
        "/",
    )
    .unwrap()
}

fn settle_requests(r: &mut Runner<Source>) {
    for q in r.take_requests() {
        let outcome = if q.target == "account" && !r.plan.modules.is_empty() {
            Outcome::Response(crate::Response {
                status: 200,
                headers: vec![],
                body: br#""public""#.to_vec(),
            })
        } else {
            Outcome::Storage(vec![])
        };
        r.fulfill(q.ticket, outcome).unwrap();
    }
}

fn shows(r: &Runner<Source>, name: &str, value: &str) {
    assert_eq!(r.resource(name), Some(&Value::str(value)), "{name}");
}

fn feeds_kept(args: &[Value], value: &str) -> (String, Vec<u8>) {
    (
        kept::kept_name("feeds"),
        kept::encode(args, &Value::str(value)),
    )
}

#[test]
fn a_sign_in_rekeeps_both_and_the_next_launch_shows_the_signed_in_answers() {
    let mut r = launch(false, vec![]);
    r.data.ready = true;
    r.data_ready().unwrap();
    settle_requests(&mut r);
    shows(&r, "feeds", "feeds of public");
    // Signing in writes the secret; the store-reading resources answer again.
    r.store.set("token", "alice").unwrap();
    r.act("gen", vec![Value::Number(0.)]).unwrap();
    settle_requests(&mut r);
    shows(&r, "account", "alice");
    shows(&r, "feeds", "feeds of alice");
    let next = launch(false, r.store.snapshot());
    shows(&next, "account", "alice");
    shows(&next, "feeds", "feeds of alice");
    assert!(next.journal().any(
        |l| l.contains("feeds shows its kept answer from the last session, kept for [0,\"alice\"]")
    ));
}

#[test]
fn an_identity_read_from_a_resource_on_its_placeholder_does_not_admit_a_kept_answer() {
    for through_derive in [false, true] {
        // The account kept nothing (a first launch after an update, an
        // answer over the budget), so the first frame's key is the
        // placeholder's, which another launch's entry happens to match.
        let mut r = launch(
            through_derive,
            vec![feeds_kept(
                &[Value::Number(0.), Value::str("nobody")],
                "feeds of somebody else",
            )],
        );
        shows(&r, "account", "nobody");
        shows(&r, "feeds", "loading");
        assert!(!r.journal().any(|l| l.contains("shows its kept answer")));
        // At readiness both are asked for what they are now.
        r.data.ready = true;
        r.data_ready().unwrap();
        settle_requests(&mut r);
        shows(&r, "feeds", "feeds of public");
    }
}

#[test]
fn a_refused_kept_answer_equal_to_the_compiled_one_is_still_a_fallback() {
    for through_derive in [false, true] {
        // The account kept "nobody" for other arguments: refused, it shows
        // the compiled "nobody", which the dependent's identity must not
        // take for this launch's answer.
        let r = launch(
            through_derive,
            vec![
                (
                    kept::kept_name("account"),
                    kept::encode(&[Value::Number(1.)], &Value::str("nobody")),
                ),
                feeds_kept(
                    &[Value::Number(0.), Value::str("nobody")],
                    "feeds of somebody else",
                ),
            ],
        );
        shows(&r, "account", "nobody");
        shows(&r, "feeds", "loading");
        assert!(!r.journal().any(|l| l.contains("shows its kept answer")));
    }
}

#[test]
fn an_identity_read_from_a_kept_answer_still_admits_its_dependent() {
    for through_derive in [false, true] {
        let r = launch(
            through_derive,
            vec![
                (
                    kept::kept_name("account"),
                    kept::encode(&[Value::Number(0.)], &Value::str("alice")),
                ),
                feeds_kept(&[Value::Number(0.), Value::str("alice")], "feeds of alice"),
            ],
        );
        shows(&r, "account", "alice");
        shows(&r, "feeds", "feeds of alice");
        assert!(r
            .journal()
            .any(|l| l.contains("account shows its kept answer")));
    }
}

#[test]
fn a_placeholder_read_only_by_with_context_does_not_refuse_the_identity() {
    // `feeds(gen) with key`: the identity is `gen`, which is known.
    let r = launch_plan(
        plan_with(false, true, true),
        vec![feeds_kept(&[Value::Number(0.)], "feeds kept for gen 0")],
    );
    shows(&r, "account", "nobody");
    shows(&r, "feeds", "feeds kept for gen 0");
}

#[test]
fn a_compiled_value_of_a_resource_that_reads_no_store_is_a_known_identity() {
    let r = launch_plan(
        plan_with(false, false, false),
        vec![feeds_kept(
            &[Value::Number(0.), Value::str("nobody")],
            "feeds of nobody",
        )],
    );
    shows(&r, "feeds", "feeds of nobody");
}

#[test]
fn a_long_identity_is_journaled_shortened_with_a_mark() {
    let line = super::lines::kept_shown("feeds", &"x".repeat(300));
    assert!(
        line.ends_with(&format!("kept for {}…", "x".repeat(200))),
        "{line}"
    );
    let line = super::lines::kept_shown("feeds", "[0]");
    assert!(line.ends_with("kept for [0]"), "{line}");
}

#[test]
fn a_refused_kept_answer_is_journaled_with_why() {
    // The account kept for other arguments; feeds kept for a key read from
    // the account's fallback.
    let mut r = launch(
        false,
        vec![
            (
                kept::kept_name("account"),
                kept::encode(&[Value::Number(1.)], &Value::str("alice")),
            ),
            feeds_kept(
                &[Value::Number(0.), Value::str("nobody")],
                "feeds of nobody",
            ),
        ],
    );
    let lines: Vec<_> = r
        .journal()
        .filter(|l| l.contains("did not use its kept answer"))
        .collect();
    assert!(
        lines.iter().any(|l| l.contains(
            "account did not use its kept answer: kept for [1], this launch asks for [0]"
        )),
        "{lines:?}"
    );
    assert!(r
        .journal()
        .any(|l| l.contains("feeds's kept answer is waiting")));
    assert_eq!(lines.len(), 1, "once consumed: {lines:?}");
    // Readiness names the feeds entry once the account answer makes its
    // identity a known mismatch.
    r.data.ready = true;
    r.data_ready().unwrap();
    settle_requests(&mut r);
    let after = r
        .journal()
        .filter(|l| l.contains("did not use its kept answer"))
        .count();
    assert_eq!(after, 2);
}

#[test]
fn a_kept_entry_that_no_longer_decodes_is_journaled() {
    let r = launch(false, vec![(kept::kept_name("account"), vec![1, 2, 3])]);
    assert!(r
        .journal()
        .any(|l| l.ends_with("account did not use its kept answer: it no longer decodes or fits")));
}

/// A root identity state, optionally persisted or a mutation result, read
/// through a derive so the diagnostics must reach the leaf input.
fn state_identity(persist: bool, mutation: bool) -> Plan {
    state_identity_gated(persist, mutation, false)
}

fn state_identity_gated(persist: bool, mutation: bool, gated: bool) -> Plan {
    let mut b = PlanBuilder::new(exact_kernel::SCHEMA_DIGEST, 1);
    let string = b.primitive(TypeKind::String);
    let number = b.primitive(TypeKind::Number);
    let zero_value = b.constant(&Value::Number(0.));
    let revision = b.slot("revision", number, zero_value);
    let identity_ty = if mutation { b.option(string) } else { string };
    let init = b.constant(&if mutation {
        Value::NONE
    } else {
        Value::str("")
    });
    let identity = b.slot("identity", identity_ty, init);
    b.set_slot_persist(identity, persist);
    if mutation {
        b.mutation("signedIn", identity, string);
    }
    let mut d = Asm::new();
    d.load_slot(identity);
    let d = b.code(d);
    let key = b.derive("key", identity_ty, d);
    let mut zero = Asm::new();
    zero.number(0.);
    let zero = b.code(zero);
    let mut k = Asm::new();
    k.load_derive(key);
    let k = b.code(k);
    let mut context = Asm::new();
    context.load_slot(revision);
    let context = b.code(context);
    let feeds = b.resource(
        "feeds",
        "feeds",
        &[zero, k, context],
        string,
        Some(&Value::str("loading")),
    );
    b.set_resource_reader(feeds, true);
    if gated {
        b.set_resource_placeholder_value(feeds, &Value::str("loading"));
        closed_gate(&mut b, feeds);
    }
    b.set_resource_context(feeds, 1);
    b.set_resource_initial_args(
        feeds,
        &[
            Value::Number(0.),
            if mutation {
                Value::NONE
            } else {
                Value::str("")
            },
            Value::Number(0.),
        ],
    );
    let mut set = Asm::new();
    set.load_param(0).store_slot(identity);
    let set = b.code(set);
    b.action("identify", &[("value", identity_ty)], &[identity], set);
    let mut set = Asm::new();
    set.load_param(0).store_slot(revision);
    let set = b.code(set);
    b.action("revision", &[("value", number)], &[revision], set);
    let loading = b.str("loading view");
    let mut text = Asm::new();
    let fallback = text.label();
    let end = text.label();
    text.current_resource(feeds)
        .jump_if_false(fallback)
        .load_resource(feeds)
        .jump(end)
        .place(fallback)
        .str(loading)
        .place(end);
    let text = b.code(text);
    b.node(
        NodeType::Text as u8,
        None,
        None,
        0,
        &[exact_plan::BindingsRow {
            kind: exact_plan::BindingKind::Prop,
            id: exact_kernel::PropId::Text as u16,
            expr: text,
            written: true,
        }],
        &[],
        None,
    );
    b.finish().unwrap()
}

fn closed_gate(b: &mut PlanBuilder, resource: exact_plan::ResourcesId) {
    let boolean = b.primitive(TypeKind::Bool);
    let closed = b.constant(&Value::Bool(false));
    let enabled = b.slot("enabled", boolean, closed);
    let mut gate = Asm::new();
    gate.load_slot(enabled);
    let gate = b.code(gate);
    b.set_resource_gate(resource, gate);
    let mut set = Asm::new();
    set.load_param(0).store_slot(enabled);
    let set = b.code(set);
    b.action("enable", &[("value", boolean)], &[enabled], set);
}

#[test]
fn an_empty_boot_identity_admits_its_kept_answer_before_or_after_readiness() {
    for (ready_first, mutation) in [(false, false), (true, false), (false, true), (true, true)] {
        let alice = if mutation {
            Value::some(Value::str("alice"))
        } else {
            Value::str("alice")
        };
        let mut r = launch_plan(
            state_identity(false, mutation),
            vec![feeds_kept(&[Value::Number(0.), alice.clone()], "private")],
        );
        shows(&r, "feeds", "loading");
        if ready_first {
            r.data.ready = true;
            r.data_ready().unwrap();
        }
        r.act("revision", vec![Value::Number(9.)]).unwrap();
        r.act("identify", vec![alice.clone()]).unwrap();
        shows(&r, "feeds", "private");
        assert!(r.resources[0].stale);
        assert!(r
            .carry()
            .resources
            .iter()
            .all(|(name, _, _, _)| name != "feeds"));
        if !ready_first {
            r.data.ready = true;
            r.data_ready().unwrap();
        }
        shows(&r, "feeds", "private");
        assert_eq!(
            r.data.asks.last().unwrap().1,
            [Value::Number(0.), alice.clone(), Value::Number(9.)]
        );
        settle_requests(&mut r);
        shows(&r, "feeds", "feeds of alice");
    }
}

#[test]
fn a_read_discovered_after_the_bake_does_not_declare_a_boot_reader() {
    let plan = plan_with(false, false, false);
    assert!(!plan.resources[0].reader);
    let mut first = launch_plan(plan.clone(), vec![]);
    first.data.ready = true;
    first.data.delayed_identity = true;
    first.data_ready().unwrap();
    assert!(
        !first.resource_reads_store("account"),
        "the first ask has not read storage"
    );
    settle_requests(&mut first);
    settle_requests(&mut first);
    assert!(
        first.resource_reads_store("account"),
        "the reply discovers the read"
    );
    shows(&first, "account", "public");
    assert!(first.store.kept(&kept::kept_name("account")).is_some());
    let next = launch_plan(plan, first.store.snapshot());
    shows(&next, "account", "nobody");
    assert!(!next.resource_reads_store("account"));
    assert!(next.store.kept(&kept::kept_name("account")).is_none());
}

#[test]
fn a_late_resource_identity_admits_a_seed_while_its_network_answer_is_pending() {
    for (through_derive, initial) in [
        (false, "nobody"),
        (true, "nobody"),
        (false, "public"),
        (true, "public"),
    ] {
        let mut r = launch_plan(
            plan_with_initial(through_derive, false, true, initial),
            vec![feeds_kept(
                &[Value::Number(0.), Value::str("public")],
                "private",
            )],
        );
        shows(&r, "feeds", "loading");
        r.data.ready = true;
        r.data_ready().unwrap();
        shows(&r, "feeds", "private");
        assert!(r.resources[1].stale);
        settle_requests(&mut r);
        shows(&r, "feeds", "feeds of public");
    }
}

#[test]
fn a_known_different_identity_consumes_an_unadmitted_seed() {
    consumed_seed_stays_discarded(false);
}

#[test]
fn an_admitted_seed_departure_stays_discarded_across_reload() {
    consumed_seed_stays_discarded(true);
}

fn consumed_seed_stays_discarded(admitted: bool) {
    for module in [false, true] {
        for ready in [false, true] {
            let (mut plan, name, fallback, entry) = if module {
                (
                    kept_module_plan(false),
                    "account",
                    "public",
                    (
                        kept::kept_name("account"),
                        kept::encode(&[Value::str("alice")], &Value::str("private")),
                    ),
                )
            } else {
                (
                    state_identity(false, false),
                    "feeds",
                    "loading",
                    feeds_kept(&[Value::Number(0.), Value::str("alice")], "private"),
                )
            };
            plan.resources[0].placeholder_value = plan.resources[0].initial;
            let mut r = launch_plan(plan.clone(), vec![entry.clone()]);
            if module {
                r.bind_extensions(&["test.kept"], &[]);
            }
            if admitted {
                r.act("identify", vec![Value::str("alice")]).unwrap();
                shows(&r, name, "private");
            }
            r.act("identify", vec![Value::str("bob")]).unwrap();
            r.act("identify", vec![Value::str("alice")]).unwrap();
            shows(&r, name, fallback);
            assert!(r.resources[0].kept_candidate.is_none());
            assert_eq!(r.store.kept(&entry.0), Some(entry.1.as_slice()));
            for _ in 0..2 {
                r = reload_plan(plan.clone(), &r, ready);
                shows(&r, name, fallback);
                assert!(r.resources[0].kept_candidate.is_none());
                r.act("identify", vec![Value::str("")]).unwrap();
                r.act("identify", vec![Value::str("alice")]).unwrap();
                shows(&r, name, fallback);
            }
        }
    }
}

#[test]
fn a_replaced_or_forgotten_entry_cannot_be_admitted_later() {
    for forget in [false, true] {
        let original = feeds_kept(&[Value::Number(0.), Value::str("alice")], "private");
        let mut r = launch_plan(state_identity(false, false), vec![original.clone()]);
        let snapshot = if forget {
            vec![]
        } else {
            vec![feeds_kept(
                &[Value::Number(0.), Value::str("alice")],
                "replacement",
            )]
        };
        r.adopt_store(snapshot).unwrap();
        // Even putting the original bytes back does not revive a consumed candidate.
        r.adopt_store(vec![original]).unwrap();
        r.act("identify", vec![Value::str("alice")]).unwrap();
        shows(&r, "feeds", "loading");
    }
}

#[test]
fn a_fresh_answer_supersedes_the_unadmitted_seed() {
    let mut r = launch_plan(
        state_identity(false, false),
        vec![feeds_kept(
            &[Value::Number(0.), Value::str("alice")],
            "private",
        )],
    );
    r.data.ready = true;
    r.data_ready().unwrap();
    settle_requests(&mut r);
    shows(&r, "feeds", "feeds of ");
    r.act("identify", vec![Value::str("alice")]).unwrap();
    shows(&r, "feeds", "feeds of ");
    settle_requests(&mut r);
    shows(&r, "feeds", "feeds of alice");
}

#[test]
fn refusal_diagnostics_name_the_input_and_distinguish_a_known_account() {
    let mut r = launch_plan(
        state_identity(false, false),
        vec![feeds_kept(
            &[Value::Number(0.), Value::str("alice")],
            "private",
        )],
    );
    assert!(r
        .journal()
        .any(|l| l.contains("feeds's kept answer is waiting")));
    assert!(!r.journal().any(|l| l.contains("feeds did not use")));
    r.act("identify", vec![Value::str("bob")]).unwrap();
    let line = r
        .journal()
        .find(|l| l.contains("feeds did not use"))
        .unwrap();
    assert!(
        line.contains("identifying argument 2 is \"bob\"")
            && line.contains("state `identity` is \"bob\""),
        "{line}"
    );
    assert!(
        !line.contains("persist"),
        "a search input need not be persisted: {line}"
    );
    let r = launch(
        true,
        vec![
            (
                kept::kept_name("account"),
                kept::encode(&[Value::Number(0.)], &Value::str("bob")),
            ),
            feeds_kept(&[Value::Number(0.), Value::str("alice")], "private"),
        ],
    );
    shows(&r, "account", "bob");
    shows(&r, "feeds", "loading");
    let line = r
        .journal()
        .find(|l| l.contains("feeds did not use"))
        .unwrap();
    assert!(
        line.contains("resource `account` is \"bob\" (admitted kept answer)"),
        "{line}"
    );
    assert!(!line.contains("no admitted device answer"), "{line}");
}

#[test]
fn rollback_restores_a_waiting_candidate_after_admission_or_departure() {
    for identity in ["alice", "bob"] {
        let mut r = launch_plan(
            state_identity(false, false),
            vec![feeds_kept(
                &[Value::Number(0.), Value::str("alice")],
                "private",
            )],
        );
        let checkpoint = r.checkpoint();
        r.slots[1] = Value::str(identity);
        r.settle(false).unwrap();
        assert!(r.resources[0].kept_candidate.is_none());
        let refusal: Result<(), RunnerError> = Err(RunnerError::Cycle);
        r.conclude(checkpoint, &refusal, false);
        shows(&r, "feeds", "loading");
        assert!(r.resources[0].kept_candidate.is_some());
        r.act("identify", vec![Value::str("alice")]).unwrap();
        shows(&r, "feeds", "private");
    }
}

fn kept_module_plan(gated: bool) -> Plan {
    kept_module_plan_with(gated, "account", false)
}

fn kept_module_plan_with(gated: bool, source_name: &str, numeric: bool) -> Plan {
    let mut b = PlanBuilder::new(exact_kernel::SCHEMA_DIGEST, 1);
    let string = b.primitive(TypeKind::String);
    let result = if numeric {
        b.primitive(TypeKind::Number)
    } else {
        string
    };
    let fallback = if numeric {
        Value::Number(0.)
    } else {
        Value::str("public")
    };
    let source = b.source(source_name, &[string], result);
    b.module(
        source,
        ("test.kept", "@fixture/test.kept"),
        "account",
        false,
        true,
        "never",
        "",
        r#"{"kept":true}"#,
    );
    let empty = b.constant(&Value::str(""));
    let identity = b.slot("identity", string, empty);
    let mut arg = Asm::new();
    arg.load_slot(identity);
    let arg = b.code(arg);
    let account = b.resource("account", source_name, &[arg], result, Some(&fallback));
    b.set_resource_initial_args(account, &[Value::str("")]);
    let mut set = Asm::new();
    set.load_param(0).store_slot(identity);
    let set = b.code(set);
    b.action("identify", &[("value", string)], &[identity], set);
    if gated {
        b.set_resource_placeholder_value(account, &fallback);
        closed_gate(&mut b, account);
    }
    b.node(NodeType::View as u8, None, None, 0, &[], &[], None);
    b.finish().unwrap()
}

#[test]
fn a_kept_module_entry_waits_for_identity_without_making_a_store_reader() {
    for ready in [false, true] {
        let plan = kept_module_plan(false);
        let r = launch_plan(
            plan.clone(),
            vec![(
                kept::kept_name("account"),
                kept::encode(&[Value::str("alice")], &Value::str("alice")),
            )],
        );
        shows(&r, "account", "public");
        let mut r = reload_plan(plan, &r, false);
        assert!(!r.resource_reads_store("account"));
        assert!(!r.asks_on_store_change(0));
        if ready {
            r.data.ready = true;
            r.data_ready().unwrap();
        }
        r.act("identify", vec![Value::str("alice")]).unwrap();
        shows(&r, "account", "alice");
        assert!(r.resources[0].stale);
    }
}

#[test]
fn a_kept_module_reload_checks_source_logic_shape_and_transience() {
    for change in ["source", "logic", "shape", "transient"] {
        let original = launch_plan(
            kept_module_plan(false),
            vec![(
                kept::kept_name("account"),
                kept::encode(&[Value::str("alice")], &Value::str("private")),
            )],
        );
        let mut carried = original.carry();
        if change == "logic" {
            carried.data_revision = Some("another revision".into());
        }
        let mut plan = kept_module_plan_with(
            false,
            if change == "source" {
                "other"
            } else {
                "account"
            },
            change == "shape",
        );
        plan.resources[0].transient = change == "transient";
        let mut next = Runner::boot_carrying(
            plan,
            Source::default(),
            Kernel::with_monospace(),
            &carried,
            Default::default(),
            "/",
        )
        .unwrap();
        next.act("identify", vec![Value::str("alice")]).unwrap();
        assert_eq!(
            next.resource("account"),
            Some(&if change == "shape" {
                Value::Number(0.)
            } else {
                Value::str("public")
            }),
            "{change}"
        );
        assert!(next.resources[0].kept_candidate.is_none(), "{change}");
        if change != "shape" {
            assert!(
                next.store.kept(&kept::kept_name("account")).is_none(),
                "{change}"
            );
        }
    }
}

fn reload_plan(plan: Plan, r: &Runner<Source>, ready: bool) -> Runner<Source> {
    let module = !plan.modules.is_empty();
    let mut next = Runner::boot_carrying(
        plan,
        Source {
            ready: ready && !module,
            ..Source::default()
        },
        Kernel::with_monospace(),
        &r.carry(),
        Default::default(),
        "/",
    )
    .unwrap();
    if module {
        next.bind_extensions(&["test.kept"], &[]);
        if ready {
            next.data.ready = true;
            next.data_ready().unwrap();
        }
    }
    next
}

#[test]
fn an_idle_boot_candidate_waits_for_identity_before_or_after_readiness_and_reload() {
    for module in [false, true] {
        for ready_first in [false, true] {
            for reload in [false, true] {
                let (plan, name, fallback, entry) = if module {
                    (
                        kept_module_plan(true),
                        "account",
                        "public",
                        (
                            kept::kept_name("account"),
                            kept::encode(&[Value::str("alice")], &Value::str("private")),
                        ),
                    )
                } else {
                    (
                        state_identity_gated(false, false, true),
                        "feeds",
                        "loading",
                        feeds_kept(&[Value::Number(0.), Value::str("alice")], "private"),
                    )
                };
                let mut r = launch_plan(plan.clone(), vec![entry]);
                if module {
                    r.bind_extensions(&["test.kept"], &[]);
                }
                shows(&r, name, "private");
                assert!(r.resources[0].stale);
                assert!(r.resources[0].kept_candidate.is_some());
                if ready_first {
                    r.data.ready = true;
                    r.data_ready().unwrap();
                }
                if reload {
                    r = reload_plan(plan, &r, ready_first);
                }
                assert!(r.resources[0].stale);
                assert!(r.carry().resources.is_empty());
                r.act("enable", vec![Value::Bool(true)]).unwrap();
                shows(&r, name, fallback);
                assert!(r.resources[0].kept_candidate.is_some());
                r.act("identify", vec![Value::str("alice")]).unwrap();
                shows(&r, name, "private");
                if !ready_first {
                    r.data.ready = true;
                    r.data_ready().unwrap();
                }
                settle_requests(&mut r);
                shows(&r, name, if module { "public" } else { "feeds of alice" });
            }
        }
    }
}

#[test]
fn an_idle_boot_keeps_its_seed_until_the_matching_identity_is_asked() {
    for module in [false, true] {
        for ready_first in [false, true] {
            let (plan, name, entry) = if module {
                (
                    kept_module_plan(true),
                    "account",
                    (
                        kept::kept_name("account"),
                        kept::encode(&[Value::str("")], &Value::str("private")),
                    ),
                )
            } else {
                (
                    state_identity_gated(false, false, true),
                    "feeds",
                    feeds_kept(&[Value::Number(0.), Value::str("")], "private"),
                )
            };
            let mut r = launch_plan(plan, vec![entry]);
            if module {
                r.bind_extensions(&["test.kept"], &[]);
            }
            shows(&r, name, "private");
            assert!(r.data.asks.is_empty());
            if ready_first {
                r.data.ready = true;
                r.data_ready().unwrap();
                shows(&r, name, "private");
                assert!(r.data.asks.is_empty());
            }
            r.act("enable", vec![Value::Bool(true)]).unwrap();
            if !ready_first {
                shows(&r, name, "private");
                r.data.ready = true;
                r.data_ready().unwrap();
            }
            settle_requests(&mut r);
            shows(&r, name, if module { "public" } else { "feeds of " });
            assert!(!r.resources[0].stale);
        }
    }
}

#[test]
fn a_waiting_candidate_survives_reload_before_or_after_readiness() {
    for ready in [false, true] {
        let plan = state_identity(false, false);
        let r = launch_plan(
            plan.clone(),
            vec![feeds_kept(
                &[Value::Number(0.), Value::str("alice")],
                "private",
            )],
        );
        let mut r = reload_plan(plan, &r, ready);
        r.act("identify", vec![Value::str("alice")]).unwrap();
        shows(&r, "feeds", "private");
    }
}

#[test]
fn an_invalid_candidate_is_journaled_and_forgotten_on_a_ready_match_and_reload() {
    use sha2::{Digest, Sha256};

    for undecodable in [false, true] {
        for match_at_boot in [false, true] {
            let mut plan = state_identity(false, false);
            plan.resources[0].placeholder_value = plan.resources[0].initial;
            let args = [Value::Number(0.), Value::str("alice")];
            let r = launch_plan(plan.clone(), vec![feeds_kept(&args, "private")]);
            let mut carried = r.carry();
            // A carried digest identifies the entry; admission must still decode
            // and check its value against this boot's shape.
            let mut entry = kept::encode(&args, &Value::Number(42.));
            if undecodable {
                entry.pop();
            }
            carried.kept_candidates[0].3 = Sha256::digest(&entry).into();
            carried.store = vec![(kept::kept_name("feeds"), entry)];
            if match_at_boot {
                carried.slots[1].1 = Value::str("alice");
            }
            let mut r = Runner::boot_carrying(
                plan.clone(),
                Source {
                    ready: true,
                    ..Source::default()
                },
                Kernel::with_monospace(),
                &carried,
                Default::default(),
                "/",
            )
            .unwrap();
            if !match_at_boot {
                assert!(r.resources[0].kept_candidate.is_some());
                r = reload_plan(plan.clone(), &r, true);
                r.act("identify", vec![Value::str("alice")]).unwrap();
            }
            shows(&r, "feeds", "loading");
            assert!(r.resources[0].pending);
            assert!(!r.resources[0].state.as_ref().unwrap().kept_seed);
            assert!(r.resources[0].kept_candidate.is_none());
            let refusal = "feeds did not use its kept answer: it no longer decodes or fits";
            assert_eq!(r.journal().filter(|l| l.ends_with(refusal)).count(), 1);
            assert!(r.store.kept(&kept::kept_name("feeds")).is_none());
            assert!(r.carry().kept_candidates.is_empty());

            let mut r = reload_plan(plan, &r, true);
            shows(&r, "feeds", "loading");
            assert!(r.resources[0].kept_candidate.is_none());
            assert!(r.store.kept(&kept::kept_name("feeds")).is_none());
            assert!(!r.journal().any(|l| l.ends_with(refusal)));
            settle_requests(&mut r);
            shows(&r, "feeds", "feeds of alice");
        }
    }
}

#[test]
fn a_leftover_entry_does_not_declare_an_idle_store_reader() {
    let mut plan = state_identity_gated(false, false, true);
    plan.resources[0].reader = false;
    let mut snapshot = vec![feeds_kept(
        &[Value::Number(0.), Value::str("alice")],
        "private",
    )];
    for _ in 0..2 {
        let r = launch_plan(plan.clone(), snapshot);
        shows(&r, "feeds", "loading");
        assert!(!r.resource_reads_store("feeds"));
        assert!(r.resources[0].kept_candidate.is_none());
        assert!(r.store.kept(&kept::kept_name("feeds")).is_none());
        snapshot = r.store.snapshot();
    }
}

#[test]
fn a_mutation_declaration_does_not_name_a_states_initial_or_assigned_value() {
    for assigned in [false, true] {
        let identity = if assigned {
            Value::NONE
        } else {
            Value::some(Value::str("alice"))
        };
        let mut r = launch_plan(
            state_identity(false, true),
            vec![feeds_kept(&[Value::Number(0.), identity], "private")],
        );
        if assigned {
            r.act("identify", vec![Value::some(Value::str("bob"))])
                .unwrap();
        }
        if !assigned {
            assert!(r
                .journal()
                .any(|l| l.contains("feeds's kept answer is waiting")));
            assert!(!r.journal().any(|l| l.contains("feeds did not use")));
            continue;
        }
        let line = r
            .journal()
            .find(|l| l.contains("feeds did not use"))
            .unwrap();
        let expected = if assigned {
            "state `identity` is \"bob\""
        } else {
            "state `identity` is null"
        };
        assert!(line.contains(expected), "{line}");
        assert!(!line.contains("mutation"), "{line}");
    }
}

#[test]
fn invalid_context_does_not_consume_a_waiting_identity() {
    let mut r = launch_plan(
        state_identity(false, false),
        vec![feeds_kept(
            &[Value::Number(0.), Value::str("alice")],
            "private",
        )],
    );
    let mut states: Vec<_> = r.resources.iter().map(|row| row.state.clone()).collect();
    r.admit_kept(
        0,
        &[Value::Number(0.), Value::str(""), Value::Number(f64::NAN)],
        &mut states,
        false,
        false,
    );
    assert!(r.resources[0].kept_candidate.is_some());
    r.act("identify", vec![Value::str("alice")]).unwrap();
    shows(&r, "feeds", "private");
}

#[test]
fn an_invalidated_idle_candidate_cannot_leave_a_reusable_seed() {
    for forget in [false, true] {
        let mut r = launch_plan(
            state_identity_gated(false, false, true),
            vec![feeds_kept(
                &[Value::Number(0.), Value::str("alice")],
                "private",
            )],
        );
        shows(&r, "feeds", "private");
        r.adopt_store(if forget {
            vec![]
        } else {
            vec![feeds_kept(
                &[Value::Number(0.), Value::str("alice")],
                "replacement",
            )]
        })
        .unwrap();
        r.act("enable", vec![Value::Bool(true)]).unwrap();
        shows(&r, "feeds", "loading");
        assert!(r.resources[0].kept_candidate.is_none());
        // Changing context re-evaluates the same empty identity after waking.
        r.act("revision", vec![Value::Number(1.)]).unwrap();
        assert!(
            !r.resources[0].current,
            "the fallback is not a post-wake answer"
        );
        r.data.ready = true;
        r.data_ready().unwrap();
        assert!(!r.resources[0].current, "the post-wake request is pending");
        settle_requests(&mut r);
        assert!(r.resources[0].current);
    }
}

#[test]
fn late_admission_keeps_sleep_history_until_a_post_wake_answer() {
    for ready_first in [false, true] {
        for identify_while_idle in [false, true] {
            let mut r = launch_plan(
                kept_module_plan(true),
                vec![(
                    kept::kept_name("account"),
                    kept::encode(&[Value::str("alice")], &Value::str("private")),
                )],
            );
            r.bind_extensions(&["test.kept"], &[]);
            // Evaluate the empty identity before another visit closes the gate.
            r.act("enable", vec![Value::Bool(true)]).unwrap();
            r.act("enable", vec![Value::Bool(false)]).unwrap();
            if ready_first {
                r.data.ready = true;
                r.data_ready().unwrap();
            }
            if identify_while_idle {
                r.act("identify", vec![Value::str("alice")]).unwrap();
            }
            r.act("enable", vec![Value::Bool(true)]).unwrap();
            if !identify_while_idle {
                r.act("identify", vec![Value::str("alice")]).unwrap();
            }
            shows(&r, "account", "private");
            assert!(
                !r.resources[0].current,
                "admission is not a post-wake answer"
            );
            if !ready_first {
                r.data.ready = true;
                r.data_ready().unwrap();
            }
            assert!(r.resources[0].pending);
            r = reload_plan(r.plan().clone(), &r, true);
            shows(&r, "account", "private");
            assert!(r.resources[0].pending && r.resources[0].stale);
            assert!(!r.resources[0].current, "the post-wake request is pending");
            settle_requests(&mut r);
            shows(&r, "account", "public");
            assert!(r.resources[0].current);
        }
    }
}

#[test]
fn a_waiting_candidate_keeps_sleep_history_across_reload_after_waking() {
    for module in [false, true] {
        for ready in [false, true] {
            let (plan, name, entry) = if module {
                (
                    kept_module_plan(true),
                    "account",
                    (
                        kept::kept_name("account"),
                        kept::encode(&[Value::str("alice")], &Value::str("private")),
                    ),
                )
            } else {
                (
                    state_identity_gated(false, false, true),
                    "feeds",
                    feeds_kept(&[Value::Number(0.), Value::str("alice")], "private"),
                )
            };
            let mut r = launch_plan(plan.clone(), vec![entry]);
            if module {
                r.bind_extensions(&["test.kept"], &[]);
            }
            r.act("enable", vec![Value::Bool(true)]).unwrap();
            // Reload while awake, with the identity still empty and no fresh answer.
            for _ in 0..2 {
                r = reload_plan(plan.clone(), &r, ready);
                assert!(r.resources[0].kept_candidate.is_some());
                assert!(!r.resources[0].current);
            }
            r.act("identify", vec![Value::str("alice")]).unwrap();
            shows(&r, name, "private");
            assert!(
                !r.resources[0].current,
                "admission is not a post-wake answer"
            );
            if !ready {
                r.data.ready = true;
                r.data_ready().unwrap();
            }
            assert!(r.resources[0].pending);
            assert!(!r.resources[0].current);
            settle_requests(&mut r);
            shows(&r, name, if module { "public" } else { "feeds of alice" });
            assert!(r.resources[0].current);
        }
    }
}

#[test]
fn keeping_a_new_identity_invalidates_the_waiting_entry() {
    let mut r = launch_plan(
        state_identity(false, false),
        vec![feeds_kept(
            &[Value::Number(0.), Value::str("alice")],
            "private",
        )],
    );
    r.data.ready = true;
    r.data_ready().unwrap();
    r.keep_answer(
        0,
        &[Value::Number(0.), Value::str(""), Value::Number(0.)],
        &Value::str("feeds of "),
    );
    assert!(r.resources[0].kept_candidate.is_none());
}

#[test]
fn a_replaced_waiting_entry_does_not_make_a_carried_answer_stale() {
    let plan = state_identity(false, false);
    let mut r = launch_plan(
        plan.clone(),
        vec![feeds_kept(
            &[Value::Number(0.), Value::str("alice")],
            "private",
        )],
    );
    r.data.ready = true;
    r.data_ready().unwrap();
    settle_requests(&mut r);
    // Simulate a carried candidate captured before the replacement was validated.
    let mut carried = r.carry();
    carried.kept_candidates = launch_plan(
        plan.clone(),
        vec![feeds_kept(
            &[Value::Number(0.), Value::str("alice")],
            "private",
        )],
    )
    .carry()
    .kept_candidates;
    let mut next = Runner::boot_carrying(
        plan.clone(),
        Source {
            ready: true,
            ..Source::default()
        },
        Kernel::with_monospace(),
        &carried,
        Default::default(),
        "/",
    )
    .unwrap();
    shows(&next, "feeds", "feeds of ");
    assert!(!next.resources[0].stale, "the carried answer is fresh");
    next.data.ready = true;
    next.data_ready().unwrap();
    assert!(next.data.asks.is_empty(), "readiness must not ask it again");
    let next = reload_plan(plan, &next, true);
    shows(&next, "feeds", "feeds of ");
    assert_eq!(next.carry().resources.len(), 1);
}

#[test]
fn late_admission_with_context_is_current_and_renders_before_the_reply() {
    for ready_first in [false, true] {
        let mut r = launch_plan(
            state_identity(false, false),
            vec![feeds_kept(
                &[Value::Number(0.), Value::str("alice")],
                "private",
            )],
        );
        if ready_first {
            r.data.ready = true;
            r.data_ready().unwrap();
        }
        r.act("identify", vec![Value::str("alice")]).unwrap();
        shows(&r, "feeds", "private");
        let node = r.kernel().node(r.roots()[0]).unwrap();
        assert_eq!(node.props.str(exact_kernel::PropId::Text), Some("private"));
        assert!(r.resources[0].current);
        r.act("revision", vec![Value::Number(9.)]).unwrap();
        assert!(
            r.resources[0].current,
            "context does not identify the answer"
        );
        if !ready_first {
            r.data.ready = true;
            r.data_ready().unwrap();
        }
        assert!(r.resources[0].pending);
        assert!(r.resources[0].current);
        assert_eq!(
            r.kernel()
                .node(r.roots()[0])
                .unwrap()
                .props
                .str(exact_kernel::PropId::Text),
            Some("private"),
        );
    }
}

#[test]
fn an_admitted_seed_survives_ready_reload_until_its_fresh_reply() {
    for ready_first in [false, true] {
        let plan = state_identity(false, false);
        let mut r = launch_plan(
            plan.clone(),
            vec![feeds_kept(
                &[Value::Number(0.), Value::str("alice")],
                "private",
            )],
        );
        if ready_first {
            r.data.ready = true;
            r.data_ready().unwrap();
        }
        r.act("identify", vec![Value::str("alice")]).unwrap();
        if !ready_first {
            r.data.ready = true;
            r.data_ready().unwrap();
        }
        shows(&r, "feeds", "private");
        assert!(r.resources[0].pending && r.resources[0].stale);
        assert!(r.resources[0].kept_candidate.is_none());
        for _ in 0..2 {
            r = reload_plan(plan.clone(), &r, true);
            shows(&r, "feeds", "private");
            assert!(r.resources[0].pending && r.resources[0].stale);
            assert!(r.resources[0].current);
            assert_eq!(
                r.kernel()
                    .node(r.roots()[0])
                    .unwrap()
                    .props
                    .str(exact_kernel::PropId::Text),
                Some("private"),
            );
            assert_eq!(
                r.carry().kept_seeds[0].2,
                [Value::Number(0.), Value::str("alice")]
            );
            assert!(r.carry().resources.is_empty());
            assert!(r.document_checkpoint("/").answers.is_empty());
        }
        settle_requests(&mut r);
        shows(&r, "feeds", "feeds of alice");
        assert!(!r.resources[0].pending && !r.resources[0].stale);
        assert_eq!(r.carry().resources.len(), 1);
        assert!(r.carry().kept_consumed.is_empty());
        let r = reload_plan(plan, &r, false);
        shows(&r, "feeds", "feeds of alice");
    }
}

#[test]
fn an_admitted_seed_reload_checks_source_logic_shape_and_transience() {
    let mut original = launch_plan(
        kept_module_plan(false),
        vec![(
            kept::kept_name("account"),
            kept::encode(&[Value::str("alice")], &Value::str("private")),
        )],
    );
    original.bind_extensions(&["test.kept"], &[]);
    original.data.ready = true;
    original.data_ready().unwrap();
    original.act("identify", vec![Value::str("alice")]).unwrap();
    shows(&original, "account", "private");
    assert_eq!(original.carry().kept_seeds.len(), 1);
    for change in ["source", "logic", "shape", "transient"] {
        let mut carried = original.carry();
        if change == "logic" {
            carried.data_revision = Some("another revision".into());
        }
        let mut plan = kept_module_plan_with(
            false,
            if change == "source" {
                "other"
            } else {
                "account"
            },
            change == "shape",
        );
        plan.resources[0].transient = change == "transient";
        let next = Runner::boot_carrying(
            plan,
            Source::default(),
            Kernel::with_monospace(),
            &carried,
            Default::default(),
            "/",
        )
        .unwrap();
        assert!(next.carry().kept_seeds.is_empty(), "{change}");
        assert!(
            !next.resources[0].state.as_ref().unwrap().provisional_kept,
            "{change}"
        );
    }
}

#[path = "kept_identity_batch_tests.rs"]
mod batch;
#[path = "kept_identity_edge_tests.rs"]
mod edge;
