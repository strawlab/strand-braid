// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! HTTP router and authentication setup.
//!
//! Extracted from the monolithic `run()` function in `strand-cam.rs`. This
//! builds the axum `Router` for the Strand Cam web UI: it selects the static
//! file service (bundled vs. on-disk), loads or generates the persistent cookie
//! secret, configures the token-auth layer, and wires up the routes.

use base64::Engine;
use preferences_serde1::Preferences;
use strand_bui_backend_session_types::AccessToken;
use tower_http::trace::TraceLayer;

use eyre::Result;

use crate::{
    APP_INFO, COOKIE_SECRET_KEY, StrandCamAppState, callback_handler, cam_name_handler,
    device_connect_urls_handler, events_handler, handle_auth_error,
};

/// Load the persistent cookie/token secret, generating and saving a fresh one
/// if none exists.
///
/// This secret signs both the session cookies and the self-expiring access
/// tokens, so it must be loaded once and shared between [start_listener] (which
/// mints the token) and [build_http_router] (which validates it). Keeping the
/// secret stable across restarts is what lets already-issued browser cookies
/// remain valid through an upgrade.
///
/// [start_listener]: braid_types::start_listener
pub(crate) fn load_persistent_secret(secret_override: Option<String>) -> Result<cookie::Key> {
    let persistent_secret_base64 = if let Some(secret) = secret_override {
        secret
    } else {
        match String::load(&APP_INFO, COOKIE_SECRET_KEY) {
            Ok(secret_base64) => secret_base64,
            Err(_) => {
                tracing::debug!("No secret loaded from preferences file, generating new.");
                let persistent_secret = cookie::Key::generate();
                let persistent_secret_base64 =
                    base64::engine::general_purpose::STANDARD.encode(persistent_secret.master());
                persistent_secret_base64.save(&APP_INFO, COOKIE_SECRET_KEY)?;
                persistent_secret_base64
            }
        }
    };

    // The secret can forge any session cookie and mint any token, so ensure its
    // on-disk file is owner-only.
    braid_types::harden_prefs_file(&APP_INFO, COOKIE_SECRET_KEY);

    let persistent_secret =
        base64::engine::general_purpose::STANDARD.decode(persistent_secret_base64)?;
    Ok(cookie::Key::try_from(persistent_secret.as_slice())?)
}

/// Build the axum router for the web UI. `apply_auth` is false only when an
/// embedding host has already authenticated every request before nesting this
/// router beneath its own authenticated application.
///
/// `persistent_secret` must be the same key that minted the access token in
/// [`braid_types::start_listener`]; the auth layer validates tokens by
/// signature against it.
pub(crate) fn build_http_router(
    persistent_secret: cookie::Key,
    trusted_networks: Vec<axum_token_auth::CidrBlock>,
    access_token: &AccessToken,
    app_state: StrandCamAppState,
    apply_auth: bool,
) -> Result<axum::Router> {
    #[cfg(feature = "bundle_files")]
    let serve_dir = tower_serve_static::ServeDir::new(&crate::ASSETS_DIR);

    #[cfg(feature = "serve_files")]
    let serve_dir = tower_http::services::fs::ServeDir::new(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("yew_frontend")
            .join("dist"),
    );

    // Setup our auth layer. With self-expiring signed tokens the auth layer no
    // longer stores a token value: it accepts any unexpired token signed with
    // `persistent_secret`. We only need to know whether a token is required.
    let token_config = match access_token {
        AccessToken::PreSharedToken(_) => Some(axum_token_auth::TokenConfig::new("token")),
        AccessToken::NoToken => None,
    };
    // `AuthConfig` is `#[non_exhaustive]`, so build it via `new` and set fields.
    let mut cfg = axum_token_auth::AuthConfig::new(persistent_secret);
    cfg.token_config = token_config;
    cfg.cookie_name = "strand-cam-session";
    // Sessions slide forward on use and survive up to 400 days of absence,
    // and the server enforces this expiry (it is embedded in the signed
    // cookie). Existing cookies that predate this field carry no embedded
    // expiry and are treated as non-expiring until renewed, so they stay
    // valid across the upgrade.
    cfg.session_expires = Some(std::time::Duration::from_secs(60 * 60 * 24 * 400)); // 400 days
    // Clients on a trusted overlay network (e.g. Tailscale/WireGuard) are
    // accepted without a token; the overlay has already authenticated them.
    cfg.trusted_networks = trusted_networks;

    let auth_layer = cfg.into_layer();
    // Create axum router.
    let router = axum::Router::new()
        .route("/strand-cam-events", axum::routing::get(events_handler))
        .route("/cam-name", axum::routing::get(cam_name_handler))
        .route(
            "/device-connect-urls",
            axum::routing::get(device_connect_urls_handler),
        )
        .route("/callback", axum::routing::post(callback_handler))
        .fallback_service(serve_dir);

    let router = if apply_auth {
        router.layer(
            tower::ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                // Auth layer will produce an error if the request cannot be
                // authorized so we must handle that.
                .layer(axum::error_handling::HandleErrorLayer::new(
                    handle_auth_error,
                ))
                .layer(auth_layer),
        )
    } else {
        router
    }
    .with_state(app_state);

    Ok(router)
}
