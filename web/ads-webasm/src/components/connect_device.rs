// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A button + modal dialog that helps connect another device (e.g. a phone) to
//! the web UI currently being served.
//!
//! The backend exposes a `device-connect-urls` endpoint that returns, for each
//! network interface the server is reachable on, a full URL including a
//! freshly minted short-lived access token. This component fetches that list
//! and renders each (non-loopback) URL as a QR code that can be scanned by a
//! phone on the same network to open the same UI directly.

use strand_bui_backend_session_types::DeviceConnectUrls;
use wasm_bindgen::{JsCast, UnwrapThrowExt};
use wasm_bindgen_futures::JsFuture;
use yew::{Component, Context, Html, html};
use yew_tincture::components::Button;

/// Relative URL of the backend endpoint returning the device connection URLs.
/// Relative so it works behind a reverse proxy and carries the session cookie.
const DEVICE_CONNECT_URLS_PATH: &str = "device-connect-urls";

/// State of the in-flight / completed fetch of connection URLs.
enum Fetch {
    /// The request has not yet completed.
    Loading,
    /// The request succeeded.
    Loaded(DeviceConnectUrls),
    /// The request failed.
    Failed(String),
}

pub struct ConnectDevice {
    /// Whether the modal dialog is open.
    open: bool,
    /// Result of fetching the connection URLs (only meaningful while `open`).
    fetch: Fetch,
}

pub enum Msg {
    Open,
    Close,
    Loaded(DeviceConnectUrls),
    Failed(String),
}

impl Component for ConnectDevice {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            open: false,
            fetch: Fetch::Loading,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Open => {
                self.open = true;
                self.fetch = Fetch::Loading;
                ctx.link().send_future(async {
                    match fetch_connect_urls().await {
                        Ok(urls) => Msg::Loaded(urls),
                        Err(err) => Msg::Failed(err),
                    }
                });
                true
            }
            Msg::Close => {
                self.open = false;
                true
            }
            Msg::Loaded(urls) => {
                self.fetch = Fetch::Loaded(urls);
                true
            }
            Msg::Failed(err) => {
                self.fetch = Fetch::Failed(err);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        html! {
            <>
                <Button
                    title={"Connect a device 📱"}
                    onsignal={link.callback(|_| Msg::Open)}
                />
                { if self.open { self.view_modal(ctx) } else { html!{} } }
            </>
        }
    }
}

impl ConnectDevice {
    fn view_modal(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let body = match &self.fetch {
            Fetch::Loading => html! { <p>{ "Loading…" }</p> },
            Fetch::Failed(err) => html! {
                <p class="connect-device-error">
                    { format!("Could not load connection info: {err}") }
                </p>
            },
            Fetch::Loaded(info) if info.loopback_only => html! {
                <p>
                    { "This server is only reachable on localhost, so other \
                       devices cannot connect. Start it bound to a network \
                       address (for example " }
                    <code>{ "0.0.0.0:<port>" }</code>
                    { ") to allow device connections." }
                </p>
            },
            Fetch::Loaded(info) => view_urls(info),
        };
        html! {
            <div class="modal-container connect-device-modal">
                <h1>{ "Connect a device" }</h1>
                <p>{ "Scan a QR code below with your phone's camera to open this \
                      page on your phone. Your phone must be on the same network \
                      as this computer." }</p>
                { body }
                <p>
                    <Button
                        title={"Close"}
                        onsignal={link.callback(|_| Msg::Close)}
                    />
                </p>
            </div>
        }
    }
}

fn view_urls(info: &DeviceConnectUrls) -> Html {
    // Loopback addresses (127.0.0.1, ::1) can never be reached from another
    // device, so do not offer them for scanning.
    let scannable: Vec<&String> = info
        .urls
        .iter()
        .filter(|url| !is_loopback_url(url))
        .collect();

    if scannable.is_empty() {
        // Should not happen (the backend reports `loopback_only` in this case),
        // but handle it defensively rather than showing an empty dialog.
        return html! {
            <p>{ "No network address is available for other devices to connect to." }</p>
        };
    }

    let hint = if scannable.len() > 1 {
        html! { <p>{ "If one address does not work, try another — they \
        correspond to different network interfaces." }</p> }
    } else {
        html! {}
    };

    let items = scannable.iter().map(|url| {
        let qr = render_qr(url).unwrap_or_else(|| {
            html! { <p class="connect-device-error">{ "Failed to render QR code." }</p> }
        });
        html! {
            <li class="connect-device-item">
                { qr }
                <p class="connect-device-link">
                    <a href={(*url).clone()} target="_blank" rel="noopener">{ (*url).clone() }</a>
                </p>
            </li>
        }
    });

    html! {
        <>
            { hint }
            <ul class="connect-device-list">
                { for items }
            </ul>
        </>
    }
}

/// Whether `url`'s host is a loopback address (so unreachable from a phone).
fn is_loopback_url(url: &str) -> bool {
    // Hosts as produced by the backend look like `http://127.0.0.1:3440/...`.
    let after_scheme = url.strip_prefix("http://").unwrap_or(url);
    let host = after_scheme
        .split(['/', ':'])
        .next()
        .unwrap_or(after_scheme);
    host == "127.0.0.1" || host == "::1" || host == "localhost"
}

/// Render a QR code for `url` as an `<img>` element with an inline SVG data URI.
fn render_qr(url: &str) -> Option<Html> {
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .build();
    // The renderer prepends an `<?xml ...?>` declaration; keep only the `<svg>`
    // element for embedding in a data URI.
    let svg = match svg.find("<svg") {
        Some(idx) => &svg[idx..],
        None => svg.as_str(),
    };
    // Percent-encode so the SVG (which contains `#`, `<`, `>`, quotes, spaces)
    // is a valid data URI in any browser.
    let encoded: String = js_sys::encode_uri_component(svg).into();
    let src = format!("data:image/svg+xml,{encoded}");
    Some(html! {
        <img
            class="connect-device-qr"
            src={src}
            alt={format!("QR code for {url}")}
            width="220"
            height="220"
        />
    })
}

/// Fetch the connection URLs from the backend. The request is made to a
/// relative URL so the browser sends the existing session cookie.
async fn fetch_connect_urls() -> Result<DeviceConnectUrls, String> {
    let window = web_sys::window().ok_or("no window")?;
    let request =
        web_sys::Request::new_with_str(DEVICE_CONNECT_URLS_PATH).map_err(|e| format!("{e:?}"))?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_value.dyn_into().unwrap_throw();
    if !resp.ok() {
        return Err(format!("HTTP status {}", resp.status()));
    }
    let text_value = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text_value.as_string().ok_or("response was not text")?;
    serde_json::from_str(&text).map_err(|e| format!("invalid response: {e}"))
}
