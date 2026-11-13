//! First admission of an unavailable boot identity (LLP 1027.005 D3 and D5 as amended;
//! issues/closed/20261113-kept-answer-refused-for-good-when-identity-starts-empty.md).
use super::*;
use crate::{held::Held, instance::Input, Carried};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub(in crate::runner) struct Candidate {
    pub(in crate::runner) args: Vec<Value>,
    pub(in crate::runner) entry: [u8; 32],
    /// Carried sleep history, transferred into the shown state at settlement.
    pub(in crate::runner) slept: bool,
}

fn json(value: &Value) -> String {
    let mut out = String::new();
    crate::agent::untyped_json(value, &mut out);
    super::super::lines::shortened(&out)
}

impl<D: DataSource> Runner<D> {
    pub(in crate::runner) fn carry_kept_consumed_in(&mut self, carried: Option<&Carried>) {
        let Some(carried) = carried else { return };
        for (i, r) in self.plan.resources.iter().enumerate() {
            self.resources[i].kept_consumed = carried.kept_consumed.iter().any(|(name, source)| {
                name == self.plan.str(r.name) && source == self.plan.str(r.source)
            });
        }
    }

    pub(in crate::runner) fn carry_kept_in(&mut self, carried: Option<&Carried>, private: bool) {
        let Some(carried) = carried.filter(|_| !private && !self.store.opened_locked()) else {
            return;
        };
        // A reload may keep a source's name and change its kind: only a
        // source that can still keep answers restores what it waited on
        // (LLP 1162.001 D4).
        fn keeps(plan: &exact_plan::Plan, i: usize) -> bool {
            kept_module(plan, i)
                || plan.resources[i].reader
                    && waits_for_module(plan, plan.str(plan.resources[i].source))
        }
        for (name, source, args, entry, slept) in &carried.kept_candidates {
            if let Some(i) = self.plan.resources.iter().position(|r| {
                self.plan.str(r.name) == name
                    && self.plan.str(r.source) == source
                    && !r.transient
                    && r.args.len as usize - usize::from(r.context) == args.len()
            }) {
                if !keeps(&self.plan, i) {
                    continue;
                }
                self.resources[i].kept_candidate = Some(Box::new(Candidate {
                    args: args.clone(),
                    entry: *entry,
                    slept: *slept,
                }));
            }
        }
        for (name, source, args, value, slept) in &carried.kept_seeds {
            let Some(i) = self.plan.resources.iter().position(|r| {
                self.plan.str(r.name) == name
                    && self.plan.str(r.source) == source
                    && !r.transient
                    && r.args.len as usize - usize::from(r.context) == args.len()
            }) else {
                continue;
            };
            if !(self.resources[i].store_reader || kept_module(&self.plan, i))
                || !keeps(&self.plan, i)
                || self.check_shape(i, value).is_err()
            {
                continue;
            }
            self.resources[i].kept_candidate = None;
            self.resources[i].kept_consumed = true;
            self.resources[i].state = Some(ResourceState {
                inspection_generation: 0,
                args: args.clone(),
                value: Held::new(value.clone()),
                store_revision: self.store.revision(),
                placeholder: false,
                kept_seed: !self.source_ready(source),
                provisional_kept: true,
                answered_for: None,
                asleep: false,
                slept: *slept,
                origin: 0,
            });
            self.resources[i].stale = true;
            self.ask_at_launch(i);
        }
        self.validate_kept_candidates();
        for row in &mut self.resources {
            if row.kept_candidate.is_some() {
                row.stale = true;
            }
        }
    }

    /// Each resource's waiting candidate, whether its entry is consumed, and
    /// whether it holds an admitted seed, as a settlement starts: a later
    /// pass judges them again against the identity that settles.
    pub(in crate::runner) fn kept_admission_at_start(
        &self,
    ) -> Vec<(Option<Box<Candidate>>, bool, bool)> {
        (self.resources.iter())
            .map(|r| {
                let seeded = r
                    .state
                    .as_ref()
                    .is_some_and(|s| s.kept_seed || s.provisional_kept);
                (r.kept_candidate.clone(), r.kept_consumed, seeded)
            })
            .collect()
    }

    pub(in crate::runner) fn remember_kept(&mut self, i: usize, args: &[Value]) {
        let name = kept_name(self.plan.str(self.resource_row(i).name));
        if let Some(entry) = self.store.kept(&name) {
            self.resources[i].kept_candidate = Some(Box::new(Candidate {
                args: args.to_vec(),
                entry: Sha256::digest(entry).into(),
                slept: false,
            }));
        }
    }

