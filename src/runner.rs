use async_recursion::async_recursion;
use async_trait::async_trait;
use futures::FutureExt;
// TODO remove reference directly to the quickcheck types, used in public API
use quickcheck::{Arbitrary, Gen};
use std::fmt::Debug;
use std::future::Future;
use std::panic;
use std::pin::Pin;

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

    /// Produces a test result that instructs `quickcheck` to ignore it.
    /// This is useful for restricting the domain of your properties.
    /// When a test is discarded, `quickcheck` will replace it with a
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
        F: FnOnce() -> T,
        F: 'static,
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
                "[quickcheck] TEST FAILED. Arguments: ({})",
                self.arguments.join(", ")
            ),
            Some(ref err) => format!(
                "[quickcheck] TEST FAILED (runtime error). \
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
    async fn result(&self, _: &mut Gen) -> TestResult;
}

#[async_trait]
impl Testable for bool {
    async fn result(&self, _: &mut Gen) -> TestResult {
        TestResult::from_bool(*self)
    }
}

#[async_trait]
impl Testable for () {
    async fn result(&self, _: &mut Gen) -> TestResult {
        TestResult::passed()
    }
}

#[async_trait]
impl Testable for TestResult {
    async fn result(&self, _: &mut Gen) -> TestResult {
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
        g: &'l1 mut Gen,
    ) -> Pin<Box<dyn Future<Output = TestResult> + Send + 'at>>
    where
        'l0: 'at,
        'l1: 'at,
        Self: 'at,
    {
        match *self {
            Ok(ref r) => r.result(g),
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
impl<T, R, $($name: Arbitrary + Debug + Send + Sync),*> Testable for fn($($name),*) -> R
where
    T: Testable + Send + Sync,
    R: Future<Output = T> + 'static + Send,
{
    #[allow(non_snake_case)]
    async fn result(&self, g: &mut Gen) -> TestResult {
        #[async_recursion]
        async fn shrink_failure<
            T: Testable + Send + Sync,
            R,
            $($name: Arbitrary + Debug + Send + Sync),*
        >(
            g: &mut Gen,
            self_: fn($($name),*) -> R,
            a: ($($name,)*),
        ) -> Option<TestResult>
        where
            R: Future<Output = T> + 'static + Send,
        {
            let shrunk = a.shrink().collect::<Vec<_>>();
            for t in shrunk {
                let ($($name,)*) = t.clone();
                let mut r_new = safe_async(async move || self_($($name,)*).await)
                    .await
                    .result(g)
                    .await;
                if r_new.is_failure() {
                    {
                        let ($(ref $name,)*): ($($name,)*) = t;
                        r_new.arguments = debug_reprs(&[$($name),*]);
                    }

                    // The shrunk value *does* witness a failure, so keep
                    // trying to shrink it.
                    let shrunk = shrink_failure(g, self_, t).await;

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
        let mut r = safe_async(async move || self_($($name),*).await)
            .await
            .result(g)
            .await;

        {
            let ($(ref $name,)*) = a;
            r.arguments = debug_reprs(&[$($name),*]);
        }
        match r.status {
            Status::Pass | Status::Discard => r,
            Status::Fail => shrink_failure(g, self_, a).await.unwrap_or(r),
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
