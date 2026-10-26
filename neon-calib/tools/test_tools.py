"""Self-tests for the analysis tools (run by scripts/test-headless.sh): python3 -I neon-calib/tools/test_tools.py"""
import json, os, sys, unittest
sys.path.insert(0, os.path.dirname(__file__))
import marklab
import wiremap

def pkt(t, d, op, n=4):
    return (t, d, op, n)

class MarkLab(unittest.TestCase):
    def test_an_opcode_that_follows_the_mark_is_found_and_background_noise_is_not(self):
        recs = []
        for i in range(600):                      # 10 minutes of background: opcode 0x029d every second, 0x0028 every 10 s
            recs.append(pkt(1000 + i, "in", 0x029d))
            if i % 10 == 0:
                recs.append(pkt(1000 + i, "in", 0x0028))
        recs += [pkt(1300.2, "out", 0x0184, 70), pkt(1300.4, "in", 0x00a7, 585)]   # the action at t=1300 s: a request and a reply
        recs += [pkt(1450.1, "out", 0x0184, 38), pkt(1450.3, "in", 0x00a7, 8)]     # a second action
        recs.sort()
        marks = [{"t": 1300_000, "label": "create"}, {"t": 1450_000, "label": "leave"}]
        res = marklab.analyse(marks, recs, window=10)
        cands = {label: {(d, op) for d, op, k, e, s in c} for label, t, c in res}
        self.assertIn(("out", 0x0184), cands["create"])
        self.assertIn(("in", 0x00a7), cands["create"])
        self.assertNotIn(("in", 0x029d), cands["create"], "a packet that arrives every second is not caused by the mark")
        self.assertNotIn(("in", 0x0028), cands["create"])
        self.assertIn(("in", 0x00a7), cands["leave"])
        self.assertEqual(len(res), 2)

    def test_no_marks_or_no_packets_is_not_an_error(self):
        self.assertEqual(marklab.analyse([], [pkt(1, "in", 1)]), [])
        self.assertEqual(marklab.analyse([{"t": 1000, "label": "x"}], []), [])

    def test_a_wire_file_is_outgoing(self):
        import tempfile
        d = tempfile.mkdtemp()
        w = os.path.join(d, "x.log.wire")
        open(w, "w").write("1793051150.500 op 8786 len 2: 86 87\n")
        self.assertEqual([(d_, op) for t, d_, op, n in marklab.load([w])], [("out", 0x8786)])

    def test_loading_both_log_formats(self):
        import tempfile
        d = tempfile.mkdtemp()
        a = os.path.join(d, "in.log"); b = os.path.join(d, "out.nsr")
        open(a, "w").write("1793051150.500 op 0028 len 2: 28 00\n")
        open(b, "w").write(json.dumps({"t": 1793051151000, "d": "out", "hex": "de00"}) + "\n")
        recs = marklab.load([a, b])
        self.assertEqual([(d_, op) for t, d_, op, n in recs], [("in", 0x28), ("out", 0xde)])

class WireMap(unittest.TestCase):
    def test_keepalive_is_the_two_byte_packet_that_answers_the_ping(self):
        pings = [(100.0 + 10 * i, 0x062e, bytes([0x2e, 0x06])) for i in range(12)]
        keep = [(100.0 + 10 * i + 0.03, 0x8786, bytes([0x86, 0x87])) for i in range(12)]
        noise = [(100.0 + 7 * i, 0x00ba, bytes([0xba, 0, 1])) for i in range(30)]          # 3 bytes: never a keepalive
        other2 = [(100.0 + 13 * i, 0x1111, bytes([0x11, 0x11])) for i in range(12)]          # 2 bytes but not tied to the pings
        self.assertEqual(wiremap.derive(pings, keep + noise + other2), {0x8786: 0x02f0})

    def test_nothing_is_guessed_without_pings(self):
        self.assertEqual(wiremap.derive([], [(1.0, 0x8786, bytes([0x86, 0x87]))] * 20), {})

    def test_attack_needs_the_fixture_bytes(self):
        fixture = bytes([0xe1, 0]) + bytes(range(2, 40))
        good = [(1.0 + i, 0x25d8, bytes([0xd8, 0x25]) + fixture[2:]) for i in range(6)]
        bad = [(1.0 + i, 0x3333, bytes([0x33, 0x33]) + bytes(38)) for i in range(6)]
        self.assertEqual(wiremap.derive([], good + bad, fixture), {0x25d8: 0x00e1})

if __name__ == "__main__":
    unittest.main(verbosity=1)
