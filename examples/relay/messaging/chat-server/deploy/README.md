# Hosting the Chat server

The Chat example needs its server (`examples/relay/messaging/chat-server`). On the desk it
runs on your computer (`make chat-server ARGS="--bind 0.0.0.0:8787"`) and the
phone reaches it on the same Wi-Fi. To use Chat anywhere, including inside
Device Lab on the go, run it on an always-on Linux machine of your own and
reach it over your tailnet.

```bash
# From a Linux machine with this checkout, the same CPU architecture as the
# server, and SSH (with sudo) to it:
examples/relay/messaging/chat-server/deploy/deploy.sh <ssh host>
```

The script builds `chat-server` (through `forge queue` when it is there),
installs it as the `ixos-chat` systemd service listening on 127.0.0.1:8787
only, keeps its data in `/var/lib/ixos-chat/` under its own throwaway user,
and has Tailscale serve it over HTTPS on the tailnet. It prints the address:

```
[deploy] Chat server: https://<server>.<tailnet>.ts.net:8787
```

Enter that address on Chat's sign-up screen (the app suggests the project's
own server). The phone needs Tailscale on. Running the script again updates
the binary and keeps accounts and messages.

**Why the tailnet, not the internet.** The server is a demo: anyone who can
reach it can create an account, and it has no rate limits. Behind Tailscale
only your own devices reach it, and Tailscale's certificate gives the
connection TLS, so account tokens never cross a network in the clear.

**Needs:** Tailscale on the server, with HTTPS certificates turned on for the
tailnet (admin console, DNS, HTTPS Certificates); the iPhone on the same
tailnet.

**Check it:** `curl -s -o /dev/null -w '%{http_code}\n' https://<address>/rooms`
prints 401 (it answers, and wants an account). On the server:
`systemctl status ixos-chat`, `journalctl -u ixos-chat`.

**Remove it:** `sudo tailscale serve --https=8787 off`,
`sudo systemctl disable --now ixos-chat`, then delete
`/etc/systemd/system/ixos-chat.service`, `/usr/local/bin/chat-server` and,
with its data, `/var/lib/private/ixos-chat/`.
