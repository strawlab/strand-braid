# Remote access with Tailscale

The Braid and Strand Camera web interfaces are normally reached over the local
network — you open `http://<braid-host>:44444/` from a computer on the same LAN.
That works well in the lab, but it breaks down the moment the device you want to
use is **somewhere else**: a laptop at home, a phone on cellular data, or a
collaborator on another campus. The instrument is usually behind a NAT router or
an institutional firewall with no inbound access, so there is no address the
remote device can simply browse to.

[Tailscale](https://tailscale.com/) solves this without any port forwarding,
public IP address, or firewall changes. It puts the Braid computer and your
remote devices onto the same private, encrypted network, so the remote device
can reach Braid by a stable address exactly as if it were on the same LAN.
**No changes to Braid's security model are required** — and you can optionally
let Braid recognize Tailscale connections so no access token is needed at all.

## When Tailscale helps

Use Tailscale when **the device and the instrument are on different networks**:

- a laptop or phone away from the lab (home, conference, cellular),
- a remote collaborator who needs to view or control an experiment,
- any case where the Braid computer is behind NAT/firewall with no inbound access.

You **do not need** Tailscale when the browser is already on the same LAN as
Braid — just use the normal `http://<braid-host>:44444/` URL. Tailscale is also a
separate concern from the [reverse-proxy guide](braid_scaling_reverse_proxy.md),
which addresses a *different* problem (browser connection limits with many camera
previews). The two compose nicely, and the [HTTPS option below](#option-b--https-with-tailscale-serve-also-fixes-many-camera-previews)
solves both at once.

> **Note:** Tailscale is for reaching Braid from a device you control and can
> install software on. It is **not** a way to hand anonymous, zero-install access
> to a random phone — that device must join your tailnet first. For one-off local
> access, the printed token URL / QR code remains the right tool.

## How it works (briefly)

Tailscale builds an encrypted [WireGuard](https://www.wireguard.com/) mesh
between your devices (your *tailnet*). Each device gets a stable address in the
`100.64.0.0/10` range and a name via *MagicDNS* (for example
`braid-host.your-tailnet.ts.net`). Connections are made directly device-to-device
when possible and fall back to Tailscale's relays when NAT makes a direct path
impossible — either way the traffic is end-to-end encrypted and authenticated, so
the connection itself is already secure before Braid sees it.

## Step 1 — install Tailscale on both ends

Install and sign in to Tailscale on:

- the **Braid computer** (see the [Tailscale download
  page](https://tailscale.com/download)), and
- each **remote device** — desktop clients for Windows/macOS/Linux, or the
  Tailscale app from the iOS App Store / Google Play for a phone or tablet.

Sign every device into the **same tailnet** (the same Tailscale account or
organization). Self-hosters who cannot use Tailscale's coordination service can
run [Headscale](https://github.com/juanfont/headscale) instead; the Braid side is
identical.

## Step 2 — find the Braid computer's Tailscale address

On the Braid computer:

```sh
tailscale ip -4        # prints the 100.x.y.z address
tailscale status       # also shows the MagicDNS name
```

Use either the `100.x.y.z` address or the MagicDNS name
(`braid-host.your-tailnet.ts.net`) from the remote device.

## Step 3 — choose how to expose Braid

There are two good options. Option A is the simplest; Option B adds HTTPS (and,
as a bonus, fixes the many-camera preview limit).

### Option A — direct over the tailnet (token-free)

Leave Braid listening on a LAN-reachable address (the default) so the tailnet
interface can reach it, and tell Braid to **trust connections coming from the
Tailscale range**. Because Tailscale has already authenticated and encrypted the
peer, an additional access token is redundant — set
[`trusted_networks`](https://strawlab.org/strand-braid-api-docs/latest/braid_config_data/struct.MainbrainConfig.html#structfield.trusted_networks)
in your Braid config:

```toml
[mainbrain]
# Accept clients arriving over Tailscale without an access token.
# 100.64.0.0/10 is Tailscale's address range (use your WireGuard subnet instead
# if you run plain WireGuard).
trusted_networks = ["100.64.0.0/10"]
```

From the remote device, browse to `http://braid-host.your-tailnet.ts.net:44444/`
(or the `100.x.y.z` address). No token is needed — Braid recognizes the
Tailscale peer and issues the session directly.

> **Security note:** `trusted_networks` matches the **immediate peer address** of
> the connection. This is correct when the browser connects *directly* to Braid
> over Tailscale. Do **not** add `100.64.0.0/10` here if Braid sits behind a
> reverse proxy, because then every request appears to come from the proxy and
> the check no longer identifies the real client. Strand Camera supports the same
> setting via the `--trusted-network` command-line flag.

If you would rather keep token authentication even over Tailscale, simply omit
`trusted_networks`: the usual `?token=<TOKEN>` flow (Braid prints the full URL on
startup) works unchanged over the tailnet.

### Option B — HTTPS with `tailscale serve` (also fixes many-camera previews)

Tailscale can terminate **HTTPS** for you using a certificate it manages
automatically (no certificate files to install on clients), and serve Braid at
its MagicDNS name. This also upgrades the browser connection to **HTTP/2**, which
removes the ~6-connection limit that stalls previews when [many cameras are
shown at once](braid_scaling_reverse_proxy.md) — so a single step gives you both
remote access and preview scaling.

First enable HTTPS / MagicDNS for your tailnet in the Tailscale admin console
(**DNS → Enable MagicDNS**, and **Enable HTTPS**). Then bind Braid to loopback so
that only the local Tailscale proxy can reach it:

```toml
[mainbrain]
http_api_server_addr = "127.0.0.1:44444"
```

> **Note:** Bind to loopback only if all cameras are local. If you also run
> [remote cameras](braid_remote_cameras.md), keep Braid on a LAN-reachable
> address so those camera computers can still connect, and point `tailscale serve`
> at that same address.

Then publish it over HTTPS:

```sh
tailscale serve --bg http://127.0.0.1:44444
```

Braid is now reachable at `https://braid-host.your-tailnet.ts.net/` from any
device on your tailnet. (The exact `tailscale serve` syntax has changed across
Tailscale versions; see [the `tailscale serve`
documentation](https://tailscale.com/kb/1242/tailscale-serve) and
`tailscale serve --help`.)

In this mode you do **not** set `trusted_networks`: the Tailscale proxy reaches
Braid over the loopback interface, and Braid already treats loopback connections
as trusted (no token required). The security boundary is your tailnet — only
devices you have added, subject to your [ACLs](#restricting-who-can-reach-braid),
can reach the served endpoint.

> **Note:** Treating loopback as trusted means *any* local user on the Braid
> computer can also reach the interface. On a single-user instrument machine this
> is fine; on a shared multi-user host, prefer Option A (keep the token, or trust
> only the Tailscale range).

## Restricting who can reach Braid

By default every device in your tailnet can reach every other. To limit which
people or devices may open the Braid interface, use [Tailscale
ACLs](https://tailscale.com/kb/1018/acls) — for example, tag the Braid computer
and grant access only to a specific group:

```jsonc
{
  "tagOwners": { "tag:braid": ["group:lab-admins"] },
  "acls": [
    { "action": "accept", "src": ["group:lab-members"], "dst": ["tag:braid:44444,443"] }
  ]
}
```

This keeps the instrument private to your team even though it is reachable from
anywhere.

## Caveats

- **Every device must run Tailscale and be signed into your tailnet.** This is
  ideal for lab members and named collaborators, but not for handing a stranger
  ad-hoc access — for that, the local token URL / QR code is still the tool.
- **Tailscale's relays may be used** when a direct path is impossible (e.g. both
  ends behind strict NAT). Throughput over a relay is lower than a direct
  connection; this matters only for live video previews, not for control or for
  the tracking data (which travels camera→Braid over the LAN and never leaves it).
- **`tailscale funnel` is different from `tailscale serve`.** Funnel exposes a
  service to the *public internet*; for private remote access to an instrument you
  want `serve` (tailnet-only), not Funnel.
- **Remote cameras do not need Tailscale to talk to Braid** if they are on the
  same LAN. Tailscale is about reaching the *web interface* from afar; the
  camera↔Braid data path is unchanged (see [Braid: Remote
  cameras](braid_remote_cameras.md)).
