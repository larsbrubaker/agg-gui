//! A WGSL uniform-control-flow check over naga's IR, used only by the browser-validation tests
//! (`wgsl_browser_validation_tests.rs`).
//!
//! WebGPU (Tint, in Chrome) rejects a shader that calls `textureSample`, `textureSampleBias`,
//! `textureSampleCompare` or a derivative builtin (`dpdx`, `fwidth`, ...) where control flow may
//! differ between neighbouring fragments.  naga cannot check this: its analyzer carries the rule
//! (`UniformityRequirements::DERIVATIVE` / `IMPLICIT_LEVEL`) but switches it off for the fragment
//! stage with the constant `DISABLE_UNIFORMITY_REQ_FOR_FRAGMENT_STAGE = true`, so wgpu's native
//! `Validator` with `ValidationFlags::all()` accepts such a shader.  This module re-implements the
//! rule over the parsed `naga::Module`.
//!
//! The analysis follows the WGSL spec's shape, conservatively where the spec is subtle:
//!
//! * Fragment inputs, texture-sample results, derivatives, storage / private / workgroup loads and
//!   atomics are *non-uniform*; literals, constants, `var<uniform>` and handle (texture, sampler)
//!   globals are *uniform*; everything else is uniform when all its operands are.
//! * A local variable is non-uniform if any store to it writes a non-uniform value or happens in
//!   non-uniform control flow (flow-insensitive, iterated to a fixpoint).
//! * Control flow turns non-uniform inside a branch whose condition is non-uniform, and for the
//!   rest of the enclosing block after such a branch that can `return`, `break` or `continue`
//!   (the browser's complaint about the gradient shader's early return).  A loop with a
//!   non-uniform exit is non-uniform from its head on.  `discard` does not affect uniformity
//!   (the spec makes it a demote-to-helper-invocation).
//! * A call to a helper that itself needs uniform control flow is a requirement at the call site.
//!
//! Anything it does not model (ray queries, subgroup ops, ...) is treated as non-uniform, so the
//! check errs towards reporting rather than missing.

use std::collections::{BTreeSet, HashMap, HashSet};

use naga::{
    AddressSpace, Block, Expression as E, Function, Handle, LocalVariable, Module, SampleLevel,
    ShaderStage, Statement as S,
};

/// Every uniformity violation in `module`'s fragment entry points (and helpers they call), as
/// readable lines with the WGSL line number.  Empty when the module is acceptable.
pub(crate) fn check_module(module: &Module, source: &str) -> Vec<String> {
    let mut an = Analyzer {
        module,
        source,
        requires: HashMap::new(),
        violations: BTreeSet::new(),
    };
    for entry in &module.entry_points {
        if entry.stage != ShaderStage::Fragment {
            continue;
        }
        let nonuniform_args = vec![true; entry.function.arguments.len()];
        an.analyze(&entry.function, &entry.name, &nonuniform_args);
    }
    an.violations.into_iter().collect()
}

struct Analyzer<'a> {
    module: &'a Module,
    source: &'a str,
    /// Memo of [`Analyzer::requires_uniform`].
    requires: HashMap<Handle<Function>, bool>,
    violations: BTreeSet<String>,
}

/// State of one pass over one function body.
struct Pass {
    /// Whether control flow here is uniform across neighbouring fragments.
    cf: bool,
    /// Incremented by a `return` reached in non-uniform control flow.
    nonuniform_returns: u32,
    /// The function's result may differ between fragments.
    result_nonuniform: bool,
    memo: HashMap<Handle<E>, bool>,
    call_results: HashMap<Handle<E>, bool>,
}

impl<'a> Analyzer<'a> {
    /// Analyzes one function with the given argument uniformity; returns whether its result may be
    /// non-uniform.  `name` labels violations.
    fn analyze(&mut self, func: &Function, name: &str, arg_nonuniform: &[bool]) -> bool {
        let mut vars: HashSet<Handle<LocalVariable>> = HashSet::new();
        loop {
            let before = vars.len();
            let mut pass = Pass {
                cf: true,
                nonuniform_returns: 0,
                result_nonuniform: false,
                memo: HashMap::new(),
                call_results: HashMap::new(),
            };
            for (h, var) in func.local_variables.iter() {
                if let Some(init) = var.init {
                    if self.nonuniform(func, arg_nonuniform, &vars, &mut pass, init) {
                        vars.insert(h);
                    }
                }
            }
            self.block(func, name, arg_nonuniform, &mut vars, &mut pass, &func.body);
            if vars.len() == before {
                return pass.result_nonuniform;
            }
        }
    }

