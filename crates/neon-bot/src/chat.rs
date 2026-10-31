//! Chat parsing for Lie Detector and chat-based events

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum ChatType {
    System,
    Chat,
    Party,
    Guild,
    LieDetector,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub chat_type: ChatType,
    pub content: String,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LieDetectorAnswer {
    Left,
    Right,
}

pub struct ChatParser {
    messages: VecDeque<ChatMessage>,
    max_messages: usize,
    lie_detector_active: bool,
    lie_detector_question: Option<String>,
}

impl ChatParser {
    pub fn new() -> Self {
        Self {
            messages: VecDeque::with_capacity(100),
            max_messages: 100,
            lie_detector_active: false,
            lie_detector_question: None,
        }
    }

    /// Parse a chat message from memory (simplified - would read from actual chat buffer)
    pub fn parse_message(&mut self, content: &str) -> ChatMessage {
        let chat_type = self.detect_chat_type(content);
        let msg = ChatMessage {
            chat_type: chat_type.clone(),
            content: content.to_string(),
            timestamp: Instant::now(),
        };

        // Check for lie detector
        if chat_type == ChatType::LieDetector {
            self.lie_detector_active = true;
            self.lie_detector_question = Some(content.to_string());
        }

        self.messages.push_back(msg.clone());

        // Trim old messages
        while self.messages.len() > self.max_messages {
            self.messages.pop_front();
        }

        msg
    }

    fn detect_chat_type(&self, content: &str) -> ChatType {
        if content.contains("Lie Detector") || content.contains("Which way") {
            ChatType::LieDetector
        } else if content.contains("[System]") || content.contains("You have") {
            ChatType::System
        } else if content.contains("[Party]") {
            ChatType::Party
        } else if content.contains("[Guild]") {
            ChatType::Guild
        } else {
            ChatType::Chat
        }
    }

    /// Get the lie detector answer based on the question
    pub fn get_lie_detector_answer(&self) -> Option<LieDetectorAnswer> {
        if !self.lie_detector_active {
            return None;
        }

        if let Some(ref question) = self.lie_detector_question {
            // Simple heuristic: answer based on question content
            // In reality, this would need to detect which button is correct
            if question.to_lowercase().contains("left") {
                return Some(LieDetectorAnswer::Left);
            } else if question.to_lowercase().contains("right") {
                return Some(LieDetectorAnswer::Right);
            }
            // Default: alternate or random
            return Some(LieDetectorAnswer::Left);
        }

        None
    }

    /// Mark lie detector as answered
    pub fn mark_lie_detector_answered(&mut self) {
        self.lie_detector_active = false;
        self.lie_detector_question = None;
    }

    /// Check if lie detector is active
    pub fn is_lie_detector_active(&self) -> bool {
        self.lie_detector_active
    }

    /// Get recent messages
    pub fn get_recent_messages(&self, count: usize) -> Vec<&ChatMessage> {
        self.messages.iter().rev().take(count).collect()
    }

    /// Check if a level up message was received
    pub fn check_level_up(&self) -> bool {
        self.messages.iter()
            .rev()
            .take(10)
            .any(|msg| msg.content.contains("level") && msg.content.contains("up"))
    }

    /// Clear old messages (older than duration)
    pub fn clear_old_messages(&mut self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        while let Some(msg) = self.messages.front() {
            if msg.timestamp < cutoff {
                self.messages.pop_front();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lie_detector_detection() {
        let mut parser = ChatParser::new();
        parser.parse_message("Lie Detector: Which way should you go?");
        assert!(parser.is_lie_detector_active());
    }

    #[test]
    fn test_chat_type_detection() {
        let mut parser = ChatParser::new();
        let msg = parser.parse_message("[System] You have logged in");
        assert_eq!(msg.chat_type, ChatType::System);
    }
}
