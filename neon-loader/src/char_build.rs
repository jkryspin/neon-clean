//! A character build: which stats the ability points of each level go to, which skills the skill points go to, and
//! which skills sit on quick slots and are used in fights. `builds/warrior.json` follows the meowdb Warrior guide
//! (meowdb.com/msclassic/guides/warrior-leveling-guide-1-30).
//!
//! Points are spent without knowing what was spent before: the unspent points are taken to be the most recent levels'
//! (the bot spends after every level-up), so the queue lists the newest level's points first, then the one before, and
//! the first `n` entries are what `n` unspent points are for.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stat {
    Str,
    Dex,
    Int,
    Luk,
}

/// AP split of every level in `from..=to`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ApRange {
    pub from: u16,
    pub to: u16,
    pub str: u32,
    pub dex: u32,
    pub int: u32,
    pub luk: u32,
}

/// The skill points of one level, in the order they are spent (a skill whose prerequisite comes later in the same
/// level is retried after the others).
#[derive(Debug, Clone, Deserialize)]
pub struct SpLevel {
    pub level: u16,
    pub points: Vec<(String, u32)>,
}

/// How a skill is used once it sits on a quick slot.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SkillUse {
    /// An attack used in place of the basic attack.
    Attack {
        #[serde(default)]
        cooldown_ms: u64,
        #[serde(default)]
        min_mp_pct: f64,
        #[serde(default)]
        min_hp_pct: f64,
        /// Only with at least this many mobs within `range_x` ahead (Slash Blast).
        #[serde(default)]
        min_mobs: u32,
        #[serde(default)]
        range_x: i32,
    },
    /// A buff pressed on a timer.
    Buff {
        every_ms: u64,
        #[serde(default)]
        min_mp_pct: f64,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillDef {
    /// Short name used in `sp` (e.g. "power_strike").
    pub name: String,
    /// The skill id in the client data (1001001 Power Strike).
    #[serde(default)]
    pub id: u32,
    /// Skill window tab: 0 Beginner, 1 first job.
    pub tab: u32,
    /// Row in that tab (skills are listed in id order).
    pub row: u32,
    /// Quick slot to put it on ("shift", "del", ... see shop::QUICKSLOTS).
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default, rename = "use")]
    pub usage: Option<SkillUse>,
}

