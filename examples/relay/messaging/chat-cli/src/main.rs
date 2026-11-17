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
    Account, Attachment, ChatClient, ChatConfig, ChatError, ChatEvent, ChatRepo, ChatStore,
    HttpRepo, Message, MessageStatus, OutgoingMessage,
};

const USAGE: &str = "usage: chat-cli [--server URL] COMMAND
  signup NAME          create an account on the server (once per server)
  whoami               the account this terminal uses
  rooms                list conversations, newest first, with the last message
  create TITLE...      start a new conversation
  read ROOM [--last N] show the last N messages (default 20) and mark them read
  send ROOM TEXT...    send a message
  send-file ROOM FILE [TEXT...]  send a photo or document, with an optional caption
  download ROOM MSG [DIR]  save a message's attachments into DIR (default: here)
  reply ROOM MSG TEXT... reply to a message
  edit ROOM MSG TEXT...  replace the text of one of your messages
  delete ROOM MSG      delete one of your messages for everyone
  react ROOM MSG EMOJI [--off]  add (or with --off, remove) your reaction
  follow ROOM          show new messages and changes as they arrive and mark them read (Ctrl-C to stop)

  MSG is @1 for the newest message in the room, @2 for the one before, and
  so on (as `read` numbers them), or a message id or the start of one.

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
        ("send-file", [room, file, text @ ..]) => {
            send_file(&accounts, &server, room, Path::new(file), &text.join(" "))
        }
        ("download", [room, target]) => download(&accounts, &server, room, target, Path::new(".")),
        ("download", [room, target, dir]) => {
            download(&accounts, &server, room, target, Path::new(dir))
        }
        ("reply", [room, target, text @ ..]) if !text.is_empty() => {
            reply(&accounts, &server, room, target, &text.join(" "))
        }
        ("edit", [room, target, text @ ..]) if !text.is_empty() => {
            change(&accounts, &server, room, target, |repo, room, id| {
                repo.edit_message(room, id, text.join(" ").trim())
            })
        }
        ("delete", [room, target]) => change(&accounts, &server, room, target, |repo, room, id| {
            repo.delete_message(room, id)
        }),
        ("react", [room, target, emoji]) => {
            change(&accounts, &server, room, target, |repo, room, id| {
                repo.react(room, id, emoji, true)
            })
        }
        ("react", [room, target, emoji, off]) if off == "--off" => {
            change(&accounts, &server, room, target, |repo, room, id| {
                repo.react(room, id, emoji, false)
            })
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
    for (i, message) in messages.iter().enumerate().skip(skip) {
        // `@1` is the newest: the handle `reply`, `edit` and the rest take.
        println!("@{:<3} {}", messages.len() - i, line(message, &account));
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
    post(&account, &repo, server, room, text, None, Vec::new())
}

fn send_file(
    accounts: &AccountFile,
    server: &str,
    room: &str,
    file: &Path,
    caption: &str,
) -> Result<(), String> {
    let (account, repo) = signed_in(accounts, server)?;
    let size = std::fs::metadata(file)
        .map_err(|e| format!("{}: {e}", file.display()))?
        .len();
    let mime = mime_for(file);
    let id = repo
        .upload_attachment(file, mime)
        .map_err(|e| explain(e, server))?;
    let attachment = Attachment {
        id,
        name: file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into()),
        mime: mime.into(),
        size,
    };
    post(
        &account,
        &repo,
        server,
        room,
        caption,
        None,
        vec![attachment],
    )
}

