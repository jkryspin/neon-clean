use neon_cmd::*;
use neon_world::{Event, World};
use std::cell::RefCell;
use std::rc::Rc;

fn gate(now: u64, seed: u64) -> SafetyGate {
    SafetyGate::new(GateConfig::default(), now, seed)
}

#[test]
fn jitter_stays_within_25_percent_and_actually_varies() {
    let mut r = Rng::new(7);
    let xs: Vec<u64> = (0..2000).map(|_| r.jitter(1000, 0.25)).collect();
    assert!(xs.iter().all(|&x| (750..=1250).contains(&x)), "outside +-25%");
    let (lo, hi) = (xs.iter().min().unwrap(), xs.iter().max().unwrap());
    assert!(*lo < 800 && *hi > 1200, "no real spread: {lo}..{hi}");
    let mean = xs.iter().sum::<u64>() as f64 / xs.len() as f64;
    assert!((mean - 1000.0).abs() < 25.0, "biased: {mean}");
}

#[test]
fn commands_are_spaced_by_a_jittered_gap() {
    let mut g = gate(0, 1);
    assert_eq!(g.check(0), Verdict::Allow);
    g.record(0);
    assert!(matches!(g.check(10), Verdict::Wait(w) if w <= 150), "too soon must wait");
    // 120 ms +-25% = 90..150
    assert_eq!(g.check(151), Verdict::Allow);
}

#[test]
fn per_minute_cap_holds_back_a_burst() {
    let cfg = GateConfig { min_gap_ms: 0, jitter: 0.0, max_per_minute: 5, ..GateConfig::default() };
    let mut g = SafetyGate::new(cfg, 0, 1);
    for i in 0..5 {
        assert_eq!(g.check(i * 10), Verdict::Allow);
        g.record(i * 10);
    }
    assert!(matches!(g.check(100), Verdict::Wait(_)));
    assert_eq!(g.check(60_001), Verdict::Allow, "window slides");
}

#[test]
fn idle_breaks_come_every_45_to_90_minutes_and_last_30s_to_3min() {
    let mut g = gate(0, 3);
    let mut t = 0u64;
    let mut breaks: Vec<(u64, u64)> = Vec::new();
    while t < 5 * 3600 * 1000 {
        match g.check(t) {
            Verdict::Allow => {
                g.record(t);
                t += 1000;
            }
            Verdict::Wait(w) if g.on_break() => {
                breaks.push((t, w));
                t += w;
            }
            Verdict::Wait(w) => t += w,
            Verdict::Blocked(r) => panic!("{r}"),
        }
    }
    assert!(breaks.len() >= 3, "only {} breaks in 5 h", breaks.len());
    let mut prev_end = 0;
    for (start, len) in &breaks {
        assert!((30_000..=180_000).contains(len), "break length {len}");
        let gap_min = (start - prev_end) / 60_000;
        assert!((44..=91).contains(&gap_min), "break after {gap_min} min");
        prev_end = start + len;
    }
}

#[test]
fn session_ends_after_six_hours() {
    let mut g = gate(0, 5);
    assert!(matches!(g.check(6 * 3600 * 1000 - 1), Verdict::Allow | Verdict::Wait(_)));
    assert_eq!(g.check(6 * 3600 * 1000), Verdict::Blocked("session limit reached".into()));
}

#[test]
fn pauses_block_until_resumed() {
    let mut g = gate(0, 9);
    g.pause("gm");
    g.pause("lie_detector");
    assert!(matches!(g.check(1000), Verdict::Blocked(r) if r.contains("gm")));
    g.resume("gm");
    assert!(matches!(g.check(1000), Verdict::Blocked(r) if r.contains("lie_detector")));
    g.resume("lie_detector");
    assert_eq!(g.check(1000), Verdict::Allow);
}

#[test]
fn xtest_argv_for_each_command() {
    let a = XtestActuator::new(101, |_| Ok(()));
    assert_eq!(a.argv(&Command::Walk(Heading::Left))[0], ["xdotool", "keyup", "Right", "keydown", "Left"]);
    assert_eq!(a.argv(&Command::Walk(Heading::Right))[0], ["xdotool", "keyup", "Left", "keydown", "Right"]);
    assert_eq!(a.argv(&Command::Stop)[0], ["xdotool", "keyup", "Left", "Right"]);
    assert_eq!(a.argv(&Command::Attack)[0], ["xdotool", "key", "ctrl"]);
    assert_eq!(a.argv(&Command::Click { x: 5, y: 6 })[0], ["xdotool", "mousemove", "5", "6", "click", "1"]);
    let say = a.argv(&Command::Say("hi".into()));
    assert_eq!(say.len(), 3);
    assert_eq!(say[1], ["xdotool", "type", "--delay", "80", "hi"]);
}

fn executor(seed: u64) -> (Executor<XtestActuator<impl FnMut(&[String]) -> Result<(), String>>>, Rc<RefCell<Vec<Vec<String>>>>) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let l2 = log.clone();
    let act = XtestActuator::new(101, move |a: &[String]| {
        l2.borrow_mut().push(a.to_vec());
        Ok(())
    });
    let cfg = GateConfig { min_gap_ms: 0, jitter: 0.0, ..GateConfig::default() };
    (Executor::new(act, SafetyGate::new(cfg, 0, seed)), log)
}

