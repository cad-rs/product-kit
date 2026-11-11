//! When to ask the product for the latest entitlements, and what a change
//! means for the session already running.
//!
//! [`RefreshSchedule`] decides *when* a check should start, one at a time — at
//! startup, when the window comes back to the foreground (at most every 30
//! seconds), when who is signed in changes, every 30 minutes, when the user
//! asks, and for two minutes after coming back from a purchase page, because a
//! payment can take a moment to reach the product's server.
//!
//! [`SessionGrace`] is the other half: a capability lost mid-session stays
//! usable until the app closes, so a lapse never takes away what someone is
//! working in. That grace does not cover signing out, which must not hand one
//! user's plan to the next, or a plan the check at startup finds has lapsed.
//!
//! Both the workbench framework and the nanobot harness carry this module; it
//! replaced their two near-verbatim copies. Time comes in through
//! [`RefreshSchedule::tick`], so the rules are tested without waiting.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Routine checks catch expiries and changes made elsewhere.
const ROUTINE_EVERY: f32 = 30.0 * 60.0;
/// Coming back to the window checks at most this often.
const ACTIVATION_GAP: f32 = 30.0;
/// How long after a purchase page opens a return to the window counts as
/// coming back from it.
const PURCHASE_WAIT: f32 = 30.0 * 60.0;
/// Back from a purchase page, the payment may not have reached the product's
/// server yet, so check this often...
const POLL_EVERY: f32 = 4.0;
/// ...for this long.
const POLL_FOR: f32 = 120.0;

/// A product's answer to "may this feature be used?".
///
/// The grace wrapper only needs to tell an allowed answer from a refused one
/// and to construct the allowed answer for a cache hit, so the product's own
/// decision type (a `bool`, an enum with a reason, ...) plugs straight in.
pub trait EntitlementAnswer {
    /// Whether the feature may be used.
    fn is_allowed(&self) -> bool;
    /// The allowed answer, for when a settled feature is answered from cache.
    fn allowed() -> Self;
}

impl EntitlementAnswer for bool {
    fn is_allowed(&self) -> bool {
        *self
    }

    fn allowed() -> Self {
        true
    }
}

/// The minimal seam the grace wrapper needs from a product: a yes/no question
/// about one feature. Products implement this for their own provider (their
/// `Entitlements` / `AccountProvider` trait object), keeping the answer type.
pub trait EntitlementSource: Send + Sync {
    /// The product's decision type.
    type Answer: EntitlementAnswer;

    /// Ask the product, without the mid-session grace.
    fn check(&self, feature: &str) -> Self::Answer;
}

/// The product's answers, except that what the user could use stays usable
/// until the app closes: a plan lapsing mid-session must not close the view
/// they are working in. The next launch starts from the product's answers.
///
/// Wrap the product's provider in one and hand that to the runtime; call
/// [`settle`](Self::settle) when the first refresh finishes and
/// [`forget`](Self::forget) when who is signed in changes.
pub struct SessionGrace<S: EntitlementSource + ?Sized> {
    product: Arc<S>,
    kept: Mutex<Kept>,
}

#[derive(Default)]
struct Kept {
    /// False until a check has finished: the product may start from a cached
    /// plan that the first check finds has lapsed.
    settled: bool,
    features: HashSet<String>,
}

impl<S: EntitlementSource + ?Sized> SessionGrace<S> {
    pub fn new(product: Arc<S>) -> Self {
        Self {
            product,
            kept: Mutex::default(),
        }
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The latest answers are in; from here on what is allowed is kept.
    pub fn settle(&self) {
        self.kept().settled = true;
    }

    /// Someone else is signed in: nothing carries over, and their answers
    /// are not known until the next check finishes.
    pub fn forget(&self) {
        *self.kept() = Kept::default();
    }

    /// The product's answer, with the mid-session grace applied.
    pub fn check(&self, feature: &str) -> S::Answer {
        let mut kept = self.kept();
        if kept.features.contains(feature) {
            return S::Answer::allowed();
        }
        let answer = self.product.check(feature);
        if answer.is_allowed() && kept.settled {
            kept.features.insert(feature.to_string());
        }
        answer
    }

    /// The wrapped product, for the questions that are not about grace.
    pub fn product(&self) -> &Arc<S> {
        &self.product
    }
}

/// Each method that returns `bool` answers whether a check should start now.
/// Only one runs at a time.
#[derive(Default)]
pub struct RefreshSchedule {
    running: bool,
    /// Asked for during a running check, which may not have seen what changed.
    queued: bool,
    /// The user asked, so the last check of this round reports its outcome.
    report: bool,
    /// None until the first check, which is due at once.
    since_start: Option<f32>,
    purchase_wait: f32,
    polling: f32,
}

impl RefreshSchedule {
    fn elapsed(&self) -> f32 {
        self.since_start.unwrap_or(f32::INFINITY)
    }

