//! Watch-mode session state.
//!
//! A new session is idle and auto-connect is off. The caller reports phone
//! attach and detach. This module does not open USB, listen on a socket, or sleep.

use crate::ipc::State;

/// One watch-mode session.
#[derive(Debug, PartialEq, Eq)]
pub struct Watch {
    state: State,
    auto_connect: bool,
}

impl Watch {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            auto_connect: false,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn auto_connect(&self) -> bool {
        self.auto_connect
    }

    pub fn enable(&mut self) {
        self.auto_connect = true;
        if self.state != State::Connected {
            self.state = State::Watching;
        }
    }

    pub fn disable(&mut self) {
        self.auto_connect = false;
        self.state = State::Idle;
    }

    pub fn phone_appeared(&mut self) {
        if self.state == State::Watching {
            self.state = State::Connected;
        }
    }

    pub fn phone_disappeared(&mut self) {
        if self.state != State::Connected {
            return;
        }
        self.state = if self.auto_connect {
            State::Watching
        } else {
            State::Disconnected
        };
    }

    pub fn stop(&mut self) {
        if self.state != State::Connected {
            return;
        }
        self.state = if self.auto_connect {
            State::Watching
        } else {
            State::Idle
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Connected with auto-connect off has no path through `enable`.
    /// The flag is set here so the off branches of disappear and stop can run.
    fn connected(auto_connect: bool) -> Watch {
        let mut watch = Watch::new();
        watch.enable();
        watch.phone_appeared();
        watch.auto_connect = auto_connect;
        watch
    }

    #[test]
    fn new_session_is_idle_with_auto_connect_off() {
        let watch = Watch::new();
        assert_eq!(watch.state(), State::Idle);
        assert!(!watch.auto_connect());
    }

    #[test]
    fn enable_turns_auto_connect_on() {
        let mut idle = Watch::new();
        idle.enable();
        assert_eq!(idle.state(), State::Watching);
        assert!(idle.auto_connect());

        let mut watching = Watch::new();
        watching.enable();
        watching.enable();
        assert_eq!(watching.state(), State::Watching);
        assert!(watching.auto_connect());

        let mut disconnected = connected(false);
        disconnected.phone_disappeared();
        assert_eq!(disconnected.state(), State::Disconnected);
        disconnected.enable();
        assert_eq!(disconnected.state(), State::Watching);
        assert!(disconnected.auto_connect());

        let mut session = connected(false);
        assert_eq!(session.state(), State::Connected);
        assert!(!session.auto_connect());
        session.enable();
        assert_eq!(session.state(), State::Connected);
        assert!(session.auto_connect());
    }

    #[test]
    fn disable_from_any_state_goes_idle() {
        let mut idle = Watch::new();
        idle.disable();
        assert_eq!(idle.state(), State::Idle);
        assert!(!idle.auto_connect());

        let mut watching = Watch::new();
        watching.enable();
        watching.disable();
        assert_eq!(watching.state(), State::Idle);
        assert!(!watching.auto_connect());

        let mut connected_on = connected(true);
        connected_on.disable();
        assert_eq!(connected_on.state(), State::Idle);
        assert!(!connected_on.auto_connect());

        let mut connected_off = connected(false);
        connected_off.disable();
        assert_eq!(connected_off.state(), State::Idle);
        assert!(!connected_off.auto_connect());

        let mut disconnected = connected(false);
        disconnected.phone_disappeared();
        disconnected.disable();
        assert_eq!(disconnected.state(), State::Idle);
        assert!(!disconnected.auto_connect());
    }

    #[test]
    fn phone_appearing_connects_only_while_watching() {
        let mut idle = Watch::new();
        idle.phone_appeared();
        assert_eq!(idle.state(), State::Idle);
        assert!(!idle.auto_connect());

        let mut watching = Watch::new();
        watching.enable();
        watching.phone_appeared();
        assert_eq!(watching.state(), State::Connected);
        assert!(watching.auto_connect());

        watching.phone_appeared();
        assert_eq!(watching.state(), State::Connected);
        assert!(watching.auto_connect());

        let mut disconnected = connected(false);
        disconnected.phone_disappeared();
        disconnected.phone_appeared();
        assert_eq!(disconnected.state(), State::Disconnected);
        assert!(!disconnected.auto_connect());
    }

    #[test]
    fn phone_disappearing_follows_auto_connect() {
        let mut stay = connected(true);
        stay.phone_disappeared();
        assert_eq!(stay.state(), State::Watching);
        assert!(stay.auto_connect());

        let mut leave = connected(false);
        leave.phone_disappeared();
        assert_eq!(leave.state(), State::Disconnected);
        assert!(!leave.auto_connect());

        let mut idle = Watch::new();
        idle.phone_disappeared();
        assert_eq!(idle.state(), State::Idle);

        let mut watching = Watch::new();
        watching.enable();
        watching.phone_disappeared();
        assert_eq!(watching.state(), State::Watching);
        assert!(watching.auto_connect());
    }

    #[test]
    fn stop_while_connected_follows_auto_connect() {
        let mut stay = connected(true);
        stay.stop();
        assert_eq!(stay.state(), State::Watching);
        assert!(stay.auto_connect());

        let mut leave = connected(false);
        leave.stop();
        assert_eq!(leave.state(), State::Idle);
        assert!(!leave.auto_connect());
    }

    #[test]
    fn stop_while_idle_or_watching_stays() {
        let mut idle = Watch::new();
        idle.stop();
        assert_eq!(idle.state(), State::Idle);
        assert!(!idle.auto_connect());

        let mut watching = Watch::new();
        watching.enable();
        watching.stop();
        assert_eq!(watching.state(), State::Watching);
        assert!(watching.auto_connect());

        let mut disconnected = connected(false);
        disconnected.phone_disappeared();
        disconnected.stop();
        assert_eq!(disconnected.state(), State::Disconnected);
        assert!(!disconnected.auto_connect());
    }

    #[test]
    fn state_returns_the_ipc_state() {
        let watch = Watch::new();
        assert_eq!(watch.state(), State::Idle);
        assert_eq!(watch.state().as_str(), "idle");

        let mut watching = Watch::new();
        watching.enable();
        assert_eq!(watching.state(), State::Watching);
        assert_eq!(watching.state().as_str(), "watching");

        let session = connected(true);
        assert_eq!(session.state(), State::Connected);
        assert_eq!(session.state().as_str(), "connected");

        let mut disconnected = connected(false);
        disconnected.phone_disappeared();
        assert_eq!(disconnected.state(), State::Disconnected);
        assert_eq!(disconnected.state().as_str(), "disconnected");
    }
}