    pub(in crate::runner) fn validate_kept_candidates(&mut self) {
        for i in 0..self.resources.len() {
            if let Some(candidate) = &self.resources[i].kept_candidate {
                let name = kept_name(self.plan.str(self.resource_row(i).name));
                if !self
                    .store
                    .kept(&name)
                    .is_some_and(|entry| <[u8; 32]>::from(Sha256::digest(entry)) == candidate.entry)
                {
                    self.resources[i].kept_candidate = None;
                    self.resources[i].kept_consumed = true;
                    if self.resources[i]
                        .state
                        .as_ref()
                        .is_some_and(|s| s.kept_seed)
                    {
                        self.replace_invalid_seed(i);
                    }
                }
            }
        }
    }

    fn replace_invalid_seed(&mut self, i: usize) {
        let row = self.resource_row(i);
        let (value, args, placeholder) = if row.initial.len > 0 {
            let value = Value::from_bytes(self.plan.bytes(row.initial)).expect("compiled value");
            let Value::List(args) =
                Value::from_bytes(self.plan.bytes(row.initial_args)).expect("compiled arguments")
            else {
                unreachable!("compiled arguments are a list")
            };
            (Held::compiled(value, row.initial), args.to_vec(), false)
        } else {
            let resources: Vec<_> = self.resources.iter().map(|r| r.value.clone()).collect();
            let value = self
                .placeholder(i, row, &resources)
                .or_else(|| super::super::settlement::zero(&self.plan, row.ty))
                .expect("resource type has a zero");
            (Held::new(value), Vec::new(), true)
        };
        // Replacing an invalid seed is not an answer since the last wake.
        let state = self.resources[i].state.as_mut().expect("seed");
        state.value = value;
        state.args = args;
        state.placeholder = placeholder;
        state.kept_seed = false;
        state.provisional_kept = false;
        state.answered_for = None;
        state.inspection_generation = 0;
        state.origin = 0;
    }

    fn identity_inputs(&self, i: usize, n: usize) -> Vec<Input> {
        let deps = self.sites.deps();
        let arg = self.resource_row(i).args.iter().nth(n).expect("argument");
        let mut work = vec![deps.reads_of(&self.plan, self.plan.arg(arg).expr)];
        let mut derives = vec![false; self.plan.derives.len()];
        let mut inputs = Vec::new();
        while let Some(reads) = work.pop() {
            for input in deps.inputs(&reads) {
                match input {
                    Input::Derive(d) if !derives[d] => {
                        derives[d] = true;
                        work.push(deps.derives[d].clone());
                    }
                    Input::Derive(_) => {}
                    _ => inputs.push(input),
                }
            }
        }
        inputs
    }

    pub(super) fn unknown_argument(
        &self,
        i: usize,
        n: usize,
        states: &[Option<ResourceState>],
    ) -> bool {
        self.identity_inputs(i, n).iter().any(|input| match *input {
            Input::Resource(r) | Input::PendingResource(r) | Input::FailedResource(r) => {
                self.resources[r].store_reader
                    && states[r]
                        .as_ref()
                        .is_some_and(|s| s.placeholder || s.value.is_compiled())
            }
            _ => false,
        })
    }

    fn waiting_identity(
        &self,
        i: usize,
        kept: &[Value],
        now: &[Value],
        states: &[Option<ResourceState>],
    ) -> bool {
        kept.iter().zip(now).enumerate().all(|(n, (kept, now))| {
            kept == now
                || self.unknown_argument(i, n, states)
                || now.as_str() == Some("")
                || *now == Value::NONE
        })
    }

    fn refuse_kept(
        &mut self,
        i: usize,
        kept: &[Value],
        now: &[Value],
        states: &[Option<ResourceState>],
        waiting: bool,
    ) {
        if self.keeping.refused.iter().any(|(r, _)| *r == i) {
            return;
        }
        if waiting {
            self.keeping.refused.push((
                i,
                super::super::lines::kept_waiting(self.plan.str(self.resource_row(i).name)),
            ));
            return;
        }
        let unknown = self.identity_unknown(i, states);
        let mut why = if unknown {
            "this launch's arguments read a store reader still on its fallback".to_string()
        } else {
            format!("this launch asks for {}", json(&Value::list(now.to_vec())))
        };
        for (n, (kept, value)) in kept.iter().zip(now).enumerate() {
            if kept == value && !self.unknown_argument(i, n, states) {
                continue;
            }
            why.push_str(&format!(
                "; identifying argument {} is {}",
                n + 1,
                json(value)
            ));
            for input in self.identity_inputs(i, n) {
                let detail = match input {
                    Input::Slot(s) => {
                        let name = self.plan.str(self.plan.slots[s].name);
                        format!("state `{name}` is {}", json(&self.slots[s]))
                    }
                    Input::Resource(r) | Input::PendingResource(r) | Input::FailedResource(r) => {
                        let name = self.plan.str(self.resource_row(r).name);
                        let Some(state) = &states[r] else { continue };
                        let provenance = if self.resources[r].store_reader
                            && (state.placeholder || state.value.is_compiled())
                        {
                            " (no admitted device answer)"
                        } else if state.kept_seed || state.provisional_kept {
                            // An admitted answer is still last session's
                            // until its fresh reply, ready source or not.
                            " (admitted kept answer)"
                        } else {
                            ""
                        };
                        format!(
                            "resource `{name}` is {}{provenance}",
                            json(state.value.get(&self.plan))
                        )
                    }
                    _ => continue,
                };
                why.push_str(&format!("; {detail}"));
            }
        }
        let line = super::super::lines::kept_refused(
            self.plan.str(self.resource_row(i).name),
            Some(&json(&Value::list(kept.to_vec()))),
            &why,
        );
        self.keeping.refused.push((i, line));
    }

