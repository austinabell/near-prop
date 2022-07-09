use async_recursion::async_recursion;
use async_trait::async_trait;
use futures::FutureExt;
// TODO remove reference directly to the quickcheck types, used in public API
use quickcheck::{Arbitrary, Gen};
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::{cmp, env, panic};
use tokio::sync::mpsc;
use workspaces::network::{DevAccountDeployer, Sandbox};
use workspaces::{Contract, Worker};

/// The main NearProp type for setting configuration and running NearProp.
pub struct NearProp {
    tests: usize,
    gen_size: usize,
    max_tests: usize,
    min_tests_passed: usize,
}

fn qc_tests() -> usize {
    let default = 100;
    match env::var("NEARPROP_TESTS") {
        Ok(val) => val.parse().unwrap_or(default),
        Err(_) => default,
    }
}

fn qc_max_tests() -> usize {
    let default = 10_000;
    match env::var("NEARPROP_MAX_TESTS") {
        Ok(val) => val.parse().unwrap_or(default),
        Err(_) => default,
    }
}

fn qc_gen_size() -> usize {
    let default = 100;
    match env::var("NEARPROP_GENERATOR_SIZE") {
        Ok(val) => val.parse().unwrap_or(default),
        Err(_) => default,
    }
}

fn qc_min_tests_passed() -> usize {
    let default = 0;
    match env::var("NEARPROP_MIN_TESTS_PASSED") {
        Ok(val) => val.parse().unwrap_or(default),
        Err(_) => default,
    }
}

impl Default for NearProp {
    fn default() -> Self {
        let gen_size = qc_gen_size();
        let tests = qc_tests();
        let max_tests = cmp::max(tests, qc_max_tests());
        let min_tests_passed = qc_min_tests_passed();

        NearProp {
            tests,
            max_tests,
            min_tests_passed,
            gen_size,
        }
    }
}

impl NearProp {
    /// Set the random bytes buffer size.
    pub fn gen_size(mut self, size: usize) -> Self {
        self.gen_size = size;
        self
    }

    /// Set the number of tests to run.
    ///
    /// This actually refers to the maximum number of *passed* tests that
    /// can occur. Namely, if a test causes a failure, future testing on that
    /// property stops. Additionally, if tests are discarded, there may be
    /// fewer than `tests` passed.
    pub fn tests(mut self, tests: usize) -> Self {
        self.tests = tests;
        self
    }

    /// Set the maximum number of tests to run.
    ///
    /// The number of invocations of a property will never exceed this number.
    /// This is necessary to cap the number of tests because NearProp
    /// properties can discard tests.
    pub fn max_tests(mut self, max_tests: usize) -> Self {
        self.max_tests = max_tests;
        self
    }

    /// Set the minimum number of tests that needs to pass.
    ///
    /// This actually refers to the minimum number of *valid* *passed* tests
    /// that needs to pass for the property to be considered successful.
    pub fn min_tests_passed(mut self, min_tests_passed: usize) -> Self {
        self.min_tests_passed = min_tests_passed;
        self
    }

    /// Tests a property and returns the result.
    ///
    /// The result returned is either the number of tests passed or a witness
    /// of failure.
    async fn test_inner<A>(&mut self, f: A) -> Result<usize, TestResult>
    where
        A: Testable + Send + Sync,
    {
        // Compile code to avoid duplication across tasks.
        let wasm = Arc::new(workspaces::compile_project("./").await.unwrap());

        let (tx, mut rx) = mpsc::channel::<TestResult>(self.tests);
        let f = Arc::new(f);

        let mut tests_scheduled = 0;
        let spawn_test_check = || {
            let gen_size = self.gen_size;
            let sender = tx.clone();
            let func = Arc::clone(&f);
            let w_cloned = Arc::clone(&wasm);
            tokio::spawn(async move {
                let worker = workspaces::sandbox().await.unwrap();
                let contract = Arc::new(worker.dev_deploy(&w_cloned).await.unwrap());

                let ctx = PropContext { contract, worker };

                // New randomness `Gen` for every result execution. Internally this uses thread rng
                // so this should be roughly equivalent to re-using as quickcheck does.
                let result = func.result(ctx, &mut Gen::new(gen_size)).await;
                sender.send(result).await.expect("test result channel full");
            });
        };

        let mut passes_remaining = self.tests;
        // Spawn enough tasks to complete all tests
        for _ in 0..passes_remaining {
            spawn_test_check();
            tests_scheduled += 1;
        }

        // Poll channel of results until enough passes or max tests is hit
        while let Some(r) = rx.recv().await {
            match r {
                TestResult {
                    status: Status::Pass,
                    ..
                } => {
                    passes_remaining -= 1;
                    if passes_remaining == 0 {
                        // Sufficient amount of passes.
                        break;
                    }
                }
                TestResult {
                    status: Status::Discard,
                    ..
                } => {
                    if tests_scheduled >= self.max_tests {
                        // Max tests hit.
                        break;
                    }
                    spawn_test_check();
                    tests_scheduled += 1;
                }
                r @ TestResult {
                    status: Status::Fail,
                    ..
                } => return Err(r),
            }
        }

        Ok(self.tests - passes_remaining)
    }

