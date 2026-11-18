//! Boa's implementation of ECMAScript's Scripts.
//!
//! This module contains the [`Script`] type, which represents a [**Script Record**][script].
//!
//! More information:
//!  - [ECMAScript reference][spec]
//!
//! [spec]: https://tc39.es/ecma262/#sec-scripts
//! [script]: https://tc39.es/ecma262/#sec-script-records

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use boa_gc::{Finalize, Gc, GcRefCell, Trace};
use boa_parser::{Parser, Source, source::ReadChar};

use boa_ast::{
    expression::{Call, Expression},
    scope::{Scope, ScopeLayout},
    visitor::{VisitWith, Visitor},
};
use boa_interner::Sym;
use std::ops::ControlFlow;

use crate::{
    Context, HostDefined, JsNativeError, JsResult, JsString, JsValue, Module, SpannedSourceText,
    bytecompiler::{ByteCompiler, global_declaration_instantiation_context},
    environments::EnvironmentStack,
    js_string,
    realm::Realm,
    spanned_source_text::SourceText,
    vm::{ActiveRunnable, CallFrame, CallFrameFlags, CodeBlock},
};

/// ECMAScript's [**Script Record**][spec].
///
/// [spec]: https://tc39.es/ecma262/#sec-script-records
#[derive(Clone, Trace, Finalize)]
pub struct Script {
    inner: Gc<Inner>,
}

impl std::fmt::Debug for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Script")
            .field("realm", &self.inner.realm.addr())
            .field("phase", &self.inner.phase.borrow())
            .field("loaded_modules", &self.inner.loaded_modules)
            .finish()
    }
}

#[derive(Trace, Debug, Finalize)]
enum ScriptPhase {
    Ast(#[unsafe_ignore_trace] boa_ast::Script),
    Codeblock(Gc<CodeBlock>),
}

#[derive(Trace, Finalize)]
struct Inner {
    realm: Realm,
    phase: GcRefCell<ScriptPhase>,
    source_text: SourceText,
    loaded_modules: GcRefCell<FxHashMap<JsString, Module>>,
    host_defined: HostDefined,
    path: Option<PathBuf>,
}

impl Script {
    /// Gets the realm of this script.
    #[must_use]
    pub fn realm(&self) -> &Realm {
        &self.inner.realm
    }

    /// Returns the [`ECMAScript specification`][spec] defined [`\[\[HostDefined\]\]`][`HostDefined`] field of the [`Module`].
    ///
    /// [spec]: https://tc39.es/ecma262/#script-record
    #[must_use]
    pub fn host_defined(&self) -> &HostDefined {
        &self.inner.host_defined
    }

    /// Gets the loaded modules of this script.
    pub(crate) fn loaded_modules(&self) -> &GcRefCell<FxHashMap<JsString, Module>> {
        &self.inner.loaded_modules
    }

    /// Abstract operation [`ParseScript ( sourceText, realm, hostDefined )`][spec].
    ///
    /// Parses the provided `src` as an ECMAScript script, returning an error if parsing fails.
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-parse-script
    pub fn parse<R: ReadChar>(
        src: Source<'_, R>,
        realm: Option<Realm>,
        context: &mut Context,
    ) -> JsResult<Self> {
        let path = src.path().map(Path::to_path_buf);
        let mut parser = Parser::new(src);
        parser.set_identifier(context.next_parser_identifier());
        if context.is_strict() {
            parser.set_strict();
        }
        let scope = context.realm().scope().clone();
        let (mut code, source) = parser.parse_script_with_source(&scope, context.interner_mut())?;
        if !context.optimizer_options().is_empty() {
            context.optimize_statement_list(code.statements_mut());
        }

        let source_text = SourceText::new(source);

        Ok(Self {
            inner: Gc::new(Inner {
                realm: realm.unwrap_or_else(|| context.realm().clone()),
                phase: GcRefCell::new(ScriptPhase::Ast(code)),
                source_text,
                loaded_modules: GcRefCell::default(),
                host_defined: HostDefined::default(),
                path,
            }),
        })
    }

    /// Compiles the codeblock of this script.
    ///
    /// This is a no-op if this has been called previously.
    pub fn codeblock(&self, context: &mut Context) -> JsResult<Gc<CodeBlock>> {
        let cb = {
            let phase = self.inner.phase.borrow();
            let source = match &*phase {
                ScriptPhase::Codeblock(codeblock) => return Ok(codeblock.clone()),
                ScriptPhase::Ast(source) => source,
            };

            compile_script(
                source,
                self.inner.realm.scope(),
                self.get_source(),
                self.path(),
                context,
            )?
            .0
        };

        *self.inner.phase.borrow_mut() = ScriptPhase::Codeblock(cb.clone());

        Ok(cb)
    }