#[test]
fn a_command_is_confirmed_when_the_world_shows_its_effect() {
    let (mut ex, log) = executor(1);
    let mut w = World::new();
    w.apply(&Event::DropSpawn { oid: 42, owner: 1, x: 0, y: 0 }, 0);
    let (s, _) = ex.submit(Command::Tap("z".into()), Expect::DropGone(42), 2000, 100);
    assert_eq!(s, Submit::Sent(1));
    assert_eq!(log.borrow().len(), 1);
    assert!(ex.tick(w.snapshot(), 500).is_empty(), "drop still there: nothing resolves yet");
    w.apply(&Event::DropRemove { oid: 42 }, 600);
    assert_eq!(ex.tick(w.snapshot(), 700), vec![(1, Outcome::Confirmed { after_ms: 600 })]);
    assert_eq!(ex.pending(), 0);
}

#[test]
fn a_key_that_did_nothing_times_out_instead_of_staying_silent() {
    let (mut ex, _) = executor(1);
    let w = World::new();
    ex.submit(Command::Tap("z".into()), Expect::MapIs(1020), 1000, 0);
    assert!(ex.tick(w.snapshot(), 999).is_empty());
    assert_eq!(ex.tick(w.snapshot(), 1000), vec![(1, Outcome::TimedOut)]);
}

#[test]
fn held_by_the_gate_means_nothing_is_sent() {
    let (mut ex, log) = executor(1);
    ex.gate.pause("gm");
    let (s, o) = ex.submit(Command::Attack, Expect::Nothing, 100, 0);
    assert!(matches!(s, Submit::Held(Verdict::Blocked(_))) && o.is_none());
    assert!(log.borrow().is_empty(), "no key may leave while paused");
}

#[test]
fn actuator_failure_is_reported() {
    let act = XtestActuator::new(101, |_: &[String]| Err("no display".to_string()));
    let cfg = GateConfig { min_gap_ms: 0, jitter: 0.0, ..GateConfig::default() };
    let mut ex = Executor::new(act, SafetyGate::new(cfg, 0, 1));
    let (_, o) = ex.submit(Command::Jump, Expect::Nothing, 100, 0);
    assert_eq!(o, Some((1, Outcome::Failed("no display".into()))));
    assert_eq!(ex.pending(), 0);
}

#[test]
fn staff_on_the_map_pauses_the_gate_and_leaving_resumes_it() {
    let mut rules = StaffRules::default();
    rules.names.insert("kevinsong".into());
    let mut g = gate(0, 1);
    let mut w = World::new();
    w.apply(&Event::PlayerSpawn { char_id: 1, name: "Gmorning".into() }, 0);
    rules.sync(&mut g, w.snapshot());
    assert_eq!(g.check(1000), Verdict::Allow, "'Gmorning' is not staff");
    w.apply(&Event::PlayerSpawn { char_id: 2, name: "KevinSong".into() }, 1);
    rules.sync(&mut g, w.snapshot());
    assert!(matches!(g.check(1000), Verdict::Blocked(r) if r.contains("gm")));
    w.apply(&Event::MapEnter { map: 5 }, 2); // map change clears the players
    rules.sync(&mut g, w.snapshot());
    assert_eq!(g.check(1000), Verdict::Allow);
    w.apply(&Event::PlayerSpawn { char_id: 3, name: "GM_Sera".into() }, 3);
    rules.sync(&mut g, w.snapshot());
    assert!(matches!(g.check(1000), Verdict::Blocked(_)));
}

#[test]
fn reconnect_backs_off_then_gives_up_and_never_hammers() {
    let mut p = ReconnectPolicy::default();
    assert_eq!(p.decide(0), Reconnect::Try);
    p.attempted(0);
    p.failed(0);
    assert_eq!(p.decide(1_000), Reconnect::Wait(59_000), "first retry after 1 min");
    assert_eq!(p.decide(60_000), Reconnect::Try);
    p.attempted(60_000);
    p.failed(60_000);
    assert_eq!(p.decide(61_000), Reconnect::Wait(299_000), "then 5 min");
    p.attempted(400_000);
    p.failed(400_000);
    assert_eq!(p.decide(401_000), Reconnect::Wait(899_000), "then 15 min");
    // a success resets the schedule
    p.succeeded();
    assert_eq!(p.decide(402_000), Reconnect::Try);
}

#[test]
fn reconnect_attempts_per_day_are_capped() {
    let mut p = ReconnectPolicy::default();
    p.backoff_ms = vec![0];
    for i in 0..6 {
        assert_eq!(p.decide(i * 1000), Reconnect::Try);
        p.attempted(i * 1000);
        p.failed(i * 1000);
    }
    assert!(matches!(p.decide(10_000), Reconnect::GiveUp(r) if r.contains("6 login attempts")));
    assert_eq!(p.decide(24 * 3600 * 1000 + 10_000), Reconnect::Try, "the window slides after a day");
}