    /// Tests a property and calls `panic!` on failure.
    ///
    /// The `panic!` message will include a (hopefully) minimal witness of
    /// failure.
    ///
    /// It is appropriate to use this method with Rust's unit testing
    /// infrastructure.
    ///
    /// Note that if the environment variable `RUST_LOG` is set to enable
    /// `info` level log messages for the `near_prop` crate, then this will
    /// include output on how many NearProp tests were passed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_prop::NearProp;
    /// use std::future::Future;
    ///
    /// async fn prop_reverse_reverse() {
    ///     async fn revrev(xs: Vec<usize>) -> bool {
    ///         let rev: Vec<_> = xs.clone().into_iter().rev().collect();
    ///         let revrev: Vec<_> = rev.into_iter().rev().collect();
    ///         xs == revrev
    ///     }
    ///     NearProp::default().test(revrev as fn(_) -> _).await;
    /// }
    /// ```
    pub async fn test<A>(&mut self, f: A)
    where
        A: Testable + Send + Sync,
    {
        // Ignore log init failures, implying it has already been done.
        let _ = crate::env_logger_init();

        let n_tests_passed = match self.test_inner(f).await {
            Ok(n_tests_passed) => n_tests_passed,
            Err(result) => panic!("{}", result.failed_msg()),
        };

        if n_tests_passed >= self.min_tests_passed {
            super::info!("(Passed {} NearProp tests.)", n_tests_passed)
        } else {
            panic!(
                "(Unable to generate enough tests, {} not discarded.)",
                n_tests_passed
            )
        }
    }
}

/// Convenience function for running NearProp.
///
/// This is an alias for `NearProp::default().test(f)`.
pub async fn prop_test<A: Testable + Send + Sync>(f: A) {
    NearProp::default().test(f).await
}

/// Context provided to the testable functions to generate results.
#[derive(Clone)]
pub struct PropContext {
    pub contract: Arc<Contract>,
    pub worker: Worker<Sandbox>,
}

/// Describes the status of a single instance of a test.
///
/// All testable things must be capable of producing a `TestResult`.
#[derive(Clone, Debug)]
pub struct TestResult {
    status: Status,
    arguments: Vec<String>,
    err: Option<String>,
}

/// Whether a test has passed, failed or been discarded.
#[derive(Clone, Debug)]
enum Status {
    Pass,
    Fail,
    Discard,
}

impl TestResult {
    /// Produces a test result that indicates the current test has passed.
    pub fn passed() -> TestResult {
        TestResult::from_bool(true)
    }

    /// Produces a test result that indicates the current test has failed.
    pub fn failed() -> TestResult {
        TestResult::from_bool(false)
    }

    /// Produces a test result that indicates failure from a runtime error.
    pub fn error<S: Into<String>>(msg: S) -> TestResult {
        let mut r = TestResult::from_bool(false);
        r.err = Some(msg.into());
        r
    }

    /// Produces a test result that instructs tests to ignore it.
    /// This is useful for restricting the domain of your properties.
    /// When a test is discarded, `near_prop` will replace it with a
    /// fresh one (up to a certain limit).
    pub fn discard() -> TestResult {
        TestResult {
            status: Status::Discard,
            arguments: vec![],
            err: None,
        }
    }

    /// Converts a `bool` to a `TestResult`. A `true` value indicates that
    /// the test has passed and a `false` value indicates that the test
    /// has failed.
    pub fn from_bool(b: bool) -> TestResult {
        TestResult {
            status: if b { Status::Pass } else { Status::Fail },
            arguments: vec![],
            err: None,
        }
    }

    /// Tests if a "procedure" fails when executed. The test passes only if
    /// `f` generates a task failure during its execution.
    pub fn must_fail<T, F>(f: F) -> TestResult
    where
        F: FnOnce() -> T + 'static,
        T: 'static,
    {
        let f = panic::AssertUnwindSafe(f);
        TestResult::from_bool(panic::catch_unwind(f).is_err())
    }

    /// Returns `true` if and only if this test result describes a failing
    /// test.
    pub fn is_failure(&self) -> bool {
        match self.status {
            Status::Fail => true,
            Status::Pass | Status::Discard => false,
        }
    }

    /// Returns `true` if and only if this test result describes a failing
    /// test as a result of a run time error.
    pub fn is_error(&self) -> bool {
        self.is_failure() && self.err.is_some()
    }

    fn failed_msg(&self) -> String {
        match self.err {
            None => format!(
                "[near_prop] TEST FAILED. Arguments: ({})",
                self.arguments.join(", ")
            ),
            Some(ref err) => format!(
                "[near_prop] TEST FAILED (runtime error). \
                 Arguments: ({})\nError: {}",
                self.arguments.join(", "),
                err
            ),
        }
    }
}