    /// Parses and compiles `src` for the current realm, and also returns a
    /// [`CompiledScript`] when the compiled code can be reused in other realms.
    ///
    /// The returned [`Script`] behaves exactly like one made by
    /// [`Script::parse`] followed by [`Script::codeblock`]. Cross-realm reuse
    /// is refused (the second value is `None`) for scripts that contain a
    /// direct `eval` call or annex B block-level function hoisting, whose
    /// compiled form depends on the realm they were compiled in.
    pub fn parse_reusable<R: ReadChar>(
        src: Source<'_, R>,
        context: &mut Context,
    ) -> JsResult<(Self, Option<CompiledScript>)> {
        let path = src.path().map(Path::to_path_buf);
        let mut parser = Parser::new(src);
        parser.set_identifier(context.next_parser_identifier());
        if context.is_strict() {
            parser.set_strict();
        }

        let realm = context.realm().clone();
        let before = realm.scope().layout();
        let mut reusable = true;
        // A private copy of the global scope: the compiled code then refers
        // only to bindings that exist in every realm with the same layout,
        // and later declarations in this realm never leak into its scopes.
        // Direct eval resolves names through those scopes at run time, so a
        // script containing one is analyzed against the live scope instead.
        let (mut code, source, scope) = parser.parse_script_with_source_and_chosen_scope(
            |ast| {
                if contains_direct_eval(ast) {
                    reusable = false;
                    realm.scope().clone()
                } else {
                    Scope::new_global_from_layout(&before)
                }
            },
            context.interner_mut(),
        )?;
        if !context.optimizer_options().is_empty() {
            context.optimize_statement_list(code.statements_mut());
        }

        let source_text = SourceText::new(source);
        let (codeblock, annex_b_hoisting) =
            compile_script(&code, &scope, source_text.clone(), path.as_deref(), context)?;
        reusable &= !annex_b_hoisting;

        if !reusable && scope == *realm.scope() {
            let script = Self::from_codeblock(realm, codeblock, source_text, path);
            return Ok((script, None));
        }

        let compiled = CompiledScript {
            inner: Gc::new(CompiledInner {
                codeblock,
                source_text,
                path,
                after: scope.layout(),
                before,
            }),
        };
        let script = compiled.instantiate(Some(realm), context).ok_or_else(|| {
            JsNativeError::error().with_message("global scope changed while compiling a script")
        })?;
        Ok((script, reusable.then_some(compiled)))
    }

    fn from_codeblock(
        realm: Realm,
        codeblock: Gc<CodeBlock>,
        source_text: SourceText,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            inner: Gc::new(Inner {
                realm,
                phase: GcRefCell::new(ScriptPhase::Codeblock(codeblock)),
                source_text,
                loaded_modules: GcRefCell::default(),
                host_defined: HostDefined::default(),
                path,
            }),
        }
    }

    /// Evaluates this script and returns its result.
    ///
    /// Note that this won't run any scheduled promise jobs; you need to call [`Context::run_jobs`]
    /// on the context or [`JobExecutor::run_jobs`] on the provided queue to run them.
    ///
    /// [`JobExecutor::run_jobs`]: crate::job::JobExecutor::run_jobs
    pub fn evaluate(&self, context: &mut Context) -> JsResult<JsValue> {
        self.prepare_run(context)?;
        let record = context.run();

        context.vm.pop_frame();

        record.consume()
    }

    /// Evaluates this script and returns its result, periodically yielding to the executor
    /// in order to avoid blocking the current thread.
    ///
    /// This uses an implementation defined amount of "clock cycles" that need to pass before
    /// execution is suspended. See [`Script::evaluate_async_with_budget`] if you want to also
    /// customize this parameter.
    #[allow(clippy::future_not_send)]
    pub async fn evaluate_async(&self, context: &mut Context) -> JsResult<JsValue> {
        self.evaluate_async_with_budget(context, 256).await
    }

    /// Evaluates this script and returns its result, yielding to the executor each time `budget`
    /// number of "clock cycles" pass.
    ///
    /// Note that "clock cycle" is in quotation marks because we can't determine exactly how many
    /// CPU clock cycles a VM instruction will take, but all instructions have a "cost" associated
    /// with them that depends on their individual complexity. We'd recommend benchmarking with
    /// different budget sizes in order to find the ideal yielding time for your application.
    #[allow(clippy::future_not_send)]
    pub async fn evaluate_async_with_budget(
        &self,
        context: &mut Context,
        budget: u32,
    ) -> JsResult<JsValue> {
        self.prepare_run(context)?;

        let record = context.run_async_with_budget(budget).await;

        context.vm.pop_frame();

        record.consume()
    }

