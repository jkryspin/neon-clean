//! Kami grinding: teleport-to-mob with targeting modes

use crate::movement::Position;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetingMode {
    Nearest,
    InRange(u16),
    Specific(Vec<u32>), // mob IDs
}

#[derive(Debug, Clone)]
pub struct Mob {
    pub id: u32,
    pub x: i16,
    pub y: i16,
    pub hp_percent: u8,
    pub is_boss: bool,
}

pub struct Kami {
    pub targeting: TargetingMode,
    pub pickup: bool,
    pub avoid_mobs: Vec<u32>,
}

impl Kami {
    pub fn new(targeting: TargetingMode) -> Self {
        Self {
            targeting,
            pickup: true,
            avoid_mobs: vec![],
        }
    }

    /// Find the best mob to target
    pub fn find_target<'a>(&self, mobs: &'a [Mob], player_pos: Position) -> Option<&'a Mob> {
        let mut best: Option<&Mob> = None;
        let mut best_dist = u32::MAX;

        for mob in mobs {
            // Skip avoided mobs
            if self.avoid_mobs.contains(&mob.id) {
                continue;
            }

            match &self.targeting {
                TargetingMode::Nearest => {
                    let dist = self.distance(player_pos, mob);
                    if dist < best_dist {
                        best_dist = dist;
                        best = Some(mob);
                    }
                }
                TargetingMode::InRange(max_range) => {
                    let dist = self.distance(player_pos, mob);
                    if dist <= *max_range as u32 && dist < best_dist {
                        best_dist = dist;
                        best = Some(mob);
                    }
                }
                TargetingMode::Specific(ids) => {
                    if ids.contains(&mob.id) {
                        let dist = self.distance(player_pos, mob);
                        if dist < best_dist {
                            best_dist = dist;
                            best = Some(mob);
                        }
                    }
                }
            }
        }

        best
    }

    fn distance(&self, player: Position, mob: &Mob) -> u32 {
        let dx = (player.x as i32 - mob.x as i32).abs();
        let dy = (player.y as i32 - mob.y as i32).abs();
        (dx * dx + dy * dy) as u32
    }
}
