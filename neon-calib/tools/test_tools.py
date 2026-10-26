"""Self-tests for the analysis tools (run by scripts/test-headless.sh): python3 -I neon-calib/tools/test_tools.py"""
import json, os, sys, unittest
sys.path.insert(0, os.path.dirname(__file__))
import marklab

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

if __name__ == "__main__":
    unittest.main(verbosity=1)