    fn prepare_run(&self, context: &mut Context) -> JsResult<()> {
        let codeblock = self.codeblock(context)?;

        let global_env = EnvironmentStack::new();
        context.vm.push_frame_with_stack(
            CallFrame::new(
                codeblock.clone(),
                Some(ActiveRunnable::Script(self.clone())),
                global_env,
                self.inner.realm.clone(),
            )
            .with_env_fp(0)
            .with_flags(CallFrameFlags::EXIT_EARLY),
            JsValue::undefined(),
            JsValue::null(),
        );

        self.realm().resize_global_env();

        context
            .global_declaration_instantiation(&codeblock)
            .inspect_err(|_| {
                context.vm.pop_frame();
            })?;

        Ok(())
    }

    pub(super) fn path(&self) -> Option<&Path> {
        self.inner.path.as_deref()
    }

    pub(super) fn get_source(&self) -> SourceText {
        self.inner.source_text.clone()
    }
}

/// A script compiled once that can run in any realm on this thread.
///
/// Made by [`Script::parse_reusable`]. The compiled code refers to global
/// `let`/`const`/`class` bindings by slot, so it can be instantiated only in
/// a realm whose global lexical bindings are exactly those that existed when
/// it was compiled (for example a fresh realm that ran the same scripts
/// first). [`CompiledScript::instantiate`] checks this and returns `None`
/// otherwise; the caller then parses the source again.
#[derive(Clone, Trace, Finalize)]
pub struct CompiledScript {
    inner: Gc<CompiledInner>,
}

impl std::fmt::Debug for CompiledScript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledScript")
            .field("global_bindings_before", &self.inner.before.len())
            .field("global_bindings_after", &self.inner.after.len())
            .finish_non_exhaustive()
    }
}

#[derive(Trace, Finalize)]
struct CompiledInner {
    codeblock: Gc<CodeBlock>,
    source_text: SourceText,
    path: Option<PathBuf>,
    /// The global scope bindings the script was compiled against.
    #[unsafe_ignore_trace]
    before: ScopeLayout,
    /// The global scope bindings after adding the script's own declarations.
    #[unsafe_ignore_trace]
    after: ScopeLayout,
}

impl CompiledScript {
    /// Creates a [`Script`] for `realm` (or the current realm) that runs this
    /// compiled code, without parsing or compiling again.
    ///
    /// Returns `None` if the realm's global lexical bindings differ from the
    /// ones this script was compiled against. On success the script's own
    /// global declarations are added to the realm's global scope, as parsing
    /// would; global declaration instantiation still runs at evaluation.
    #[must_use]
    pub fn instantiate(&self, realm: Option<Realm>, context: &Context) -> Option<Script> {
        let realm = realm.unwrap_or_else(|| context.realm().clone());
        let scope = realm.scope();
        if !scope.has_layout(&self.inner.before) {
            return None;
        }
        scope.extend_from_layout(&self.inner.after, self.inner.before.len());
        Some(Script::from_codeblock(
            realm,
            self.inner.codeblock.clone(),
            self.inner.source_text.clone(),
            self.inner.path.clone(),
        ))
    }
}

/// Compiles a parsed script against the global scope `scope`.
///
/// Also returns whether annex B hoisted a block-level function to a global
/// `var`, which depends on the current realm's global object.
fn compile_script(
    source: &boa_ast::Script,
    scope: &Scope,
    source_text: SourceText,
    path: Option<&Path>,
    context: &mut Context,
) -> JsResult<(Gc<CodeBlock>, bool)> {
    let mut annex_b_function_names = Vec::new();

    global_declaration_instantiation_context(&mut annex_b_function_names, source, scope, context)?;
    let annex_b_hoisting = !annex_b_function_names.is_empty();

    let spanned_source_text = SpannedSourceText::new_source_only(source_text);

    let mut compiler = ByteCompiler::new(
        js_string!("<main>"),
        source.strict(),
        false,
        scope.clone(),
        scope.clone(),
        false,
        false,
        context.interner_mut(),
        false,
        spanned_source_text,
        path.map(Path::to_owned).into(),
    );

    #[cfg(feature = "annex-b")]
    {
        compiler.annex_b_function_names = annex_b_function_names;
    }

    compiler.global_declaration_instantiation(source);
    compiler.compile_statement_list(source.statements(), true, false);

    Ok((Gc::new(compiler.finish()), annex_b_hoisting))
}

/// Returns `true` if `script` calls `eval` directly anywhere, including
/// inside nested functions and classes.
fn contains_direct_eval(script: &boa_ast::Script) -> bool {
    struct DirectEvalFinder;

    impl<'ast> Visitor<'ast> for DirectEvalFinder {
        type BreakTy = ();

        fn visit_call(&mut self, node: &'ast Call) -> ControlFlow<Self::BreakTy> {
            if let Expression::Identifier(ident) = node.function().flatten()
                && ident.sym() == Sym::EVAL
            {
                return ControlFlow::Break(());
            }
            node.visit_with(self)
        }
    }

    script.visit_with(&mut DirectEvalFinder).is_break()
}
