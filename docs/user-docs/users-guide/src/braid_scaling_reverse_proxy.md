# Running Braid with many cameras (reverse proxy)

When you open the Braid web interface in a browser with **many cameras**, the
live camera previews can stop updating once more than a handful are shown at
once. This is a limitation of the browser, not of Braid, and it is easily solved
by placing an HTTPS reverse proxy in front of Braid. **No changes to Braid or to
its configuration logic are required** — the proxy is purely a deployment step.

## Why the previews stall

Browsers limit how many simultaneous network connections they will open to a
single server over HTTP/1.1 — typically **6 connections per origin** (host and
port). The Braid web interface holds connections open continuously:

- **one** connection for the main Braid event stream, plus
- **one** connection for *each live camera preview* (every preview tile opens its
  own [server-sent events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events)
  stream through Braid's camera proxy).

So with the main stream plus ~5 live previews you reach the browser's 6-connection
limit. Any further previews — and the per-frame requests that pace them — simply
queue inside the browser and never run, so those tiles freeze.

This limit applies **only to browsers**. Remote Strand Camera computers
connecting to Braid are not affected and do **not** need the proxy; they continue
to connect directly to Braid as described in [Braid: Remote
cameras](braid_remote_cameras.md).

## The fix: serve the web interface over HTTP/2

HTTP/2 multiplexes many independent streams over a *single* connection, so the
6-connection limit disappears and all camera previews stream concurrently.
Browsers only use HTTP/2 over TLS (HTTPS), so the solution is to run a small
reverse proxy that terminates HTTPS and forwards plain HTTP to Braid on the same
machine:

```text
   browser  ──HTTPS / HTTP/2──▶  reverse proxy  ──HTTP (localhost)──▶  braid-run
```

Because the browser now sees a single HTTP/2 connection to the proxy, all of the
event streams and preview requests share it. Braid itself keeps speaking ordinary
HTTP/1.1 on the loopback interface and is unchanged.

We recommend [Caddy](https://caddyserver.com/) because it obtains and renews TLS
certificates automatically and streams server-sent events correctly out of the
box. The examples below use Caddy; any HTTP/2-capable proxy (nginx, Traefik,
…) works too (see the note on buffering at the end).

## Step 1: let the proxy reach Braid

Run the proxy on the **same computer** as `braid-run` and forward to Braid over
the loopback interface. Braid listens on port `44444` by default. If you want
Braid to accept *only* proxied browser connections, bind its HTTP server to
loopback by setting [the `http_api_server_addr`
field](https://strawlab.org/strand-braid-api-docs/latest/braid_config_data/struct.MainbrainConfig.html#structfield.http_api_server_addr):

```toml
[mainbrain]
http_api_server_addr = "127.0.0.1:44444"
```

> **Note:** Bind to loopback only if all cameras are local. If you also run
> [remote cameras](braid_remote_cameras.md), those computers must still be able
> to reach Braid's HTTP server directly, so keep Braid bound to a LAN-reachable
> address (the default) and simply let the proxy forward to that same address.

## Step 2: configure Caddy

Automatic TLS still requires the browser to *trust* the certificate. On a private
network you have two practical options.

### Option A — with a domain name (publicly trusted, nothing to install on clients)

If you control a domain name, point a DNS record at the Braid computer's address
(a private/LAN IP is fine) and let Caddy obtain a publicly trusted certificate
using the **DNS-01** challenge, which needs no inbound access to the LAN. This is
the smoothest option: every browser trusts the certificate with no per-machine
setup.

```caddy
braid.lab.example.com {
    reverse_proxy 127.0.0.1:44444
    tls {
        dns <your-dns-provider> {env.DNS_API_TOKEN}
    }
}
```

DNS-01 requires a build of Caddy that includes your DNS provider's module (see
the [Caddy DNS-challenge
documentation](https://caddyserver.com/docs/automatic-https#dns-challenge); build
with [`xcaddy`](https://github.com/caddyserver/xcaddy)) and an API token for that
provider supplied via the `DNS_API_TOKEN` environment variable.

### Option B — fully offline (Caddy's internal CA, one-time trust install)

With no domain or no internet access, let Caddy act as its own certificate
authority. Caddy issues and renews certificates automatically; you install
Caddy's root certificate on each computer that will open the web interface
**once**.

```caddy
braid.local {
    reverse_proxy 127.0.0.1:44444
    tls internal
}
```

Install Caddy's root certificate into the system trust store:

```sh
caddy trust
```

On other computers, copy Caddy's root certificate (printed by `caddy trust`,
typically under Caddy's data directory at
`pki/authorities/local/root.crt`) and add it to that machine's (or browser's)
trust store. On a managed set of lab computers this can be pushed out
automatically. Make sure `braid.local` resolves to the Braid computer on each
client (via DNS, an `/etc/hosts` entry, or mDNS).

## Step 3: open the web interface through the proxy

Browse to the proxy's HTTPS address (e.g. `https://braid.lab.example.com/`)
instead of `http://<braid-host>:44444/`. Braid's access token works exactly as
before: append `?token=<TOKEN>` (Braid prints the full URL with the token on
startup) on first connection; the browser then stores a cookie and reconnects
without it. Do **not** disable Braid's token authentication — the proxy only
changes the transport, not the security model.

## Scaling notes and caveats

- **The ceiling moves from ~6 to ~100–250.** HTTP/2 multiplexes streams but
  still caps the number of *concurrent* streams per connection (Caddy allows a
  few hundred by default). Each live preview uses one stream, so this is far
  above any realistic camera count. If you ever approach it, raise the proxy's
  maximum-concurrent-streams setting — it is a single configuration knob.
- **The proxy → Braid hop stays HTTP/1.1, and that is fine.** It is a
  server-to-server connection on the same machine and is not subject to the
  browser's per-origin limit.
- **If you use nginx instead of Caddy**, disable response buffering for the
  event streams or previews will be delayed indefinitely: set
  `proxy_buffering off;`, `proxy_http_version 1.1;`, and forward to Braid over
  HTTP/1.1. Caddy needs none of this.
- **Remote cameras do not use the proxy, and should not.** The connection limit
  is a browser-only concern — Strand Camera is a native client and is not subject
  to it — so [remote cameras](braid_remote_cameras.md) keep connecting directly
  to Braid. There is also nothing to gain: the high-rate tracking data travels
  camera→Braid over **UDP**, which an HTTP proxy cannot carry, and the only HTTP
  traffic between them is light control messages. Routing cameras through the
  proxy would add a TLS certificate-trust and configuration burden on every
  camera computer for no real benefit. The proxy and the cameras simply use
  independent paths to Braid.