    fn block(
        &mut self,
        func: &Function,
        name: &str,
        args: &[bool],
        vars: &mut HashSet<Handle<LocalVariable>>,
        pass: &mut Pass,
        block: &Block,
    ) {
        for stmt in block.iter() {
            match stmt {
                S::Emit(range) => {
                    for h in range.clone() {
                        self.check_requirement(func, name, pass, h);
                    }
                }
                S::Block(b) => self.block(func, name, args, vars, pass, b),
                S::If {
                    condition,
                    accept,
                    reject,
                } => {
                    let nu = self.nonuniform(func, args, vars, pass, *condition);
                    let cf_in = pass.cf;
                    pass.cf = cf_in && !nu;
                    self.block(func, name, args, vars, pass, accept);
                    let cf_accept = pass.cf;
                    pass.cf = cf_in && !nu;
                    self.block(func, name, args, vars, pass, reject);
                    let cf_reject = pass.cf;
                    pass.cf = if nu {
                        cf_in && !(exits(accept, true, true) || exits(reject, true, true))
                    } else {
                        cf_accept && cf_reject
                    };
                }
                S::Switch { selector, cases } => {
                    let nu = self.nonuniform(func, args, vars, pass, *selector);
                    let cf_in = pass.cf;
                    let mut all_uniform = true;
                    let mut escapes = false;
                    for case in cases {
                        pass.cf = cf_in && !nu;
                        self.block(func, name, args, vars, pass, &case.body);
                        all_uniform &= pass.cf;
                        escapes |= exits(&case.body, false, true);
                    }
                    pass.cf = if nu {
                        cf_in && !escapes
                    } else {
                        cf_in && all_uniform
                    };
                }
                S::Loop {
                    body,
                    continuing,
                    break_if,
                } => {
                    let cf_in = pass.cf;
                    let returns_before = pass.nonuniform_returns;
                    self.block(func, name, args, vars, pass, body);
                    self.block(func, name, args, vars, pass, continuing);
                    let mut exit_nonuniform = !pass.cf;
                    if let Some(c) = break_if {
                        exit_nonuniform |= self.nonuniform(func, args, vars, pass, *c);
                    }
                    if exit_nonuniform && cf_in {
                        // Later iterations run with only some fragments left in the loop.
                        pass.cf = false;
                        self.block(func, name, args, vars, pass, body);
                        self.block(func, name, args, vars, pass, continuing);
                    }
                    pass.cf = cf_in && pass.nonuniform_returns == returns_before;
                }
                S::Return { value } => {
                    if !pass.cf {
                        pass.nonuniform_returns += 1;
                    }
                    pass.result_nonuniform |= !pass.cf;
                    if let Some(v) = value {
                        let nu = self.nonuniform(func, args, vars, pass, *v);
                        pass.result_nonuniform |= nu;
                    }
                }
                S::Store { pointer, value } => {
                    if let Some(var) = local_root(func, *pointer) {
                        let nu = !pass.cf || self.nonuniform(func, args, vars, pass, *value);
                        if nu && vars.insert(var) {
                            pass.memo.clear();
                        }
                    }
                }
                S::Call {
                    function,
                    arguments,
                    result,
                } => {
                    let callee = &self.module.functions[*function];
                    let callee_name = callee.name.clone().unwrap_or_else(|| "<anonymous>".into());
                    if !pass.cf && self.requires_uniform(*function) {
                        self.violations.insert(format!(
                            "`{name}` calls `{callee_name}`, which needs uniform control flow \
                             (it samples with implicit derivatives), from non-uniform control flow"
                        ));
                    }
                    let arg_nu: Vec<bool> = arguments
                        .iter()
                        .map(|a| self.nonuniform(func, args, vars, pass, *a))
                        .collect();
                    let res_nu = self.analyze(callee, &callee_name, &arg_nu);
                    if let Some(r) = result {
                        pass.call_results.insert(*r, res_nu || !pass.cf);
                    }
                }
                _ => {}
            }
        }
    }

    /// Reports `h` if it needs uniform control flow and `pass.cf` says it does not have it.
    fn check_requirement(&mut self, func: &Function, name: &str, pass: &Pass, h: Handle<E>) {
        let what = match &func.expressions[h] {
            E::ImageSample {
                level: SampleLevel::Auto | SampleLevel::Bias(_),
                ..
            } => "textureSample / textureSampleBias / textureSampleCompare",
            E::Derivative { .. } => "a derivative builtin (dpdx, dpdy, fwidth)",
            _ => return,
        };
        if pass.cf {
            return;
        }
        let line = func
            .expressions
            .get_span(h)
            .location(self.source)
            .line_number;
        self.violations.insert(format!(
            "WGSL line {line}: {what} in `{name}` is reached by non-uniform control flow \
             (after a fragment-dependent branch, early return or loop exit); the browser requires \
             it in uniform control flow"
        ));
    }

