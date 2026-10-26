//! Lie Detector - Solve anti-macro checks
//!
//! Feature 24: Automatically answer lie detector questions

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Copy)]
pub enum LieDetectorAnswer {
    Left,
    Right,
}

/// A decoded challenge. How it is read off the wire is not known yet (no live capture), so the
/// packet decoder that fills this in is a separate, still-missing piece.
#[derive(Debug, Clone, PartialEq)]
pub struct Challenge {
    pub image: Vec<u8>,
    pub options: Vec<String>,
    pub time_limit: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Text(String),
    Side(LieDetectorAnswer),
}

/// Produces an answer for a challenge, or `None` when it is not confident. Returning `None`
/// is always better than guessing: it hands the check to a person.
pub trait ChallengeSolver {
    fn solve(&self, challenge: &Challenge) -> Option<Answer>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum EscalateReason {
    AutoAnswerOff,
    NoChallengeData,
    Unsolvable,
    AttemptsExhausted,
    RunningOutOfTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LieDetectorAction {
    Idle,
    Wait(Duration),
    Submit(Answer),
    /// Stop automation and alert a person.
    Escalate(EscalateReason),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LieDetectorConfig {
    pub enabled: bool,
    pub auto_answer: bool,
    pub disable_during_check: bool, // Disable other features during lie detector
    pub disabled_features: Vec<String>, // Features to disable during check
    /// Wrong answers can be penalised, so the default is a single try.
    pub max_attempts: u32,
    /// Human-like pause before answering.
    pub min_delay: Duration,
    pub max_delay: Duration,
    /// Hand over to a person this long before the check expires.
    pub escalate_margin: Duration,
}

impl Default for LieDetectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_answer: true,
            disable_during_check: true,
            disabled_features: vec!["kami".to_string()],
            max_attempts: 1,
            min_delay: Duration::from_secs(4),
            max_delay: Duration::from_secs(12),
            escalate_margin: Duration::from_secs(15),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LieDetectorStats {
    pub checks_encountered: u32,
    pub checks_answered: u32,
    pub correct_answers: u32,
}

pub struct LieDetector {
    config: LieDetectorConfig,
    pub enabled: bool,
    stats: LieDetectorStats,
    is_active: bool,
    activation_time: Option<Instant>,
    features_disabled: bool,
    challenge: Option<Challenge>,
    attempts: u32,
    answer_delay: Option<Duration>,
}

impl LieDetector {
    pub fn new(config: LieDetectorConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: LieDetectorStats::default(),
            is_active: false,
            activation_time: None,
            features_disabled: false,
            challenge: None,
            attempts: 0,
            answer_delay: None,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(LieDetectorConfig::default())
    }

    pub fn enable(&mut self) {
        self.enabled = true;
        self.config.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.config.enabled = false;
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        self.config.enabled = self.enabled;
    }

    /// Lie detector appeared (no challenge data decoded yet; cannot be auto-solved).
    pub fn activate(&mut self) {
        self.begin(None, Instant::now());
    }

    /// Lie detector appeared with a decoded challenge.
    pub fn on_challenge(&mut self, challenge: Challenge, now: Instant) {
        self.begin(Some(challenge), now);
    }

    fn begin(&mut self, challenge: Option<Challenge>, now: Instant) {
        self.is_active = true;
        self.activation_time = Some(now);
        self.challenge = challenge;
        self.attempts = 0;
        self.answer_delay = None;
        self.stats.checks_encountered += 1;
        if self.config.disable_during_check {
            self.features_disabled = true;
        }
    }

    /// What the bot should do now. `solver` is consulted once per attempt; the answer is held
    /// back for a human-like delay (`jitter` in 0.0..1.0 picks a point in the configured range).
    pub fn next_action(
        &mut self,
        solver: &dyn ChallengeSolver,
        now: Instant,
        jitter: f64,
    ) -> LieDetectorAction {
        let Some(since) = self.activation_time.filter(|_| self.is_active) else {
            return LieDetectorAction::Idle;
        };
        if !self.config.auto_answer {
            return LieDetectorAction::Escalate(EscalateReason::AutoAnswerOff);
        }
        let elapsed = now.saturating_duration_since(since);
        let Some(challenge) = self.challenge.as_ref() else {
            return LieDetectorAction::Escalate(EscalateReason::NoChallengeData);
        };
        if self.attempts >= self.config.max_attempts {
            return LieDetectorAction::Escalate(EscalateReason::AttemptsExhausted);
        }
        // Leave a person time to step in before the check expires.
        let deadline = challenge.time_limit.saturating_sub(self.config.escalate_margin);
        if elapsed >= deadline {
            return LieDetectorAction::Escalate(EscalateReason::RunningOutOfTime);
        }
        let delay = *self.answer_delay.get_or_insert_with(|| {
            let (lo, hi) = (self.config.min_delay, self.config.max_delay.max(self.config.min_delay));
            lo + (hi - lo).mul_f64(jitter.clamp(0.0, 1.0))
        });
        if elapsed < delay {
            return LieDetectorAction::Wait(delay - elapsed);
        }
        match solver.solve(challenge) {
            Some(a) => {
                self.attempts += 1;
                LieDetectorAction::Submit(a)
            }
            None => LieDetectorAction::Escalate(EscalateReason::Unsolvable),
        }
    }

    /// The server's verdict on a submitted answer. A wrong answer keeps the check open for a
    /// retry only while attempts remain; otherwise `next_action` escalates.
    pub fn on_result(&mut self, success: bool) {
        if !self.is_active {
            return;
        }
        self.stats.checks_answered += 1;
        if success {
            self.stats.correct_answers += 1;
            self.finish();
        } else {
            self.answer_delay = None;
        }
    }

    /// A person (or the game) closed the check without us.
    pub fn dismiss(&mut self) {
        if self.is_active {
            self.finish();
        }
    }

    fn finish(&mut self) {
        self.is_active = false;
        self.activation_time = None;
        self.challenge = None;
        self.answer_delay = None;
        self.features_disabled = false;
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn features_disabled(&self) -> bool {
        self.features_disabled
    }

    pub fn get_stats(&self) -> LieDetectorStats {
        self.stats.clone()
    }

    pub fn time_active(&self) -> Option<std::time::Duration> {
        self.activation_time.map(|t| t.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Option<Answer>);
    impl ChallengeSolver for Fixed {
        fn solve(&self, _: &Challenge) -> Option<Answer> {
            self.0.clone()
        }
    }

    fn challenge(secs: u64) -> Challenge {
        Challenge { image: vec![1, 2, 3], options: vec![], time_limit: Duration::from_secs(secs) }
    }

    fn yes() -> Fixed {
        Fixed(Some(Answer::Text("abc".into())))
    }

    #[test]
    fn idle_when_no_check() {
        let mut ld = LieDetector::with_defaults();
        assert_eq!(ld.next_action(&yes(), Instant::now(), 0.5), LieDetectorAction::Idle);
    }

    #[test]
    fn waits_then_submits_after_human_delay() {
        let mut ld = LieDetector::with_defaults();
        let t0 = Instant::now();
        ld.on_challenge(challenge(60), t0);
        assert!(ld.features_disabled());
        // jitter 0.0 -> min_delay (4s)
        assert_eq!(ld.next_action(&yes(), t0, 0.0), LieDetectorAction::Wait(Duration::from_secs(4)));
        let later = t0 + Duration::from_secs(5);
        assert_eq!(
            ld.next_action(&yes(), later, 0.9),
            LieDetectorAction::Submit(Answer::Text("abc".into()))
        );
    }

    #[test]
    fn delay_is_fixed_once_chosen() {
        let mut ld = LieDetector::with_defaults();
        let t0 = Instant::now();
        ld.on_challenge(challenge(60), t0);
        ld.next_action(&yes(), t0, 1.0); // 12s
        let a = ld.next_action(&yes(), t0 + Duration::from_secs(2), 0.0);
        assert_eq!(a, LieDetectorAction::Wait(Duration::from_secs(10)));
    }

    #[test]
    fn success_clears_and_counts() {
        let mut ld = LieDetector::with_defaults();
        let t0 = Instant::now();
        ld.on_challenge(challenge(60), t0);
        ld.next_action(&yes(), t0 + Duration::from_secs(20), 0.0);
        ld.on_result(true);
        assert!(!ld.is_active() && !ld.features_disabled());
        let s = ld.get_stats();
        assert_eq!((s.checks_encountered, s.checks_answered, s.correct_answers), (1, 1, 1));
    }

    #[test]
    fn wrong_answer_escalates_with_single_attempt() {
        let mut ld = LieDetector::with_defaults();
        let t0 = Instant::now();
        ld.on_challenge(challenge(120), t0);
        let t1 = t0 + Duration::from_secs(20);
        assert!(matches!(ld.next_action(&yes(), t1, 0.0), LieDetectorAction::Submit(_)));
        ld.on_result(false);
        assert!(ld.is_active());
        assert_eq!(
            ld.next_action(&yes(), t1, 0.0),
            LieDetectorAction::Escalate(EscalateReason::AttemptsExhausted)
        );
        assert_eq!(ld.get_stats().correct_answers, 0);
    }

    #[test]
    fn unsure_solver_escalates_instead_of_guessing() {
        let mut ld = LieDetector::with_defaults();
        let t0 = Instant::now();
        ld.on_challenge(challenge(60), t0);
        assert_eq!(
            ld.next_action(&Fixed(None), t0 + Duration::from_secs(20), 0.0),
            LieDetectorAction::Escalate(EscalateReason::Unsolvable)
        );
    }

    #[test]
    fn escalates_before_time_runs_out() {
        let mut ld = LieDetector::with_defaults();
        let t0 = Instant::now();
        ld.on_challenge(challenge(30), t0); // deadline = 30 - 15 = 15s
        assert_eq!(
            ld.next_action(&yes(), t0 + Duration::from_secs(16), 0.0),
            LieDetectorAction::Escalate(EscalateReason::RunningOutOfTime)
        );
    }

    #[test]
    fn bare_activation_without_challenge_escalates() {
        let mut ld = LieDetector::with_defaults();
        ld.activate();
        assert_eq!(
            ld.next_action(&yes(), Instant::now(), 0.0),
            LieDetectorAction::Escalate(EscalateReason::NoChallengeData)
        );
    }

    #[test]
    fn auto_answer_off_escalates_and_dismiss_clears() {
        let mut cfg = LieDetectorConfig::default();
        cfg.auto_answer = false;
        let mut ld = LieDetector::new(cfg);
        let t0 = Instant::now();
        ld.on_challenge(challenge(60), t0);
        assert_eq!(
            ld.next_action(&yes(), t0, 0.0),
            LieDetectorAction::Escalate(EscalateReason::AutoAnswerOff)
        );
        ld.dismiss();
        assert!(!ld.is_active() && !ld.features_disabled());
    }
}
