//! `chat-cli`: the Chat example from a terminal, built on the same
//! `chat-core` the phone runs. It lets you be the other person in a
//! conversation with your iPhone, from Linux, without a second phone.
//!
//! ```text
//! cargo run -p chat-cli -- signup "Reuben (laptop)"
//! cargo run -p chat-cli -- send general "Hello from the terminal"
//! cargo run -p chat-cli -- follow general
//! ```
//!
//! The account (with its token) is kept per server in
//! `$XDG_CONFIG_HOME/ixos-chat/cli-accounts.json` (owner-readable only).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use chat_core::{
    Account, ChatClient, ChatConfig, ChatError, ChatEvent, ChatRepo, ChatStore, HttpRepo, Message,
    MessageStatus, OutgoingMessage,
};

const USAGE: &str = "usage: chat-cli [--server URL] COMMAND
  signup NAME          create an account on the server (once per server)
  whoami               the account this terminal uses
  rooms                list conversations, newest first, with the last message
  create TITLE...      start a new conversation
  read ROOM [--last N] show the last N messages (default 20) and mark them read
  send ROOM TEXT...    send a message
  follow ROOM          show new messages as they arrive and mark them read (Ctrl-C to stop)

  --server  the chat server (default $CHAT_SERVER, or http://127.0.0.1:8787)";

const DEFAULT_SERVER: &str = "http://127.0.0.1:8787";

fn main() {
    if let Err(problem) = run(std::env::args().skip(1).collect()) {
        if !problem.is_empty() {
            eprintln!("error: {problem}");
        }
        if problem.is_empty() || problem.starts_with("usage") {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut server = std::env::var("CHAT_SERVER")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_SERVER.into());
    let mut rest = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--server" => server = iter.next().ok_or("--server needs a URL")?,
            "-h" | "--help" => return Err(String::new()),
            _ => rest.push(arg),
        }
    }
    let server = server.trim_end_matches('/').to_string();
    let accounts = AccountFile::default_location()?;
    let (command, args) = rest.split_first().ok_or_else(String::new)?;
    match (command.as_str(), args) {
        ("signup", [name, more @ ..]) => {
            let name = std::iter::once(name.as_str())
                .chain(more.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            signup(&accounts, &server, &name)
        }
        ("whoami", []) => whoami(&accounts, &server),
        ("create", title) if !title.is_empty() => create(&accounts, &server, &title.join(" ")),
        ("rooms", []) => rooms(&accounts, &server),
        ("read", [room]) => read(&accounts, &server, room, 20),
        ("read", [room, flag, n]) if flag == "--last" => {
            let n = n
                .parse()
                .map_err(|_| format!("--last: not a number: {n}"))?;
            read(&accounts, &server, room, n)
        }
        ("send", [room, text @ ..]) if !text.is_empty() => {
            send(&accounts, &server, room, &text.join(" "))
        }
        ("follow", [room]) => follow(&accounts, &server, room),
        _ => Err(format!("usage: unknown command {:?}", rest.join(" "))),
    }
}

fn explain(err: ChatError, server: &str) -> String {
    match err {
        ChatError::Unauthorized { .. } => format!(
            "{server} does not know this terminal's account (was its data file deleted?). Run: chat-cli signup NAME"
        ),
        ChatError::Network { reason } => {
            format!("cannot reach {server}: {reason}. Is chat-server running?")
        }
        other => other.to_string(),
    }
}

fn signup(accounts: &AccountFile, server: &str, name: &str) -> Result<(), String> {
    let account = HttpRepo::with_ureq(server)
        .register(name)
        .map_err(|e| explain(e, server))?;
    accounts.save(server, &account)?;
    println!(
        "Signed up on {server} as {} ({}).",
        account.display_name, account.user_id
    );
    Ok(())
}

fn signed_in(accounts: &AccountFile, server: &str) -> Result<(Account, HttpRepo), String> {
    let account = accounts
        .load(server)?
        .ok_or_else(|| format!("no account for {server} yet. Run: chat-cli signup NAME"))?;
    let repo = HttpRepo::with_ureq(server).with_token(&account.token);
    Ok((account, repo))
}

fn whoami(accounts: &AccountFile, server: &str) -> Result<(), String> {
    let (_, repo) = signed_in(accounts, server)?;
    let me = repo.identity().map_err(|e| explain(e, server))?;
    println!("{} ({}) on {server}", me.display_name, me.user_id);
    Ok(())
}

fn rooms(accounts: &AccountFile, server: &str) -> Result<(), String> {
    let (account, repo) = signed_in(accounts, server)?;
    let mut rooms = repo.list_conversations().map_err(|e| explain(e, server))?;
    // Newest activity first, like the phone's list.
    rooms.sort_by_key(|room| std::cmp::Reverse(room.last_message_at));
    for room in rooms {
        let last = match &room.last_message {
            Some(message) => {
                let unread = room.read_up_to.is_some_and(|read| {
                    message.sent_at > read && message.author_id != account.user_id
                });
                format!(
                    "{}{}",
                    if unread { "* " } else { "" },
                    line(message, &account)
                )
            }
            None => "(no messages)".into(),
        };
        println!("{:<16} {:<20} {last}", room.id, room.title);
    }
    Ok(())
}

fn create(accounts: &AccountFile, server: &str, title: &str) -> Result<(), String> {
    let (_, repo) = signed_in(accounts, server)?;
    let room = repo
        .create_conversation(title)
        .map_err(|e| explain(e, server))?;
    println!("Created {:?}. Its id is {}.", room.title, room.id);
    Ok(())
}

fn read(accounts: &AccountFile, server: &str, room: &str, last: usize) -> Result<(), String> {
    let (account, repo) = signed_in(accounts, server)?;
    let messages = repo
        .load_messages(room, None)
        .map_err(|e| explain(e, server))?;
    let skip = messages.len().saturating_sub(last);
    for message in &messages[skip..] {
        println!("{}", line(message, &account));
    }
    // Reading them here is reading them: the phone shows them as read.
    if let Some(newest) = messages.last() {
        repo.mark_read(room, newest.sent_at)
            .map_err(|e| explain(e, server))?;
    }
    Ok(())
}

fn send(accounts: &AccountFile, server: &str, room: &str, text: &str) -> Result<(), String> {
    let (account, repo) = signed_in(accounts, server)?;
    let stored = repo
        .post_message(&OutgoingMessage {
            id: message_id(),
            conversation_id: room.into(),
            author: account.display_name.clone(),
            author_id: account.user_id.clone(),
            body: text.trim().into(),
        })
        .map_err(|e| explain(e, server))?;
    println!("{}", line(&stored, &account));
    Ok(())
}

/// Follows a room with chat-core's own client: the same reconnect and
/// catch-up logic the phone uses. The room is open, so what arrives is
/// marked read, as on the phone.
fn follow(accounts: &AccountFile, server: &str, room: &str) -> Result<(), String> {
    let (account, repo) = signed_in(accounts, server)?;
    let (tx, rx) = mpsc::channel();
    let client = ChatClient::open(
        Arc::new(repo),
        ChatStore::in_memory(),
        ChatConfig {
            user: account.display_name.clone(),
            user_id: account.user_id.clone(),
            ..ChatConfig::default()
        },
        move |event| {
            let _ = tx.send(event);
        },
    )
    .map_err(|e| e.to_string())?;
    // History first, so the live part starts where it ends.
    let history = client
        .load_messages(room, None)
        .map_err(|e| explain(e, server))?;
    for message in &history[history.len().saturating_sub(10)..] {
        println!("{}", line(message, &account));
    }
    while rx.try_recv().is_ok() {} // the history's own events
    client.subscribe(room).map_err(|e| explain(e, server))?;
    eprintln!("Following {room} on {server}. Ctrl-C to stop.");
    loop {
        match rx.recv_timeout(Duration::from_secs(3600)) {
            Ok(ChatEvent::MessageReceived(message)) if message.status == MessageStatus::Sent => {
                println!("{}", line(&message, &account));
            }
            Ok(ChatEvent::StreamDisconnected {
                reason, will_retry, ..
            }) => {
                eprintln!("(disconnected: {reason})");
                if !will_retry {
                    return Err(format!("stopped following {room}"));
                }
            }
            Ok(ChatEvent::StreamConnected { .. }) => eprintln!("(connected)"),
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
        let _ = std::io::stdout().flush();
    }
}

fn line(message: &Message, account: &Account) -> String {
    let who = if message.author_id == account.user_id {
        "you".to_string()
    } else {
        message.author.clone()
    };
    format!("[{}] {who}: {}", clock(message.sent_at), message.body)
}

/// `HH:MM:SS` UTC. A terminal tool; good enough to tell messages apart.
fn clock(unix_ms: i64) -> String {
    let secs = unix_ms.div_euclid(1000).rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02} UTC",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

fn message_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("cli-{nanos:x}-{:x}", std::process::id())
}

/// The accounts this terminal has, one per server URL.
struct AccountFile {
    path: PathBuf,
}

impl AccountFile {
    fn default_location() -> Result<Self, String> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or("no HOME or XDG_CONFIG_HOME to keep the account in")?;
        Ok(Self {
            path: base.join("ixos-chat").join("cli-accounts.json"),
        })
    }

    fn read_all(&self) -> Result<BTreeMap<String, Account>, String> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("{} is not readable: {e}", self.path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(err) => Err(format!("{}: {err}", self.path.display())),
        }
    }

    fn load(&self, server: &str) -> Result<Option<Account>, String> {
        Ok(self.read_all()?.remove(server))
    }

    fn save(&self, server: &str, account: &Account) -> Result<(), String> {
        let mut all = self.read_all()?;
        all.insert(server.into(), account.clone());
        let json = serde_json::to_vec_pretty(&all).map_err(|e| e.to_string())?;
        write_private(&self.path, &json).map_err(|e| format!("{}: {e}", self.path.display()))
    }
}

/// Writes a file only its owner can read (it holds tokens).
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_are_kept_per_server() {
        let dir = tempfile::tempdir().unwrap();
        let file = AccountFile {
            path: dir.path().join("a/accounts.json"),
        };
        assert_eq!(file.load("http://x").unwrap(), None);
        let account = Account {
            user_id: "u-1".into(),
            display_name: "Ana".into(),
            token: "t".into(),
        };
        file.save("http://x", &account).unwrap();
        file.save(
            "http://y",
            &Account {
                user_id: "u-2".into(),
                ..account.clone()
            },
        )
        .unwrap();
        assert_eq!(file.load("http://x").unwrap(), Some(account));
        assert_eq!(file.load("http://y").unwrap().unwrap().user_id, "u-2");
    }

    #[test]
    fn clock_formats_utc() {
        assert_eq!(clock(0), "00:00:00 UTC");
        assert_eq!(clock((13 * 3600 + 5 * 60 + 9) * 1000 + 999), "13:05:09 UTC");
    }
}
