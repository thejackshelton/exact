//! Admission follows the identity after every store reader in the batch settles.
use super::*;

struct BatchSource {
    inner: Source,
    token: String,
}

impl DataSource for BatchSource {
    fn query(&mut self, _: &str, _: &[Value]) -> Result<Value, DataError> {
        unreachable!("the fixture implements answer")
    }

    fn answer(
        &mut self,
        store: &mut Store,
        source: &str,
        args: &[Value],
    ) -> Result<Answer, DataError> {
        if source == "writer" {
            store.set("token", &self.token).unwrap();
            return Ok(Answer::Now(Value::Unit));
        }
        self.inner.answer(store, source, args)
    }

    fn parse(
        &mut self,
        store: &mut Store,
        source: &str,
        args: &[Value],
        outcome: Outcome,
    ) -> Result<Answer, DataError> {
        self.inner.parse(store, source, args, outcome)
    }

    fn ready(&self) -> bool {
        self.inner.ready
    }
    fn grants(&self) -> &str {
        self.inner.grants()
    }
}

fn batch(initial: &str, settled: &str) -> Runner<BatchSource> {
    batch_with(initial, settled, None)
}

/// As [`batch`]; `kept_account` is the kept answer of `account`, if any.
fn batch_with(initial: &str, settled: &str, kept_account: Option<&str>) -> Runner<BatchSource> {
    let mut b = PlanBuilder::new(exact_kernel::SCHEMA_DIGEST, 1);
    let string = b.primitive(TypeKind::String);
    let unit = b.primitive(TypeKind::Unit);
    let mut zero = Asm::new();
    zero.number(0.);
    let zero = b.code(zero);
    let account = b.resource(
        "account",
        "account",
        &[zero],
        string,
        Some(&Value::str("nobody")),
    );
    b.set_resource_reader(account, true);
    b.set_resource_initial_args(account, &[Value::Number(0.)]);
    let mut key = Asm::new();
    key.load_resource(account);
    let key = b.code(key);
    let feeds = b.resource(
        "feeds",
        "feeds",
        &[zero, key],
        string,
        Some(&Value::str("loading")),
    );
    b.set_resource_reader(feeds, true);
    b.set_resource_initial_args(feeds, &[Value::Number(0.), Value::str("nobody")]);
    let writer = b.resource("writer", "writer", &[], unit, Some(&Value::Unit));
    b.set_resource_reader(writer, true);
    b.node(NodeType::View as u8, None, None, 0, &[], &[], None);
    Runner::boot_stored(
        b.finish().unwrap(),
        BatchSource {
            inner: Source::default(),
            token: settled.to_string(),
        },
        Kernel::with_monospace(),
        [
            ("token".into(), initial.as_bytes().to_vec()),
            feeds_kept(&[Value::Number(0.), Value::str("alice")], "private"),
        ]
        .into_iter()
        .chain(kept_account.map(|value| {
            (
                kept::kept_name("account"),
                kept::encode(&[Value::Number(0.)], &Value::str(value)),
            )
        }))
        .collect(),
        Default::default(),
        "/",
    )
    .unwrap()
}

#[test]
fn a_transient_mismatch_does_not_consume_the_settled_identitys_seed() {
    let mut r = batch("bob", "alice");
    r.data.inner.ready = true;
    r.data_ready().unwrap();
    assert_eq!(r.resource("account"), Some(&Value::str("alice")));
    assert_eq!(r.resource("feeds"), Some(&Value::str("private")));
    assert!(r.resources[1].stale);
    assert!(!r
        .journal()
        .any(|line| line.contains("this launch asks for [0,\"bob\"]")));
}

#[test]
fn a_transient_match_keeps_waiting_when_the_settled_identity_is_empty() {
    let mut r = batch("alice", "");
    r.data.inner.ready = true;
    r.data_ready().unwrap();
    assert_eq!(r.resource("account"), Some(&Value::str("")));
    assert!(r.resources[1].kept_candidate.is_some());
    assert_eq!(r.resource("feeds"), Some(&Value::str("loading")));
    assert!(!r
        .journal()
        .any(|line| line.contains("feeds shows its kept answer")));
    let mut snapshot = r.store.snapshot();
    snapshot
        .iter_mut()
        .find(|(name, _)| name == "token")
        .unwrap()
        .1 = b"alice".to_vec();
    r.store_changed(snapshot).unwrap();
    assert_eq!(r.resource("feeds"), Some(&Value::str("private")));
}

#[test]
fn a_transient_mismatch_keeps_an_admitted_seed_for_the_next_reload() {
    let mut r = batch_with("bob", "alice", Some("alice"));
    assert_eq!(r.resource("feeds"), Some(&Value::str("private")));
    r.data.inner.ready = true;
    r.data_ready().unwrap();
    assert_eq!(r.resource("account"), Some(&Value::str("alice")));
    assert_eq!(r.resource("feeds"), Some(&Value::str("private")));
    assert!(!r
        .journal()
        .any(|line| line.contains("this launch asks for [0,\"bob\"]")));
    let carried = r.carry();
    assert!(
        carried.kept_seeds.iter().any(|(name, ..)| name == "feeds"),
        "the settled identity still matches feeds' admitted seed"
    );
}
