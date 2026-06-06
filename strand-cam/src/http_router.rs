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
    events_handler, handle_auth_error,
};

/// Build the axum router for the web UI, including the auth layer.
pub(crate) fn build_http_router(
    secret: Option<String>,
    access_token: &AccessToken,
    app_state: StrandCamAppState,
) -> Result<axum::Router> {
    #[cfg(feature = "bundle_files")]
    let serve_dir = tower_serve_static::ServeDir::new(&crate::ASSETS_DIR);

    #[cfg(feature = "serve_files")]
    let serve_dir = tower_http::services::fs::ServeDir::new(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("yew_frontend")
            .join("dist"),
    );

    let persistent_secret_base64 = if let Some(secret) = &secret {
        secret.clone()
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

    let persistent_secret =
        base64::engine::general_purpose::STANDARD.decode(persistent_secret_base64)?;
    let persistent_secret = cookie::Key::try_from(persistent_secret.as_slice())?;

    // Setup our auth layer.
    let token_config = match access_token {
        AccessToken::PreSharedToken(value) => Some(axum_token_auth::TokenConfig {
            name: "token".to_string(),
            value: value.clone(),
        }),
        AccessToken::NoToken => None,
    };
    let cfg = axum_token_auth::AuthConfig {
        token_config,
        persistent_secret,
        cookie_name: "strand-cam-session",
        cookie_expires: Some(std::time::Duration::from_secs(60 * 60 * 24 * 400)), // 400 days
    };

    let auth_layer = cfg.into_layer();
    // Create axum router.
    let router = axum::Router::new()
        .route("/strand-cam-events", axum::routing::get(events_handler))
        .route("/cam-name", axum::routing::get(cam_name_handler))
        .route("/callback", axum::routing::post(callback_handler))
        .fallback_service(serve_dir)
        .layer(
            tower::ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                // Auth layer will produce an error if the request cannot be
                // authorized so we must handle that.
                .layer(axum::error_handling::HandleErrorLayer::new(
                    handle_auth_error,
                ))
                .layer(auth_layer),
        )
        .with_state(app_state);

    Ok(router)
}
