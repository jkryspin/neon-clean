//! Auto Rune - Runes solve themselves
//!
//! Feature 26: Automatically solve rune puzzles

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum RuneType {
    Horizontal, // Left/Right
    Vertical,   // Up/Down
    Mixed,      // Any direction
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuneSolution {
    pub directions: Vec<Direction>, // Sequence of directions
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRuneConfig {
    pub enabled: bool,
    pub auto_solve: bool, // Auto solve when rune detected
    pub saved_solutions: std::collections::HashMap<u32, RuneSolution>, // Map ID -> solution
}

impl Default for AutoRuneConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_solve: true,
            saved_solutions: std::collections::HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoRuneStats {
    pub runes_solved: u32,
    pub manual_solves: u32,
    pub auto_solves: u32,
}

pub struct AutoRune {
    config: AutoRuneConfig,
    pub enabled: bool,
    stats: AutoRuneStats,
    current_solution: Vec<Direction>,
    solution_index: usize,
}

impl AutoRune {
    pub fn new(config: AutoRuneConfig) -> Self {
        Self {
            enabled: config.enabled,
            config,
            stats: AutoRuneStats::default(),
            current_solution: vec![],
            solution_index: 0,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(AutoRuneConfig::default())
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

    /// Save a solution for a map
    pub fn save_solution(&mut self, map_id: u32, solution: RuneSolution) {
        self.config.saved_solutions.insert(map_id, solution);
    }

    /// Get saved solution for a map
    pub fn get_solution(&self, map_id: u32) -> Option<&RuneSolution> {
        self.config.saved_solutions.get(&map_id)
    }

    /// Start solving a rune
    pub fn start_solving(&mut self, map_id: u32) -> bool {
        if let Some(solution) = self.get_solution(map_id) {
            self.current_solution = solution.directions.clone();
            self.solution_index = 0;
            return true;
        }
        false
    }

    /// Get next direction to move
    pub fn next_direction(&mut self) -> Option<Direction> {
        if self.solution_index < self.current_solution.len() {
            let dir = self.current_solution[self.solution_index].clone();
            self.solution_index += 1;
            Some(dir)
        } else {
            None
        }
    }

    /// Record a successful solve
    pub fn record_solve(&mut self, was_auto: bool) {
        self.stats.runes_solved += 1;
        if was_auto {
            self.stats.auto_solves += 1;
        } else {
            self.stats.manual_solves += 1;
        }
    }

    /// Add a direction to current manual solution
    pub fn add_direction(&mut self, direction: Direction) {
        self.current_solution.push(direction);
    }

    pub fn get_stats(&self) -> AutoRuneStats {
        self.stats.clone()
    }
}