    /// Only an unadmitted boot seed waits. A known different identity, a
    /// replaced entry or an admitted seed's departure consumes eligibility.
    /// Resource rows and the store checkpoint restore this on refusal.
    pub(in crate::runner) fn admit_kept(
        &mut self,
        i: usize,
        args: &[Value],
        states: &mut [Option<ResourceState>],
        ready: bool,
        valid: bool,
    ) -> (bool, bool, bool) {
        if self.resources[i].kept_candidate.is_none()
            && !states[i]
                .as_ref()
                .is_some_and(|s| s.kept_seed || s.provisional_kept)
        {
            return (false, false, false);
        }
        let row = self.resource_row(i);
        let now = &args[..row.args.len as usize - usize::from(row.context)];
        let unknown = self.identity_unknown(i, states);
        if let Some(state) = &mut states[i] {
            state.provisional_kept &= !unknown && state.args == now;
        }
        if self.resources[i].kept_candidate.is_none()
            && !states[i].as_ref().is_some_and(|s| s.kept_seed)
        {
            return (false, false, false);
        }
        let row = self.resource_row(i);
        let name = kept_name(self.plan.str(row.name));
        let seeded_before = states[i].as_ref().is_some_and(|s| s.kept_seed);
        let mut admitted = false;
        if let Some(candidate) = self.resources[i].kept_candidate.take() {
            self.resources[i].kept_consumed = true;
            let entry = self
                .store
                .kept(&name)
                .filter(|bytes| <[u8; 32]>::from(Sha256::digest(bytes)) == candidate.entry);
            if let Some(entry) = entry {
                if valid && !unknown && candidate.args == now {
                    // An installed seed already crossed the shape boundary.
                    // A later admission rechecks the bounded store entry.
                    if !seeded_before {
                        let seed =
                            decode(entry).filter(|(_, value)| self.check_shape(i, value).is_ok());
                        if let Some((args, value)) = seed {
                            states[i] = Some(ResourceState {
                                inspection_generation: 0,
                                args,
                                value: Held::new(value),
                                store_revision: self.store.revision(),
                                placeholder: false,
                                kept_seed: !ready,
                                provisional_kept: true,
                                answered_for: None,
                                asleep: false,
                                slept: candidate.slept
                                    || states[i].as_ref().is_some_and(|s| s.slept),
                                origin: 0,
                            });
                            self.resources[i].stale = true;
                            admitted = true;
                            if ready || self.tree.is_some() {
                                self.keeping.refused.push((
                                    i,
                                    super::super::lines::kept_shown(
                                        self.plan.str(self.resource_row(i).name),
                                        &json(&Value::list(now.to_vec())),
                                    ),
                                ));
                            }
                        } else {
                            self.keeping.refused.push((
                                i,
                                super::super::lines::kept_refused(
                                    self.plan.str(self.resource_row(i).name),
                                    None,
                                    "it no longer decodes or fits",
                                ),
                            ));
                            self.store.forget_kept(&name);
                        }
                    }
                    if seeded_before {
                        states[i].as_mut().expect("seed").provisional_kept = true;
                    }
                } else {
                    let waiting = self.waiting_identity(i, &candidate.args, now, states);
                    if !waiting || seeded_before || states[i].is_none() {
                        self.refuse_kept(i, &candidate.args, now, states, waiting);
                    }
                    if waiting {
                        self.resources[i].kept_consumed = false;
                        self.resources[i].kept_candidate = Some(candidate);
                    }
                    // The gate may have left an unvalidated seed asleep.
                    // Once its identity is evaluated, only a match may stand.
                    if seeded_before {
                        states[i] = None;
                    }
                }
            } else if seeded_before {
                states[i] = None;
            }
        }
        let seeded = states[i].as_ref().is_some_and(|s| s.kept_seed);
        let waiting = if kept_surface(&self.plan, i) {
            self.surface_seed_waiting(i)
        } else {
            !ready
        };
        let keep = seeded
            && valid
            && !unknown
            && waiting
            && states[i].as_ref().is_some_and(|s| s.args == now);
        if seeded && !keep && waiting {
            let kept = states[i].as_ref().expect("seed").args.clone();
            self.refuse_kept(i, &kept, now, states, false);
        }
        if let Some(state) = &mut states[i] {
            state.kept_seed = keep;
        }
        (keep, seeded && unknown, admitted)
    }
}