    fn start(&mut self) -> bool {
        if self.running {
            return false;
        }
        self.running = true;
        self.since_start = Some(0.0);
        true
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let dt = dt.max(0.0);
        self.since_start = self.since_start.map(|elapsed| elapsed + dt);
        self.purchase_wait = (self.purchase_wait - dt).max(0.0);
        let poll_due = self.polling > 0.0 && self.elapsed() >= POLL_EVERY;
        self.polling = (self.polling - dt).max(0.0);
        (poll_due || self.elapsed() >= ROUTINE_EVERY) && self.start()
    }

    /// The window came back to the foreground.
    pub fn activated(&mut self) -> bool {
        if self.purchase_wait > 0.0 {
            self.polling = POLL_FOR;
            return self.start();
        }
        self.elapsed() >= ACTIVATION_GAP && self.start()
    }

    /// Something the answers depend on changed, such as who is signed in.
    pub fn asked(&mut self) -> bool {
        self.queued = self.running;
        self.start()
    }

    pub fn asked_by_user(&mut self) -> bool {
        self.report = true;
        self.asked()
    }

    pub fn purchase_page_opened(&mut self) {
        self.purchase_wait = PURCHASE_WAIT;
    }

    /// What the user was waiting for arrived.
    pub fn answers_changed(&mut self) {
        self.purchase_wait = 0.0;
        self.polling = 0.0;
    }

    /// The running check ended.
    pub fn finished(&mut self) -> bool {
        self.running = false;
        std::mem::take(&mut self.queued) && self.start()
    }

