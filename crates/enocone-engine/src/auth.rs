//! The sign-in state of one auth profile, shared by every flow of a run.
//!
//! A profile is in one of six states. At most one attempt (a sign-in or a
//! token refresh) is in flight per profile, and every use waiting on it gets
//! its one outcome. The state sits behind a short lock that is never held
//! across an `.await`; attempts run outside it.
//!
//! | State | A use that needs the token | A use that needs only the sign-in | Leaves for |
//! |---|---|---|---|
//! | `Unsigned` | starts the sign-in, then as `SigningIn` | the same | `SigningIn` |
//! | `SigningIn` | waits for the sign-in's outcome | the same | `SignedIn`, `SignInFailed`, or back to `Unsigned` if the attempt is dropped |
//! | `SignInFailed` | the sign-in's error, for the rest of the run | the same | (none) |
//! | `SignedIn` | the kept token, or starts a refresh when it is stale | the kept sign-in | `Refreshing` |
//! | `Refreshing` | waits for the refresh's outcome | the kept sign-in, at once | `SignedIn`, `CoolingDown`, or back to `SignedIn` if the attempt is dropped |
//! | `CoolingDown` | the refresh's error until the cooldown ends, then as `SignedIn` | the kept sign-in, at once | `Refreshing` once it ends |
//!
//! A use that needs only the sign-in (a browser signed in as the profile,
//! its cookies) therefore never waits on a token refresh, and never starts
//! one. A good attempt leaves `SignedIn`, clearing any earlier failure.
//!
//! Attempts are bounded by their runner: a refresh by the step timeout, a
//! sign-in by `SIGN_IN_STEPS` step timeouts, so every wait on one ends. A
//! sign-in run inside another profile's login counts against that login's
//! budget too. A use made from inside a login that would wait on an attempt
//! itself waiting, directly or through other logins, on that login is
//! refused at once, naming the chain.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::watch;
use tokio::time::Instant;

/// What signing in as a profile gave: headers for HTTP requests and, for a
/// browser login, the browser's saved storage.
#[derive(Clone)]
pub(crate) struct AuthState {
    pub headers: Vec<(String, String)>,
    pub storage: Option<Value>,
    /// When the token was read, for profiles with `refresh`.
    pub fetched: Instant,
}

/// How long a failed token refresh answers the uses that need the token
/// before the next one tries again.
pub(crate) const REFRESH_RETRY_AFTER: Duration = Duration::from_secs(30);

/// How many step timeouts a sign-in may take as a whole.
pub(crate) const SIGN_IN_STEPS: u32 = 6;

/// What a use of an auth profile needs from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Need {
    /// The current token: a stale one is refreshed first.
    Token,
    /// The kept sign-in (headers and browser storage), as it is: never refreshed.
    SignIn,
}

pub(crate) type Outcome = Result<AuthState, String>;

/// An attempt in flight: `None` until it ends, then its outcome. A closed
/// channel with no outcome means the attempt was dropped.
pub(crate) type Attempt = watch::Receiver<Option<Outcome>>;

enum Phase {
    Unsigned,
    SigningIn(Attempt),
    SignInFailed(String),
    SignedIn(AuthState),
    Refreshing { kept: AuthState, attempt: Attempt },
    CoolingDown { kept: AuthState, error: String, until: Instant },
}

/// What a use does next.
pub(crate) enum Next {
    /// Use this outcome.
    Answer(Outcome),
    /// Wait for this attempt's outcome, then decide again if it was dropped.
    Wait(Attempt),
    /// Sign in, and report the outcome through this lead.
    SignIn(Lead),
    /// Refresh the token of this sign-in, and report the outcome through this lead.
    Refresh(AuthState, Lead),
}

/// One profile's state.
pub(crate) struct ProfileAuth {
    phase: Mutex<Phase>,
}

impl Default for ProfileAuth {
    fn default() -> Self {
        ProfileAuth { phase: Mutex::new(Phase::Unsigned) }
    }
}

impl ProfileAuth {
    fn phase(&self) -> MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Decide what a use that needs `need` does now; `stale` says whether a
    /// kept token is due for a refresh. Starting an attempt moves the state
    /// on at once, so later uses wait for it instead of starting another.
    pub(crate) fn next(self: &Arc<Self>, need: Need, stale: impl Fn(&AuthState) -> bool) -> Next {
        let mut phase = self.phase();
        match &*phase {
            Phase::SigningIn(attempt) => Next::Wait(attempt.clone()),
            Phase::SignInFailed(error) => Next::Answer(Err(error.clone())),
            Phase::Refreshing { kept, attempt } => match need {
                Need::SignIn => Next::Answer(Ok(kept.clone())),
                Need::Token => Next::Wait(attempt.clone()),
            },
            Phase::CoolingDown { error, until, .. } if need == Need::Token && Instant::now() < *until => Next::Answer(Err(error.clone())),
            Phase::SignedIn(kept) | Phase::CoolingDown { kept, .. } if need == Need::SignIn || !stale(kept) => Next::Answer(Ok(kept.clone())),
            Phase::SignedIn(kept) | Phase::CoolingDown { kept, .. } => {
                let kept = kept.clone();
                let (lead, attempt) = Lead::start(self, Kind::Refresh(kept.clone()));
                *phase = Phase::Refreshing { kept: kept.clone(), attempt };
                Next::Refresh(kept, lead)
            }
            Phase::Unsigned => {
                let (lead, attempt) = Lead::start(self, Kind::SignIn);
                *phase = Phase::SigningIn(attempt);
                Next::SignIn(lead)
            }
        }
    }
}

enum Kind {
    SignIn,
    Refresh(AuthState),
}

/// The right and the duty to end one attempt. Dropped without an outcome
/// (its use was cancelled), it puts the state back as it was before the
/// attempt and closes the attempt, so the uses waiting on it decide again.
pub(crate) struct Lead {
    profile: Arc<ProfileAuth>,
    kind: Kind,
    outcome: Option<watch::Sender<Option<Outcome>>>,
}

impl Lead {
    fn start(profile: &Arc<ProfileAuth>, kind: Kind) -> (Lead, Attempt) {
        let (outcome, attempt) = watch::channel(None);
        (Lead { profile: Arc::clone(profile), kind, outcome: Some(outcome) }, attempt)
    }

    /// End the attempt: move the state on and hand `outcome` to every use waiting on it.
    pub(crate) fn finish(mut self, outcome: Outcome) -> Outcome {
        let next = match (&outcome, &self.kind) {
            (Ok(fresh), _) => Phase::SignedIn(fresh.clone()),
            (Err(error), Kind::SignIn) => Phase::SignInFailed(error.clone()),
            (Err(error), Kind::Refresh(kept)) => Phase::CoolingDown { kept: kept.clone(), error: error.clone(), until: Instant::now() + REFRESH_RETRY_AFTER },
        };
        *self.profile.phase() = next;
        if let Some(sender) = self.outcome.take() {
            sender.send_replace(Some(outcome.clone()));
        }
        outcome
    }
}

impl Drop for Lead {
    fn drop(&mut self) {
        if let Some(sender) = self.outcome.take() {
            *self.profile.phase() = match &self.kind {
                Kind::SignIn => Phase::Unsigned,
                Kind::Refresh(kept) => Phase::SignedIn(kept.clone()),
            };
            drop(sender);
        }
    }
}

/// The outcome of `attempt`, or `None` if it was dropped without one.
pub(crate) async fn outcome_of(mut attempt: Attempt) -> Option<Outcome> {
    attempt.wait_for(Option::is_some).await.ok().and_then(|outcome| outcome.clone())
}
