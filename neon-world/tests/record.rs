use neon_protocol::{Dir, Registry};
use neon_world::record::*;
use neon_world::*;

/// A sink that folds packets into a world, like the view does.
struct WorldSink {
    reg: Registry,
    w: World,
    fed: Vec<(u64, Dir)>,
}

impl WorldSink {
    fn new() -> Self {
        WorldSink { reg: Registry::builtin().unwrap(), w: World::new(), fed: vec![] }
    }
}

impl Sink for WorldSink {
    fn reset(&mut self) {
        self.w = World::new();
        self.fed.clear();
    }
    fn feed(&mut self, rec: &Record) {
        self.fed.push((rec.t_ms, rec.dir));
        let e = match rec.dir {
            Dir::In => event_from_packet(&self.reg, &rec.plain),
            Dir::Out => event_from_out_packet(&self.reg, &rec.plain),
        };
        if let Some(e) = e {
            self.w.apply(&e, rec.t_ms);
        }
    }
}

fn session() -> Session {
    Session::from_lines(include_str!("fixtures/exp_session.log").lines().map(String::from))
}

#[test]
fn jsonl_roundtrip_keeps_direction_time_and_bytes() {
    let recs = vec![
        Record { t_ms: 0, dir: Dir::In, plain: vec![0x28, 0x00] },
        Record { t_ms: 1500, dir: Dir::Out, plain: vec![0xfe, 0x02, 0xaa, 0x00] },
    ];
    for r in &recs {
        assert_eq!(Record::parse_line(&r.to_line()).as_ref(), Some(r));
    }
    let s = Session { records: recs.clone(), bad_lines: 0 };
    let back = Session::from_lines(s.to_text().lines().map(String::from));
    assert_eq!(back.records, recs);
    assert_eq!(back.duration_ms(), 1500);
}

#[test]
fn legacy_stamped_logs_replay_as_incoming_with_relative_times() {
    let s = Session::from_lines(vec!["1793048300.500 op 0028 len 2: 28 00".to_string(), "1793048301.750 op 004f len 3: 4f 00 01".to_string()]);
    assert_eq!(s.records.len(), 2);
    assert!(s.records.iter().all(|r| r.dir == Dir::In));
    assert_eq!((s.records[0].t_ms, s.records[1].t_ms), (0, 1250));
}

#[test]
fn bad_lines_are_counted_and_order_is_by_time() {
    let s = Session::from_lines(vec![
        r#"{"t":50,"d":"in","hex":"2800"}"#.to_string(),
        "garbage".to_string(),
        r#"{"t":10,"d":"out","hex":"de00"}"#.to_string(),
        r#"{"t":10,"d":"in","hex":"zz"}"#.to_string(),
        r#"{"t":5,"d":"sideways","hex":"00"}"#.to_string(),
        "".to_string(),
    ]);
    assert_eq!(s.bad_lines, 3, "garbage, bad hex, unknown direction; the blank line is skipped");
    assert_eq!(s.records.iter().map(|r| r.t_ms).collect::<Vec<_>>(), vec![10, 50]);
}

#[test]
fn seeking_equals_playing_up_to_that_time() {
    let s = session();
    assert!(s.records.len() >= 10);
    let dur = s.duration_ms();
    let mid = dur / 2;
    // playing from the start in small steps
    let mut played = WorldSink::new();
    let mut r = Replayer::new(s.clone());
    r.playing = true;
    while r.pos_ms() < mid {
        r.tick(&mut played, 50);
    }
    let reached = r.pos_ms();
    // seeking straight there
    let mut sought = WorldSink::new();
    let mut r2 = Replayer::new(s.clone());
    r2.seek(&mut sought, reached);
    assert_eq!(played.w.snapshot(), sought.w.snapshot(), "scrubbing must give the same world as playing");
    assert_eq!(played.fed.len(), sought.fed.len());
    // seeking backwards starts over
    r2.seek(&mut sought, 0);
    assert!(sought.fed.len() <= 1);
    assert_eq!(sought.w.snapshot().exp_gained, 0);
    r2.seek(&mut sought, dur + 99_999); // past the end clamps
    assert_eq!(r2.pos_ms(), dur);
    assert!(r2.at_end());
}

#[test]
fn speed_pause_and_end_of_session() {
    let s = session();
    let dur = s.duration_ms();
    let mut sink = WorldSink::new();
    let mut r = Replayer::new(s);
    assert_eq!(r.tick(&mut sink, 1000), 0, "paused: nothing is fed");
    r.playing = true;
    r.speed = 1.0;
    r.tick(&mut sink, 1000);
    assert_eq!(r.pos_ms(), 1000);
    r.speed = 4.0;
    r.tick(&mut sink, 1000);
    assert_eq!(r.pos_ms(), 5000, "speed multiplies wall time");
    r.speed = 0.0;
    r.tick(&mut sink, 5000);
    assert_eq!(r.pos_ms(), 5000, "speed 0 holds position");
    r.speed = 1000.0;
    r.tick(&mut sink, dur + 1);
    assert!(r.at_end() && sink.fed.len() == r.session.records.len(), "everything fed exactly once");
    r.tick(&mut sink, 10);
    assert!(!r.playing, "playback stops at the end");
}

