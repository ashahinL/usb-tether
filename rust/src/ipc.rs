//! Newline-delimited JSON the menu bar already speaks.
//! This module formats and parses those lines. It does not open a socket.

/// Path a later daemon listener will bind. Nothing in this module binds it.
pub const SOCK_PATH: &str = "/tmp/tetherd.sock";

/// Daemon state names the menu bar accepts.
/// `connecting` is not one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Watching,
    Connected,
    Disconnected,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Watching => "watching",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
        }
    }

    /// `connecting` and any other name return `None`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "idle" => Some(Self::Idle),
            "watching" => Some(Self::Watching),
            "connected" => Some(Self::Connected),
            "disconnected" => Some(Self::Disconnected),
            _ => None,
        }
    }
}

/// Counters the daemon sends once a second. Mbps values print with two decimal places.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub tx_mbps: f64,
    pub rx_mbps: f64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_pkts: u64,
    pub rx_pkts: u64,
}

/// One command from the menu bar. `status` is not a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Stop,
    Disable,
    Enable,
}

/// One stats object plus a newline.
pub fn format_stats(stats: &Stats) -> String {
    format!(
        "{{\"type\":\"stats\",\"tx_mbps\":{:.2},\"rx_mbps\":{:.2},\"tx_bytes\":{},\"rx_bytes\":{},\"tx_pkts\":{},\"rx_pkts\":{}}}\n",
        stats.tx_mbps,
        stats.rx_mbps,
        stats.tx_bytes,
        stats.rx_bytes,
        stats.tx_pkts,
        stats.rx_pkts,
    )
}

/// One state object plus a newline.
/// `address` is `(ip, iface)`. Omit both fields when it is `None`.
pub fn format_state(state: State, address: Option<(&str, &str)>) -> String {
    match address {
        Some((ip, iface)) => format!(
            "{{\"type\":\"state\",\"state\":\"{}\",\"ip\":\"{}\",\"iface\":\"{}\"}}\n",
            state.as_str(),
            ip,
            iface,
        ),
        None => format!("{{\"type\":\"state\",\"state\":\"{}\"}}\n", state.as_str()),
    }
}

/// Read one UI object. `stop`, `disable`, and `enable` are commands.
/// `status` and any other text return `None`.
pub fn parse_command(line: &str) -> Option<Command> {
    match message_type(line.trim())? {
        "stop" => Some(Command::Stop),
        "disable" => Some(Command::Disable),
        "enable" => Some(Command::Enable),
        _ => None,
    }
}

fn message_type(line: &str) -> Option<&str> {
    const KEY: &str = "\"type\":\"";
    let start = line.find(KEY)? + KEY.len();
    let rest = line.get(start..)?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Bytes from the menu bar. A line is parsed only after its newline.
#[derive(Debug, Default)]
pub struct CommandParser {
    pending: String,
}

impl CommandParser {
    pub fn new() -> Self {
        Self {
            pending: String::new(),
        }
    }

    pub fn pending(&self) -> &str {
        &self.pending
    }

    /// Append `bytes`. Return one entry per complete line, in order.
    /// A trailing fragment with no newline stays in `pending`.
    pub fn push(&mut self, bytes: &str) -> Vec<Option<Command>> {
        self.pending.push_str(bytes);
        let mut commands = Vec::new();
        while let Some(index) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=index).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            commands.push(parse_command(line));
        }
        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_line_prints_two_decimal_mbps() {
        let line = format_stats(&Stats {
            tx_mbps: 1.25,
            rx_mbps: 5.5,
            tx_bytes: 1,
            rx_bytes: 2,
            tx_pkts: 3,
            rx_pkts: 4,
        });
        assert_eq!(
            line,
            "{\"type\":\"stats\",\"tx_mbps\":1.25,\"rx_mbps\":5.50,\"tx_bytes\":1,\"rx_bytes\":2,\"tx_pkts\":3,\"rx_pkts\":4}\n"
        );
    }

    #[test]
    fn state_with_address() {
        let line = format_state(State::Connected, Some(("10.210.24.4", "utun6")));
        assert_eq!(
            line,
            "{\"type\":\"state\",\"state\":\"connected\",\"ip\":\"10.210.24.4\",\"iface\":\"utun6\"}\n"
        );
    }

    #[test]
    fn state_without_address() {
        let line = format_state(State::Disconnected, None);
        assert_eq!(line, "{\"type\":\"state\",\"state\":\"disconnected\"}\n");
    }

    #[test]
    fn connecting_is_rejected() {
        assert_eq!(State::parse("connecting"), None);
        assert_eq!(State::parse("idle"), Some(State::Idle));
        assert_eq!(State::parse("watching"), Some(State::Watching));
        assert_eq!(State::parse("connected"), Some(State::Connected));
        assert_eq!(State::parse("disconnected"), Some(State::Disconnected));
        assert_eq!(
            format_state(State::Idle, None),
            "{\"type\":\"state\",\"state\":\"idle\"}\n"
        );
        assert_eq!(
            format_state(State::Watching, None),
            "{\"type\":\"state\",\"state\":\"watching\"}\n"
        );
    }

    #[test]
    fn parse_returns_stop_disable_enable() {
        assert_eq!(parse_command("{\"type\":\"stop\"}"), Some(Command::Stop));
        assert_eq!(
            parse_command("{\"type\":\"disable\"}"),
            Some(Command::Disable)
        );
        assert_eq!(
            parse_command("{\"type\":\"enable\"}"),
            Some(Command::Enable)
        );
    }

    #[test]
    fn status_and_other_text_return_none() {
        assert_eq!(parse_command("{\"type\":\"status\"}"), None);
        assert_eq!(parse_command("hello"), None);
    }

    #[test]
    fn two_lines_return_both_commands_in_order() {
        let mut parser = CommandParser::new();
        let got = parser.push("{\"type\":\"stop\"}\n{\"type\":\"disable\"}\n");
        assert_eq!(got, vec![Some(Command::Stop), Some(Command::Disable)]);
        assert_eq!(parser.pending(), "");
    }

    #[test]
    fn partial_line_stays_unparsed() {
        let mut parser = CommandParser::new();
        let got = parser.push("{\"type\":\"enable\"");
        assert!(got.is_empty());
        assert_eq!(parser.pending(), "{\"type\":\"enable\"");
        let got = parser.push("}\n");
        assert_eq!(got, vec![Some(Command::Enable)]);
        assert_eq!(parser.pending(), "");
    }
}