    /// Whether the check that just ended should tell the user how it went.
    /// Call after [`Self::finished`]: a queued check reports instead.
    pub fn take_report(&mut self) -> bool {
        !self.running && std::mem::take(&mut self.report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A schedule whose startup check has come and gone.
    fn started() -> RefreshSchedule {
        let mut schedule = RefreshSchedule::default();
        assert!(schedule.tick(0.0), "the first tick checks");
        assert!(!schedule.finished());
        schedule
    }

    /// Ticks one second at a time; how many checks started (each ends at once).
    fn run_for(schedule: &mut RefreshSchedule, seconds: u32) -> u32 {
        let mut checks = 0;
        for _ in 0..seconds {
            if schedule.tick(1.0) {
                checks += 1;
                schedule.finished();
            }
        }
        checks
    }

    #[test]
    fn routine_checks_are_half_an_hour_apart() {
        let mut schedule = started();
        assert_eq!(run_for(&mut schedule, 1799), 0);
        assert_eq!(run_for(&mut schedule, 1), 1);
    }

    #[test]
    fn only_one_check_runs_at_a_time() {
        let mut schedule = RefreshSchedule::default();
        assert!(schedule.tick(0.0));
        assert!(!schedule.tick(ROUTINE_EVERY), "the first is still running");
        assert!(!schedule.activated());
        schedule.finished();
        assert!(schedule.tick(ROUTINE_EVERY));
    }

    #[test]
    fn coming_back_to_the_window_checks_at_most_every_half_minute() {
        let mut schedule = started();
        assert!(!schedule.activated(), "just checked");
        run_for(&mut schedule, 30);
        assert!(schedule.activated());
        schedule.finished();
        assert!(!schedule.activated());
    }

    #[test]
    fn back_from_a_purchase_page_it_keeps_checking_for_two_minutes() {
        let mut schedule = started();
        schedule.purchase_page_opened();
        assert_eq!(run_for(&mut schedule, 60), 0, "nothing while the user is away");
        assert!(schedule.activated(), "a return is not held back by the gap");
        schedule.finished();
        assert_eq!(run_for(&mut schedule, 8), 2);
        assert_eq!(run_for(&mut schedule, 112), 28);
        assert_eq!(run_for(&mut schedule, 60), 0, "it gives up");
    }

    #[test]
    fn a_change_in_the_answers_ends_the_wait() {
        let mut schedule = started();
        schedule.purchase_page_opened();
        assert!(schedule.activated());
        schedule.finished();
        schedule.answers_changed();
        assert_eq!(run_for(&mut schedule, 60), 0);
        run_for(&mut schedule, 1);
        assert!(!schedule.purchase_wait_is_on());
    }

    #[test]
    fn a_later_return_within_the_wait_checks_again() {
        let mut schedule = started();
        schedule.purchase_page_opened();
        assert!(schedule.activated());
        schedule.finished();
        run_for(&mut schedule, 180);
        assert!(schedule.activated(), "they went back and finished paying");
    }

    #[test]
    fn the_wait_for_a_purchase_runs_out() {
        let mut schedule = started();
        schedule.purchase_page_opened();
        run_for(&mut schedule, 1800);
        assert!(!schedule.purchase_wait_is_on());
    }

    #[test]
    fn a_change_during_a_check_gets_a_check_of_its_own() {
        let mut schedule = RefreshSchedule::default();
        assert!(schedule.tick(0.0));
        assert!(!schedule.asked(), "one is running");
        assert!(schedule.finished(), "and another follows it");
        assert!(!schedule.finished(), "only one");
    }

    #[test]
    fn the_outcome_is_reported_once_by_the_last_check() {
        let mut schedule = RefreshSchedule::default();
        assert!(schedule.tick(0.0));
        assert!(!schedule.asked_by_user());
        assert!(schedule.finished());
        assert!(!schedule.take_report(), "the queued check saw the latest state");
        assert!(!schedule.finished());
        assert!(schedule.take_report());
        assert!(!schedule.take_report());
    }

    #[derive(Default)]
    struct Product(Mutex<Vec<&'static str>>);

    impl Product {
        fn sells(&self, features: &[&'static str]) {
            *self.0.lock().unwrap() = features.to_vec();
        }
    }

    impl EntitlementSource for Product {
        type Answer = bool;

        fn check(&self, feature: &str) -> bool {
            self.0.lock().unwrap().contains(&feature)
        }
    }

    fn session(features: &[&'static str]) -> (Arc<Product>, SessionGrace<Product>) {
        let product = Arc::new(Product::default());
        product.sells(features);
        (product.clone(), SessionGrace::new(product))
    }

    #[test]
    fn until_a_check_finishes_the_product_decides() {
        let (product, session) = session(&["pro"]);
        assert!(session.check("pro"));
        product.sells(&[]);
        assert!(!session.check("pro"), "the cached plan had lapsed");
    }

    #[test]
    fn once_settled_a_lost_feature_stays() {
        let (product, session) = session(&["pro"]);
        session.settle();
        assert!(session.check("pro"));
        product.sells(&[]);
        assert!(session.check("pro"));
    }

    #[test]
    fn a_feature_gained_later_applies_and_stays() {
        let (product, session) = session(&[]);
        session.settle();
        assert!(!session.check("pro"));
        product.sells(&["pro"]);
        assert!(session.check("pro"));
        product.sells(&[]);
        assert!(session.check("pro"));
    }

    #[test]
    fn a_feature_never_asked_about_while_allowed_is_not_kept() {
        let (product, session) = session(&["pro"]);
        session.settle();
        product.sells(&[]);
        assert!(!session.check("pro"));
    }

    #[test]
    fn the_next_user_starts_over() {
        let (product, session) = session(&["pro"]);
        session.settle();
        assert!(session.check("pro"));
        product.sells(&[]);
        session.forget();
        assert!(!session.check("pro"), "nothing carries over");
        product.sells(&["pro"]);
        assert!(session.check("pro"));
        product.sells(&[]);
        assert!(!session.check("pro"), "not kept until their check finishes");
    }

    impl RefreshSchedule {
        fn purchase_wait_is_on(&self) -> bool {
            self.purchase_wait > 0.0
        }
    }
}