#[test]
fn outgoing_records_replay_without_touching_the_world() {
    let mut recs = session().records;
    let n_in = recs.len();
    // an outgoing packet that means nothing to the world (an undecoded opcode)
    recs.push(Record { t_ms: 100, dir: Dir::Out, plain: vec![0x07, 0x04, 1, 2, 3] });
    let s = Session::from_lines(Session { records: recs, bad_lines: 0 }.to_text().lines().map(String::from)); // sorted, as every loaded session is
    let mut a = WorldSink::new();
    Replayer::new(s.clone()).seek(&mut a, u64::MAX);
    let mut b = WorldSink::new();
    let only_in = Session { records: s.records.iter().filter(|r| r.dir == Dir::In).cloned().collect(), bad_lines: 0 };
    assert_eq!(only_in.records.len(), n_in);
    Replayer::new(only_in).seek(&mut b, u64::MAX);
    assert_eq!(a.w.snapshot(), b.w.snapshot());
    assert_eq!(a.fed.len(), b.fed.len() + 1, "the out record was delivered to the sink");
}

#[test]
fn our_own_position_comes_from_the_packets_we_send() {
    // A real walk (jib01 in Amherst, 2026-10-26): right for 2 s, left for 2 s, a jump, another jump while walking, then standing.
    // 17 outgoing 0x00db movement packets; no incoming packet is involved.
    let s = Session::from_lines(include_str!("fixtures/out_walk.jsonl").lines().map(String::from));
    assert_eq!(s.records.len(), 17);
    assert!(s.records.iter().all(|r| r.dir == Dir::Out));
    let mut sink = WorldSink::new();
    let mut r = Replayer::new(s.clone());
    // the walk right: x grows (415 at the start, the turning point 721 in the packet at 2.6 s)
    r.seek(&mut sink, 0);
    let x0 = sink.w.snapshot().me.x.unwrap();
    r.seek(&mut sink, 2600);
    let peak = sink.w.snapshot().me.x.unwrap();
    assert!(peak > x0 + 250, "walked right: {x0} -> {peak}");
    // the walk left: x shrinks again after the turning point (packets arrive every ~0.5 s, so allow for that)
    r.seek(&mut sink, 4600);
    let x_left = sink.w.snapshot().me.x.unwrap();
    assert!(x_left < peak - 250, "walked left: {peak} -> {x_left}");
    // the jump: y goes above the ground (smaller y) at some point, never below it
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for t in (0..s.duration_ms()).step_by(100) {
        r.seek(&mut sink, t);
        if let Some(y) = sink.w.snapshot().me.y {
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    assert!(min_y < 250 && max_y <= 274, "jump arc {min_y}..{max_y} on a ground at y = 274");
    // the end: exactly where the independent memory reader put the character a moment later
    r.seek(&mut sink, u64::MAX);
    let me = &sink.w.snapshot().me;
    assert_eq!((me.x, me.y), (Some(598), Some(274)));
}

#[test]
fn an_incoming_log_and_an_outgoing_capture_of_the_same_period_merge_by_time() {
    // legacy stamped incoming lines (unix seconds) and an .nsr outgoing file (absolute unix milliseconds), as neon-view --log a --log b
    let incoming = vec!["1793049935.000 op 0028 len 2: 28 00".to_string(), "1793049937.000 op 0028 len 2: 28 00".to_string()];
    let outgoing = vec![r#"{"t":1793049936500,"d":"out","hex":"de00"}"#.to_string(), r#"{"t":1793049934000,"d":"out","hex":"de00"}"#.to_string()];
    let s = Session::from_lines(incoming.into_iter().chain(outgoing));
    let order: Vec<(u64, Dir)> = s.records.iter().map(|r| (r.t_ms, r.dir)).collect();
    assert_eq!(order, vec![(0, Dir::Out), (1000, Dir::In), (2500, Dir::Out), (3000, Dir::In)]);
    assert_eq!(s.bad_lines, 0);
}

#[test]
fn a_real_party_exchange_shows_request_then_reply_and_the_world_follows() {
    // 12 real records (jib01, 2026-10-26): twice, a click on Create then on Leave. Outgoing 0x0184/0x0228 and the server's
    // incoming 0x00a7 replies, merged on one timeline.
    let reg = Registry::builtin().unwrap();
    let s = Session::from_lines(include_str!("fixtures/party_exchange.jsonl").lines().map(String::from));
    assert_eq!(s.records.len(), 12);
    let name_of = |r: &Record| reg.decode(r.dir, &r.plain).and_then(|d| d.name).unwrap_or_default();
    let reqs: Vec<&Record> = s.records.iter().filter(|r| r.dir == Dir::Out && name_of(r).ends_with("Request")).collect();
    assert_eq!(reqs.iter().map(|r| name_of(r)).collect::<Vec<_>>(), ["PartyCreateRequest", "PartyLeaveRequest", "PartyCreateRequest", "PartyLeaveRequest"]);
    for req in &reqs {
        let want = if name_of(req) == "PartyCreateRequest" { "PartyCreated" } else { "PartyLeft" };
        let reply = s.records.iter().find(|r| r.dir == Dir::In && name_of(r) == want && r.t_ms >= req.t_ms).unwrap_or_else(|| panic!("no {want} after {}", name_of(req)));
        assert!(reply.t_ms - req.t_ms <= 1500, "{want} came {} ms after {}", reply.t_ms - req.t_ms, name_of(req));
    }
    // the world's party follows the replies: one member after the first create, none after the leave
    let mut sink = WorldSink::new();
    let mut rp = Replayer::new(s.clone());
    let t_created = s.records.iter().find(|r| r.dir == Dir::In && name_of(r) == "PartyCreated").unwrap().t_ms;
    rp.seek(&mut sink, t_created);
    assert_eq!(sink.w.snapshot().party.len(), 1);
    rp.seek(&mut sink, u64::MAX);
    assert!(sink.w.snapshot().party.is_empty() && sink.w.snapshot().party_id.is_none());
}