/// `Testable` describes types (e.g., a function) whose values can be
/// tested.
///
/// Anything that can be tested must be capable of producing a `TestResult`
/// given a random number generator. This is trivial for types like `bool`,
/// which are just converted to either a passing or failing test result.
///
/// For functions, an implementation must generate random arguments
/// and potentially shrink those arguments if they produce a failure.
///
/// It's unlikely that you'll have to implement this trait yourself.
#[async_trait]
pub trait Testable: 'static {
    // TODO experiment if possible to make context a reference. Seems possible but
    // TODO might not be possible with lifetimes.
    async fn result(&self, _: PropContext, _: &mut Gen) -> TestResult;
}

#[async_trait]
impl Testable for bool {
    async fn result(&self, _: PropContext, _: &mut Gen) -> TestResult {
        TestResult::from_bool(*self)
    }
}

#[async_trait]
impl Testable for () {
    async fn result(&self, _: PropContext, _: &mut Gen) -> TestResult {
        TestResult::passed()
    }
}

#[async_trait]
impl Testable for TestResult {
    async fn result(&self, _: PropContext, _: &mut Gen) -> TestResult {
        self.clone()
    }
}

impl<A, E> Testable for Result<A, E>
where
    A: Testable + Sync,
    E: Debug + Sync + 'static,
{
    fn result<'l0, 'l1, 'at>(
        &'l0 self,
        c: PropContext,
        g: &'l1 mut Gen,
    ) -> Pin<Box<dyn Future<Output = TestResult> + Send + 'at>>
    where
        'l0: 'at,
        'l1: 'at,
        Self: 'at,
    {
        match *self {
            Ok(ref r) => r.result(c, g),
            Err(ref err) => Box::pin(async move { TestResult::error(format!("{:?}", err)) }),
        }
    }
}

/// Return a vector of the debug formatting of each item in `args`
fn debug_reprs(args: &[&dyn Debug]) -> Vec<String> {
    args.iter().map(|x| format!("{:?}", x)).collect()
}

macro_rules! testable_fn {
    ($($name: ident),*) => {

#[async_trait]
impl<T, R, $($name: Arbitrary + Debug + Send + Sync),*> Testable for fn(PropContext, $($name),*) -> R
where
    T: Testable + Send + Sync,
    R: Future<Output = T> + 'static + Send,
{
    #[allow(non_snake_case)]
    async fn result(&self, ctx: PropContext, g: &mut Gen) -> TestResult {
        #[async_recursion]
        async fn shrink_failure<
            T: Testable + Send + Sync,
            R,
            $($name: Arbitrary + Debug + Send + Sync),*
        >(
            g: &mut Gen,
            self_: fn(PropContext, $($name),*) -> R,
            ctx: PropContext,
            a: ($($name,)*),
        ) -> Option<TestResult>
        where
            R: Future<Output = T> + 'static + Send,
        {
            let shrunk = a.shrink().collect::<Vec<_>>();
            for t in shrunk {
                let ($($name,)*) = t.clone();
                // FIXME: clear contract storage when shrinking data for failures.
                let c_cloned = ctx.clone();
                let mut r_new = safe_async(async move || self_(c_cloned, $($name,)*).await)
                    .await
                    .result(ctx.clone(), g)
                    .await;
                if r_new.is_failure() {
                    {
                        let ($(ref $name,)*): ($($name,)*) = t;
                        r_new.arguments = debug_reprs(&[$($name),*]);
                    }

                    // The shrunk value *does* witness a failure, so keep
                    // trying to shrink it.
                    let shrunk = shrink_failure(g, self_, ctx, t).await;

                    // If we couldn't witness a failure on any shrunk value,
                    // then return the failure we already have.
                    return Some(shrunk.unwrap_or(r_new));
                }
            }
            None
        }

        let self_ = *self;
        let a: ($($name,)*) = Arbitrary::arbitrary(g);
        let ($($name,)*) = a.clone();
        let c_cloned = ctx.clone();
        let mut r = safe_async(async move || self_(c_cloned, $($name),*).await)
            .await
            .result(ctx.clone(), g)
            .await;

        {
            let ($(ref $name,)*) = a;
            r.arguments = debug_reprs(&[$($name),*]);
        }
        match r.status {
            Status::Pass | Status::Discard => r,
            Status::Fail => shrink_failure(g, self_, ctx, a).await.unwrap_or(r),
        }
    }
}
}}

testable_fn!();
testable_fn!(A);
testable_fn!(A, B);
testable_fn!(A, B, C);
testable_fn!(A, B, C, D);
testable_fn!(A, B, C, D, E);
testable_fn!(A, B, C, D, E, F);
testable_fn!(A, B, C, D, E, F, G);
testable_fn!(A, B, C, D, E, F, G, H);

async fn safe_async<T, F, R>(fun: F) -> Result<T, String>
where
    F: FnOnce() -> R + 'static,
    T: 'static,
    R: Future<Output = T>,
{
    panic::AssertUnwindSafe(async { panic::AssertUnwindSafe(fun)().await })
        .catch_unwind()
        .await
        .map_err(|any_err| {
            // Extract common types of panic payload:
            // panic and assert produce &str or String
            if let Some(&s) = any_err.downcast_ref::<&str>() {
                s.to_owned()
            } else if let Some(s) = any_err.downcast_ref::<String>() {
                s.to_owned()
            } else {
                "UNABLE TO SHOW RESULT OF PANIC.".to_owned()
            }
        })
}
