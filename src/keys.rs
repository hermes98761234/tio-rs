/// Ctrl-t (0x14) prefix state machine.
///
/// In interactive mode, ctrl-t is the prefix key. After pressing ctrl-t,
/// the next character determines the action. ctrl-t ctrl-t sends literal 0x14.
/// Actions returned by the ctrl-t state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TtyAction {
    /// No action yet (need more input).
    None,
    /// Quit the session.
    Quit,
    /// Send a break signal.
    SendBreak,
    /// Show current configuration.
    ShowConfig,
    /// Toggle local echo.
    ToggleEcho,
    /// Toggle timestamps.
    ToggleTimestamps,
    /// Toggle input hex mode.
    ToggleInputHex,
    /// Toggle output hex mode.
    ToggleOutputHex,
    /// Clear screen.
    ClearScreen,
    /// Prompt for DTR/RTS toggle.
    PromptSignal,
    /// Show RX/TX stats.
    ShowStats,
    /// Toggle logging.
    ToggleLogging,
    /// Flush buffers.
    FlushBuffers,
    /// Show version.
    ShowVersion,
    /// Send literal ctrl-t (0x14).
    LiteralCtrlT,
    /// Show help.
    Help,
    /// Pass through the byte (not a ctrl-t command).
    Pass(u8),
}

/// State of the ctrl-t prefix machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CtrlTState {
    /// Normal state — not expecting a ctrl-t suffix.
    Idle,
    /// ctrl-t was received, waiting for the next byte.
    Prefix,
}

/// The ctrl-t state machine.
pub struct CtrlTStateMachine {
    state: CtrlTState,
}

impl CtrlTStateMachine {
    pub fn new() -> Self {
        Self {
            state: CtrlTState::Idle,
        }
    }

    /// Reset to idle state.
    pub fn reset(&mut self) {
        self.state = CtrlTState::Idle;
    }

    /// Process a byte from stdin. Returns an action if one is determined.
    pub fn feed(&mut self, byte: u8) -> TtyAction {
        match self.state {
            CtrlTState::Idle => {
                if byte == 0x14 {
                    // ctrl-t
                    self.state = CtrlTState::Prefix;
                    TtyAction::None
                } else {
                    TtyAction::Pass(byte)
                }
            }
            CtrlTState::Prefix => {
                self.state = CtrlTState::Idle;
                match byte {
                    b'?' => TtyAction::Help,
                    b'q' => TtyAction::Quit,
                    b'b' => TtyAction::SendBreak,
                    b'c' => TtyAction::ShowConfig,
                    b'e' => TtyAction::ToggleEcho,
                    b't' => TtyAction::ToggleTimestamps,
                    b'i' => TtyAction::ToggleInputHex,
                    b'o' => TtyAction::ToggleOutputHex,
                    b'l' => TtyAction::ClearScreen,
                    b'g' => TtyAction::PromptSignal,
                    b's' => TtyAction::ShowStats,
                    b'f' => TtyAction::ToggleLogging,
                    b'F' => TtyAction::FlushBuffers,
                    b'v' => TtyAction::ShowVersion,
                    0x14 => TtyAction::LiteralCtrlT,
                    // Unknown ctrl-t command — ignore
                    _ => TtyAction::None,
                }
            }
        }
    }

    /// Return true if we're in the prefix state (waiting for the next byte).
    pub fn in_prefix(&self) -> bool {
        self.state == CtrlTState::Prefix
    }
}

impl Default for CtrlTStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idle_passthrough() {
        let mut sm = CtrlTStateMachine::new();
        assert_eq!(sm.feed(b'A'), TtyAction::Pass(b'A'));
        assert_eq!(sm.feed(b'z'), TtyAction::Pass(b'z'));
        assert_eq!(sm.feed(b'\n'), TtyAction::Pass(b'\n'));
    }

    #[test]
    fn test_ctrl_t_prefix() {
        let mut sm = CtrlTStateMachine::new();
        assert_eq!(sm.feed(0x14), TtyAction::None);
        assert!(sm.in_prefix());
    }

    #[test]
    fn test_ctrl_t_q() {
        let mut sm = CtrlTStateMachine::new();
        assert_eq!(sm.feed(0x14), TtyAction::None);
        assert_eq!(sm.feed(b'q'), TtyAction::Quit);
        assert!(!sm.in_prefix());
    }

    #[test]
    fn test_ctrl_t_b() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'b'), TtyAction::SendBreak);
    }

    #[test]
    fn test_ctrl_t_c() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'c'), TtyAction::ShowConfig);
    }

    #[test]
    fn test_ctrl_t_e() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'e'), TtyAction::ToggleEcho);
    }

    #[test]
    fn test_ctrl_t_t() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b't'), TtyAction::ToggleTimestamps);
    }

    #[test]
    fn test_ctrl_t_i() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'i'), TtyAction::ToggleInputHex);
    }

    #[test]
    fn test_ctrl_t_o() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'o'), TtyAction::ToggleOutputHex);
    }

    #[test]
    fn test_ctrl_t_l() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'l'), TtyAction::ClearScreen);
    }

    #[test]
    fn test_ctrl_t_g() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'g'), TtyAction::PromptSignal);
    }

    #[test]
    fn test_ctrl_t_s() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b's'), TtyAction::ShowStats);
    }

    #[test]
    fn test_ctrl_t_f() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'f'), TtyAction::ToggleLogging);
    }

    #[test]
    fn test_ctrl_t_upper_f() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'F'), TtyAction::FlushBuffers);
    }

    #[test]
    fn test_ctrl_t_v() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'v'), TtyAction::ShowVersion);
    }

    #[test]
    fn test_ctrl_t_ctrl_t() {
        let mut sm = CtrlTStateMachine::new();
        assert_eq!(sm.feed(0x14), TtyAction::None);
        assert_eq!(sm.feed(0x14), TtyAction::LiteralCtrlT);
    }

    #[test]
    fn test_ctrl_t_question() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'?'), TtyAction::Help);
    }

    #[test]
    fn test_ctrl_t_unknown() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert_eq!(sm.feed(b'x'), TtyAction::None);
    }

    #[test]
    fn test_reset() {
        let mut sm = CtrlTStateMachine::new();
        sm.feed(0x14);
        assert!(sm.in_prefix());
        sm.reset();
        assert!(!sm.in_prefix());
        // After reset, bytes pass through
        assert_eq!(sm.feed(0x14), TtyAction::None);
        assert!(sm.in_prefix());
    }

    #[test]
    fn test_multiple_sequences() {
        let mut sm = CtrlTStateMachine::new();
        // First sequence: ctrl-t e (toggle echo)
        assert_eq!(sm.feed(0x14), TtyAction::None);
        assert_eq!(sm.feed(b'e'), TtyAction::ToggleEcho);
        // Second sequence: ctrl-t q (quit)
        assert_eq!(sm.feed(0x14), TtyAction::None);
        assert_eq!(sm.feed(b'q'), TtyAction::Quit);
        // Passthrough
        assert_eq!(sm.feed(b'X'), TtyAction::Pass(b'X'));
    }
}