/// A starting attack rotation suggestion for one skill, before a human tunes it into a build's `use`
/// (PLAN.md phase 2, "Rotation Builder"): an area skill that hits more than one mob at once is only
/// worth using over the basic attack/single-target skill once at least two mobs are in its range;
/// a single-target skill is always worth using once it is affordable. This does not replace a
/// hand-tuned build (e.g. `builds/warrior.json`'s guide-sourced numbers) — it is a reasonable
/// default for a skill nobody has written usage numbers for yet.
pub fn suggest_attack_usage(hits_multiple_mobs: bool, range_x: i32, cooldown_ms: u64) -> SkillUse {
    SkillUse::Attack {
        cooldown_ms,
        min_mp_pct: 10.0,
        min_hp_pct: 0.0,
        min_mobs: if hits_multiple_mobs { 2 } else { 1 },
        range_x: if hits_multiple_mobs { range_x } else { 0 },
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CharBuild {
    pub name: String,
    pub ap: Vec<ApRange>,
    pub sp: Vec<SpLevel>,
    pub skills: Vec<SkillDef>,
}

impl CharBuild {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let b: CharBuild = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        b.check()?;
        Ok(b)
    }

    /// Every skill named in `sp` must be defined.
    fn check(&self) -> Result<(), String> {
        for l in &self.sp {
            for (name, _) in &l.points {
                if self.skill(name).is_none() {
                    return Err(format!("build: level {} spends on unknown skill '{name}'", l.level));
                }
            }
        }
        Ok(())
    }

    pub fn skill(&self, name: &str) -> Option<&SkillDef> {
        self.skills.iter().find(|s| s.name == name)
    }

    fn ap_split(&self, level: u16) -> Vec<Stat> {
        let Some(r) = self.ap.iter().find(|r| (r.from..=r.to).contains(&level)) else { return vec![] };
        let mut v = vec![Stat::Str; r.str as usize];
        v.extend(std::iter::repeat(Stat::Dex).take(r.dex as usize));
        v.extend(std::iter::repeat(Stat::Int).take(r.int as usize));
        v.extend(std::iter::repeat(Stat::Luk).take(r.luk as usize));
        v
    }

    /// Where unspent AP goes at `level`: the newest level's split first, then older levels'.
    pub fn ap_queue(&self, level: u16) -> Vec<Stat> {
        (1..=level).rev().flat_map(|l| self.ap_split(l)).collect()
    }

    /// Unspent SP at `level` for the skills of tab `tab`, grouped by level, newest level first. Each entry is a skill
    /// name repeated once per point.
    pub fn sp_queue(&self, level: u16, tab: u32) -> Vec<(u16, Vec<String>)> {
        let mut levels: Vec<&SpLevel> = self.sp.iter().filter(|l| l.level <= level).collect();
        levels.sort_by_key(|l| std::cmp::Reverse(l.level));
        levels
            .into_iter()
            .map(|l| {
                let pts = l
                    .points
                    .iter()
                    .filter(|(n, _)| self.skill(n).is_some_and(|s| s.tab == tab))
                    .flat_map(|(n, k)| std::iter::repeat(n.clone()).take(*k as usize))
                    .collect::<Vec<_>>();
                (l.level, pts)
            })
            .filter(|(_, p)| !p.is_empty())
            .collect()
    }

    /// Skill levels the build has reached by `level`.
    pub fn skill_levels(&self, level: u16) -> BTreeMap<String, u32> {
        let mut m = BTreeMap::new();
        for l in self.sp.iter().filter(|l| l.level <= level) {
            for (n, k) in &l.points {
                *m.entry(n.clone()).or_insert(0) += k;
            }
        }
        m
    }

    /// Skills that should be on a quick slot by `level` (learnt and given a slot).
    pub fn slotted(&self, level: u16) -> Vec<&SkillDef> {
        let have = self.skill_levels(level);
        self.skills.iter().filter(|s| s.slot.is_some() && have.get(&s.name).copied().unwrap_or(0) > 0).collect()
    }
}

impl CharBuild {
    /// The fight settings for the skills now on quick slots: attacks (area attacks first, so one with enough mobs in
    /// range is chosen before a single-target one) and timed buffs.
    pub fn fight_cfg(&self, on_slots: &[String]) -> (Vec<crate::brain::SkillCfg>, Vec<crate::brain::MacroCfg>) {
        let (mut attacks, mut buffs) = (vec![], vec![]);
        for sk in self.skills.iter().filter(|s| on_slots.contains(&s.name)) {
            let Some(key) = sk.slot.as_deref().and_then(|n| crate::shop::QUICKSLOTS.iter().find(|(q, _, _)| *q == n)).map(|q| q.1.to_string()) else { continue };
            match &sk.usage {
                Some(SkillUse::Attack { cooldown_ms, min_mp_pct, min_hp_pct, min_mobs, range_x }) => attacks.push(crate::brain::SkillCfg {
                    label: format!("build:{}", sk.name),
                    key,
                    cooldown_ms: *cooldown_ms,
                    min_mp_pct: *min_mp_pct,
                    min_hp_pct: *min_hp_pct,
                    min_mobs: *min_mobs,
                    range_x: *range_x,
                    enabled: true,
                }),
                Some(SkillUse::Buff { every_ms, min_mp_pct }) => buffs.push(crate::brain::MacroCfg {
                    label: format!("build:{}", sk.name),
                    key,
                    every_ms: *every_ms,
                    min_mp_pct: *min_mp_pct,
                    only_when_fighting: false,
                    enabled: true,
                }),
                None => {}
            }
        }
        attacks.sort_by_key(|a| std::cmp::Reverse(a.min_mobs));
        (attacks, buffs)
    }
}

/// Unspent points of one kind. `known` is the server's count when the stream has one; without it every point the
/// window still offers is taken to belong to the newest levels.
pub fn take_known<T: Clone>(queue: &[T], known: Option<u32>) -> Vec<T> {
    match known {
        Some(n) => queue.iter().take(n as usize).cloned().collect(),
        None => queue.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_area_skill_waits_for_a_second_mob_a_single_target_skill_does_not() {
        assert_eq!(suggest_attack_usage(true, 130, 600), SkillUse::Attack { cooldown_ms: 600, min_mp_pct: 10.0, min_hp_pct: 0.0, min_mobs: 2, range_x: 130 });
        assert_eq!(suggest_attack_usage(false, 130, 600), SkillUse::Attack { cooldown_ms: 600, min_mp_pct: 10.0, min_hp_pct: 0.0, min_mobs: 1, range_x: 0 });
    }

    fn warrior() -> CharBuild {
        CharBuild::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../builds/warrior.json")).unwrap()
    }

    #[test]
    fn the_warrior_build_matches_the_guide_totals() {
        let b = warrior();
        // AP: 5 per level from 2; level 30 ends at 135 STR / 27 DEX from 12 / 5 at creation... the guide counts from
        // 4/4 base: what matters is the per-level split.
        let q = b.ap_queue(30);
        assert_eq!(q.len(), 29 * 5, "5 AP for each of levels 2..30");
        let dex = q.iter().filter(|s| **s == Stat::Dex).count();
        assert_eq!(dex, 3 + 5 + 5 + 4 + 5, "DEX at 15, 19, 20, 29 and 30");
        assert_eq!(&b.ap_queue(15)[..5], &[Stat::Str, Stat::Str, Stat::Dex, Stat::Dex, Stat::Dex], "level 15 is 2 STR / 3 DEX");
        // SP: 61 Warrior points from 10 to 30 reaching the guide's skill levels.
        let s = b.skill_levels(30);
        let want: BTreeMap<String, u32> = [("power_strike", 20), ("slash_blast", 19), ("precise_strikes", 15), ("hp_recovery", 3), ("max_hp", 3), ("iron_body", 1), ("three_snails", 3), ("recovery", 3), ("nimble_feet", 3)]
            .into_iter()
            .map(|(n, k)| (n.to_string(), k))
            .collect();
        assert_eq!(s, want);
        let warrior_sp: u32 = b.sp_queue(30, 1).iter().map(|(_, p)| p.len() as u32).sum();
        assert_eq!(warrior_sp, 1 + 20 * 3, "1 at the advancement, 3 per level after");
        assert_eq!(b.skill_levels(14).get("power_strike"), Some(&4));
        assert_eq!(b.skill_levels(19).get("power_strike"), Some(&19));
        assert_eq!(b.skill_levels(24).get("slash_blast"), Some(&18));
    }

    #[test]
    fn newest_levels_come_first() {
        let b = warrior();
        let q = b.sp_queue(12, 1);
        assert_eq!(q[0].0, 12);
        assert_eq!(q[0].1, vec!["power_strike", "slash_blast", "slash_blast"]);
        assert_eq!(q[1].0, 11);
        assert_eq!(take_known(&b.ap_queue(16), Some(5)), vec![Stat::Str; 5]);
        assert_eq!(take_known(&b.ap_queue(16), Some(7))[5..], [Stat::Str, Stat::Str]);
        // Beginner skill points stay on their own tab.
        assert!(b.sp_queue(9, 1).is_empty());
        assert_eq!(b.sp_queue(10, 0).iter().map(|(_, p)| p.len()).sum::<usize>(), 9);
    }

    #[test]
    fn slotted_skills_appear_once_learnt() {
        let b = warrior();
        assert!(b.slotted(9).iter().all(|s| s.tab == 0));
        let names: Vec<&str> = b.slotted(11).iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"power_strike") && names.contains(&"slash_blast"), "{names:?}");
        assert!(!b.slotted(29).iter().any(|s| s.name == "iron_body"));
        assert!(b.slotted(30).iter().any(|s| s.name == "iron_body"));
    }

    #[test]
    fn slotted_skills_become_attacks_and_buffs() {
        let b = warrior();
        let (atk, buffs) = b.fight_cfg(&["power_strike".into(), "slash_blast".into(), "iron_body".into()]);
        assert_eq!(atk.iter().map(|a| a.key.as_str()).collect::<Vec<_>>(), vec!["Delete", "Shift_L"], "Slash Blast (3+ mobs) is tried first");
        assert_eq!(atk[0].min_mobs, 3);
        assert_eq!(buffs.len(), 1);
        assert_eq!(buffs[0].key, "End");
    }

    #[test]
    fn unknown_skills_are_rejected() {
        let b: CharBuild = serde_json::from_str(r#"{"sp":[{"level":10,"points":[["nope",1]]}]}"#).unwrap();
        assert!(b.check().is_err());
    }
}