/// Saves every attachment of one message into `dir`, under its name,
/// after checking its bytes against its id.
fn download(
    accounts: &AccountFile,
    server: &str,
    room: &str,
    target: &str,
    dir: &Path,
) -> Result<(), String> {
    let (_, repo) = signed_in(accounts, server)?;
    let message = find_message(&repo, server, room, target)?;
    if message.attachments.is_empty() {
        return Err("that message has no attachments".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for attachment in &message.attachments {
        let name = Path::new(&attachment.name)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| attachment.id.clone());
        let destination = dir.join(name);
        repo.download_attachment(&attachment.id, &destination)
            .map_err(|e| explain(e, server))?;
        let actual = chat_core::sha256_file(&destination)
            .map_err(|e| format!("{}: {e}", destination.display()))?;
        if actual != attachment.id {
            let _ = std::fs::remove_file(&destination);
            return Err(format!("{} arrived damaged; try again", attachment.name));
        }
        println!(
            "{} ({})",
            destination.display(),
            human_size(attachment.size)
        );
    }
    Ok(())
}

/// The media type for a file name, for the few kinds people send.
fn mime_for(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{:.0} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn reply(
    accounts: &AccountFile,
    server: &str,
    room: &str,
    target: &str,
    text: &str,
) -> Result<(), String> {
    let (account, repo) = signed_in(accounts, server)?;
    let to = find_message(&repo, server, room, target)?;
    post(&account, &repo, server, room, text, Some(to.id), Vec::new())
}

fn post(
    account: &Account,
    repo: &HttpRepo,
    server: &str,
    room: &str,
    text: &str,
    reply_to: Option<String>,
    attachments: Vec<Attachment>,
) -> Result<(), String> {
    let stored = repo
        .post_message(&OutgoingMessage {
            id: message_id(),
            conversation_id: room.into(),
            author: account.display_name.clone(),
            author_id: account.user_id.clone(),
            body: text.trim().into(),
            reply_to,
            attachments,
        })
        .map_err(|e| explain(e, server))?;
    println!("{}", line(&stored, account));
    Ok(())
}

/// Edits, deletes or reacts to one message and shows what it became.
fn change(
    accounts: &AccountFile,
    server: &str,
    room: &str,
    target: &str,
    apply: impl FnOnce(&HttpRepo, &str, &str) -> Result<Message, ChatError>,
) -> Result<(), String> {
    let (account, repo) = signed_in(accounts, server)?;
    let message = find_message(&repo, server, room, target)?;
    let changed = apply(&repo, room, &message.id).map_err(|e| explain(e, server))?;
    println!("{}", line(&changed, &account));
    Ok(())
}

/// The message `target` names: `@N` counts back from the newest, anything
/// else is an id or the unique start of one.
fn find_message(
    repo: &HttpRepo,
    server: &str,
    room: &str,
    target: &str,
) -> Result<Message, String> {
    let messages = repo
        .load_messages(room, None)
        .map_err(|e| explain(e, server))?;
    pick(messages, target)
}

fn pick(mut messages: Vec<Message>, target: &str) -> Result<Message, String> {
    if let Some(n) = target.strip_prefix('@') {
        let n: usize = n
            .parse()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("{target}: use @1 for the newest message"))?;
        let count = messages.len();
        return count
            .checked_sub(n)
            .map(|i| messages.swap_remove(i))
            .ok_or_else(|| format!("{target}: the room has only {count} messages"));
    }
    if let Some(i) = messages.iter().position(|m| m.id == target) {
        return Ok(messages.swap_remove(i));
    }
    let mut starting: Vec<Message> = messages
        .into_iter()
        .filter(|m| m.id.starts_with(target))
        .collect();
    match starting.len() {
        1 => Ok(starting.remove(0)),
        0 => Err(format!("no message {target:?} in this room")),
        n => Err(format!(
            "{n} messages start with {target:?}; give more of the id"
        )),
    }
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
            Ok(ChatEvent::MessageUpdated(message)) => {
                println!("changed: {}", line(&message, &account));
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
    if message.deleted {
        return format!("[{}] {who}: (deleted)", clock(message.sent_at));
    }
    let mut text = format!("[{}] {who}: ", clock(message.sent_at));
    if let Some(to) = &message.reply_to {
        text.push_str(&format!("(reply to {}) ", short(to)));
    }
    text.push_str(&message.body);
    for attachment in &message.attachments {
        if !text.ends_with(' ') {
            text.push(' ');
        }
        let kind = if attachment.mime.starts_with("audio/") {
            "voice"
        } else {
            "file"
        };
        text.push_str(&format!(
            "[{kind}: {} ({})]",
            attachment.name,
            human_size(attachment.size)
        ));
    }
    if message.edited_at.is_some() {
        text.push_str(" (edited)");
    }
    for (emoji, people) in &message.reactions {
        text.push_str(&format!("  {emoji}{}", people.len()));
    }
    text
}

/// Enough of an id to tell messages apart and to pass back as MSG.
fn short(id: &str) -> &str {
    id.char_indices().nth(12).map_or(id, |(i, _)| &id[..i])
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

    fn message(id: &str) -> Message {
        Message {
            id: id.into(),
            body: id.into(),
            ..Message::default()
        }
    }

    #[test]
    fn messages_are_picked_by_position_id_or_prefix() {
        let all = || vec![message("abc-1"), message("abd-2"), message("x")];
        assert_eq!(pick(all(), "@1").unwrap().id, "x");
        assert_eq!(pick(all(), "@3").unwrap().id, "abc-1");
        assert!(pick(all(), "@4").is_err());
        assert!(pick(all(), "@0").is_err());
        assert_eq!(pick(all(), "x").unwrap().id, "x");
        assert_eq!(pick(all(), "abd").unwrap().id, "abd-2");
        assert!(pick(all(), "ab").unwrap_err().contains("2 messages"));
        assert!(pick(all(), "zz").is_err());
    }

    #[test]
    fn lines_show_edits_deletions_replies_and_reactions() {
        let me = Account {
            user_id: "u-1".into(),
            display_name: "Ana".into(),
            token: String::new(),
        };
        let mut m = Message {
            author: "Ben".into(),
            body: "hi".into(),
            reply_to: Some("0123456789abcdef".into()),
            edited_at: Some(1),
            ..message("m")
        };
        m.reactions
            .insert("👍".into(), vec!["u-1".into(), "u-2".into()]);
        assert_eq!(
            line(&m, &me),
            "[00:00:00 UTC] Ben: (reply to 0123456789ab) hi (edited)  👍2"
        );
        m.deleted = true;
        assert_eq!(line(&m, &me), "[00:00:00 UTC] Ben: (deleted)");
    }

    #[test]
    fn files_get_a_media_type_and_a_readable_size() {
        assert_eq!(mime_for(Path::new("IMG_1.JPG")), "image/jpeg");
        assert_eq!(mime_for(Path::new("notes.pdf")), "application/pdf");
        assert_eq!(mime_for(Path::new("blob")), "application/octet-stream");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(3 * 1024 * 1024 / 2), "1.5 MB");
        let message = Message {
            author: "Ben".into(),
            attachments: vec![Attachment {
                name: "a.pdf".into(),
                size: 2048,
                ..Attachment::default()
            }],
            ..message("m")
        };
        let me = Account {
            user_id: "u-1".into(),
            display_name: "Ana".into(),
            token: String::new(),
        };
        assert_eq!(
            line(&message, &me),
            "[00:00:00 UTC] Ben: m [file: a.pdf (2 KB)]"
        );
        let voice = Message {
            body: String::new(),
            attachments: vec![Attachment {
                name: "Voice message.m4a".into(),
                mime: "audio/mp4".into(),
                size: 40 * 1024,
                ..Attachment::default()
            }],
            ..message.clone()
        };
        assert_eq!(
            line(&voice, &me),
            "[00:00:00 UTC] Ben: [voice: Voice message.m4a (40 KB)]"
        );
    }

    #[test]
    fn clock_formats_utc() {
        assert_eq!(clock(0), "00:00:00 UTC");
        assert_eq!(clock((13 * 3600 + 5 * 60 + 9) * 1000 + 999), "13:05:09 UTC");
    }
}
