#!/usr/bin/env python3
"""Extract quest definitions from the client's QuestData wz into neon-calib/quests.json.

  wzquests.py <QuestData_000.wz> <out.json>

Per quest: name, start/complete NPC, level range, jobs, prerequisite quests, kill and item
requirements for completion, and rewards (money, exp, items, next quest).
"""
import json, sys, os
sys.path.insert(0, os.path.dirname(__file__))
import wzmap


def lst(d):
    return [d[k] for k in sorted(d, key=lambda x: int(x))] if isinstance(d, dict) else []


def phase(check, act):
    c, a = check or {}, act or {}
    return {
        'npc': c.get('npc'), 'lvmin': c.get('lvmin'), 'lvmax': c.get('lvmax'),
        'jobs': [j for j in lst(c.get('job')) if isinstance(j, int)],
        'quests': [{'id': q.get('id'), 'state': q.get('state')} for q in lst(c.get('quest')) if isinstance(q, dict)],
        'mobs': [{'id': m.get('id'), 'count': m.get('count')} for m in lst(c.get('mob')) if isinstance(m, dict)],
        'items': [{'id': i.get('id'), 'count': i.get('count')} for i in lst(c.get('item')) if isinstance(i, dict)],
        'money': a.get('money'), 'exp': a.get('exp'), 'next': a.get('nextQuest'),
        'rewards': [{'id': i.get('id'), 'count': i.get('count')} for i in lst(a.get('item')) if isinstance(i, dict)],
        'end_date': c.get('end'), 'interval': c.get('interval'),
    }


def main():
    wz, out = sys.argv[1:3]
    r, imgs = wzmap.open_wz(wz)
    quests = {}
    for path, off, _ in imgs:
        qid = path[:-4]
        try: img = wzmap.parse_img(r, off)
        except Exception: continue
        info = img.get('QuestInfo') or {}
        chk, act = img.get('Check') or {}, img.get('Act') or {}
        quests[qid] = {
            'name': info.get('name'), 'parent': info.get('parent'), 'area': info.get('area'),
            'auto_start': bool(info.get('autoStart')), 'blocked': bool(info.get('blocked')),
            'start': phase(chk.get('0'), act.get('0')), 'end': phase(chk.get('1'), act.get('1')),
        }
    json.dump(quests, open(out, 'w'), separators=(',', ':'))
    print('wrote', len(quests), 'quests')


if __name__ == '__main__':
    main()