    /// Whether `f` (directly or through a callee) contains an operation needing uniform control
    /// flow.
    fn requires_uniform(&mut self, f: Handle<Function>) -> bool {
        if let Some(&r) = self.requires.get(&f) {
            return r;
        }
        let func = &self.module.functions[f];
        let mut callees = Vec::new();
        let mut direct = false;
        for_each_stmt(&func.body, &mut |s| match s {
            S::Emit(range) => {
                for h in range.clone() {
                    direct |= matches!(
                        func.expressions[h],
                        E::ImageSample {
                            level: SampleLevel::Auto | SampleLevel::Bias(_),
                            ..
                        } | E::Derivative { .. }
                    );
                }
            }
            S::Call { function, .. } => callees.push(*function),
            _ => {}
        });
        let r = direct || callees.into_iter().any(|c| self.requires_uniform(c));
        self.requires.insert(f, r);
        r
    }

    /// Whether the value of `h` may differ between neighbouring fragments.
    fn nonuniform(
        &self,
        func: &Function,
        args: &[bool],
        vars: &HashSet<Handle<LocalVariable>>,
        pass: &mut Pass,
        h: Handle<E>,
    ) -> bool {
        if let Some(&r) = pass.memo.get(&h) {
            return r;
        }
        let nu = |pass: &mut Pass, x: Handle<E>| self.nonuniform(func, args, vars, pass, x);
        let r = match &func.expressions[h] {
            E::Literal(_) | E::Constant(_) | E::Override(_) | E::ZeroValue(_) => false,
            E::FunctionArgument(i) => args.get(*i as usize).copied().unwrap_or(true),
            E::GlobalVariable(g) => !matches!(
                self.module.global_variables[*g].space,
                AddressSpace::Uniform | AddressSpace::Handle
            ),
            E::LocalVariable(v) => vars.contains(v),
            E::Load { pointer } => nu(pass, *pointer),
            E::Access { base, index } => nu(pass, *base) || nu(pass, *index),
            E::AccessIndex { base, .. } => nu(pass, *base),
            E::Splat { value, .. } => nu(pass, *value),
            E::Swizzle { vector, .. } => nu(pass, *vector),
            E::Compose { components, .. } => components.iter().any(|c| nu(pass, *c)),
            E::Unary { expr, .. } => nu(pass, *expr),
            E::Binary { left, right, .. } => nu(pass, *left) || nu(pass, *right),
            E::Select {
                condition,
                accept,
                reject,
            } => nu(pass, *condition) || nu(pass, *accept) || nu(pass, *reject),
            E::Relational { argument, .. } => nu(pass, *argument),
            E::Math {
                arg,
                arg1,
                arg2,
                arg3,
                ..
            } => {
                nu(pass, *arg)
                    || [arg1, arg2, arg3]
                        .into_iter()
                        .flatten()
                        .any(|a| nu(pass, *a))
            }
            E::As { expr, .. } => nu(pass, *expr),
            E::ImageQuery { image, .. } => nu(pass, *image),
            E::ArrayLength(_) => false,
            E::CallResult(_) => pass.call_results.get(&h).copied().unwrap_or(true),
            // Sample / load results, derivatives, atomics, ray queries, subgroup ops, ...
            _ => true,
        };
        pass.memo.insert(h, r);
        r
    }
}

/// The local variable a pointer expression ultimately points into, if any.
fn local_root(func: &Function, mut p: Handle<E>) -> Option<Handle<LocalVariable>> {
    loop {
        match &func.expressions[p] {
            E::LocalVariable(v) => return Some(*v),
            E::Access { base, .. } | E::AccessIndex { base, .. } => p = *base,
            _ => return None,
        }
    }
}

/// Whether `block` can leave its enclosing branch: a `return`, or a `break` / `continue` that is
/// not captured by a loop (or, for `break`, a switch) nested inside `block`.
/// `brk` / `cont` say whether a `break` / `continue` here would escape the branch.
fn exits(block: &Block, brk: bool, cont: bool) -> bool {
    block.iter().any(|s| match s {
        S::Return { .. } => true,
        S::Break => brk,
        S::Continue => cont,
        S::Block(b) => exits(b, brk, cont),
        S::If { accept, reject, .. } => exits(accept, brk, cont) || exits(reject, brk, cont),
        S::Switch { cases, .. } => cases.iter().any(|c| exits(&c.body, false, cont)),
        S::Loop {
            body, continuing, ..
        } => exits(body, false, false) || exits(continuing, false, false),
        _ => false,
    })
}

/// Calls `f` on every statement in `block`, nested ones included.
fn for_each_stmt<'b>(block: &'b Block, f: &mut dyn FnMut(&'b S)) {
    for s in block.iter() {
        f(s);
        match s {
            S::Block(b) => for_each_stmt(b, f),
            S::If { accept, reject, .. } => {
                for_each_stmt(accept, f);
                for_each_stmt(reject, f);
            }
            S::Switch { cases, .. } => cases.iter().for_each(|c| for_each_stmt(&c.body, f)),
            S::Loop {
                body, continuing, ..
            } => {
                for_each_stmt(body, f);
                for_each_stmt(continuing, f);
            }
            _ => {}
        }
    }
}
