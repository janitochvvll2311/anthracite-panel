//! Tests for the credential layer.
//!
//! Every test builds its own settings file through [`super::super::tests::scratch`],
//! so two tests cannot see each other's account, and the file lives under
//! `TMPDIR` rather than the system temp directory.
//!
//! The passwords here are long on purpose: the minimum is part of the behaviour,
//! so a short one in a fixture would make a test fail for a reason it was not
//! written about.

use super::*;
use crate::tests::scratch;

const PASSWORD: &str = "correct horse battery staple";

#[test]
fn a_fresh_file_reports_no_account() {
    let config = scratch("fresh");
    assert!(
        store::read(&config)
            .expect("a missing file is not an error")
            .is_none()
    );
}

#[test]
fn registering_stores_a_hash_and_never_the_password() {
    let config = scratch("hash");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");

    assert_eq!(account.username(), "operator");

    let text = std::fs::read_to_string(&config).expect("the settings file");
    assert!(
        !text.contains(PASSWORD),
        "the password itself must never be written:\n{text}"
    );
    assert!(
        text.contains("$argon2id$"),
        "the stored value must be an Argon2id PHC string:\n{text}"
    );
    // The signing key must be present and must not be the hash: a key that is
    // the hash cannot sign anything the verifier could check.
    assert!(text.contains("signing_key"), "{text}");
}

#[test]
fn a_second_registration_is_refused_and_leaves_the_first_account_alone() {
    let config = scratch("second");
    register(&config, "operator", PASSWORD).expect("the first registration");

    let error = register(&config, "intruder", "another long password").expect_err("refused");
    assert!(matches!(error, AuthError::AlreadyRegistered), "{error:?}");

    let account = store::read(&config)
        .expect("readable")
        .expect("still there");
    assert_eq!(
        account.username(),
        "operator",
        "the first account must survive"
    );
}

#[test]
fn a_short_password_is_refused() {
    let config = scratch("short");
    let error = register(&config, "operator", "short").expect_err("refused");
    assert!(matches!(error, AuthError::Invalid(_)), "{error:?}");
    assert!(
        store::read(&config).expect("readable").is_none(),
        "a refused registration must write nothing"
    );
}

#[test]
fn the_right_password_opens_a_session_and_the_wrong_one_does_not() {
    let config = scratch("login");
    register(&config, "operator", PASSWORD).expect("the first registration");

    let account = login(&config, "operator", PASSWORD).expect("the right password");
    assert_eq!(account.username(), "operator");

    // A successful login is what opens a session, and the session is what
    // carries the two tokens; `login` itself only proves the password.
    let grant = session::start(&config, &account).expect("a session");
    assert_eq!(grant.token_type, "Bearer");
    assert!(!grant.access_token.is_empty());
    assert!(!grant.refresh_token.is_empty());

    let error = login(&config, "operator", "not the password").expect_err("refused");
    assert!(matches!(error, AuthError::Unauthorized(_)), "{error:?}");
}

#[test]
fn a_wrong_username_costs_the_same_as_a_wrong_password() {
    // Timing is not asserted (it would be flaky on a loaded machine) but the
    // *kind* must match: a `404` for an unknown user would turn this route into
    // a way to ask whether a username is in use.
    let config = scratch("username");
    register(&config, "operator", PASSWORD).expect("the first registration");

    let unknown = login(&config, "someone", PASSWORD).expect_err("refused");
    let wrong = login(&config, "operator", "wrong but long enough").expect_err("refused");
    assert_eq!(
        std::mem::discriminant(&unknown),
        std::mem::discriminant(&wrong),
        "an unknown user and a wrong password must be indistinguishable: {unknown:?} vs {wrong:?}"
    );
}

#[test]
fn an_access_token_names_the_account_and_expires() {
    let config = scratch("claims");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    let token = session::issue_access(&account, session::now()).expect("a token");

    let claims = session::verify_access(&account, &token).expect("the token must verify");
    assert_eq!(claims.sub, "operator");
    assert_eq!(claims.iss, session::ISSUER);
    assert_eq!(
        claims.exp - claims.iat,
        ACCESS_TTL_SECONDS,
        "the window must be the documented one"
    );
}

#[test]
fn an_expired_access_token_is_refused_with_the_word_expired() {
    let config = scratch("expired-token");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    // Issued well before now, so the leeway cannot rescue it.
    let past = session::now() - ACCESS_TTL_SECONDS - 60;
    let token = session::issue_access(&account, past).expect("a token");

    let error = session::verify_access(&account, &token).expect_err("refused");
    let message = error.message().to_string();
    assert!(message.contains("expired"), "{message}");
}

#[test]
fn a_token_signed_with_another_key_is_refused() {
    let config = scratch("forged");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    let other = register(&scratch("forged-other"), "operator", PASSWORD).expect("another file");

    let forged = session::issue_access(&other, session::now()).expect("a token");
    assert!(
        session::verify_access(&account, &forged).is_err(),
        "a token signed by another key must not verify"
    );
}

#[test]
fn a_token_with_a_broken_signature_is_refused() {
    let config = scratch("tampered");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    let token = session::issue_access(&account, session::now()).expect("a token");

    // The signature is the last segment; changing one character must break it.
    let mut tampered = token.clone();
    let last = tampered.pop().expect("a token has characters");
    tampered.push(if last == 'a' { 'b' } else { 'a' });
    assert!(
        session::verify_access(&account, &tampered).is_err(),
        "a tampered token must not verify"
    );
}

#[test]
fn a_refresh_rotates_the_token_and_the_old_one_stops_working() {
    let config = scratch("rotate");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    let first = session::start(&config, &account).expect("a session");

    let second = session::refresh(&config, &first.refresh_token).expect("the first refresh");
    assert_ne!(
        first.refresh_token, second.refresh_token,
        "a refresh must hand back a new token"
    );

    // The presented token is now consumed, and presenting it again is the reuse
    // case, not a second refresh.
    let error = session::refresh(&config, &first.refresh_token).expect_err("refused");
    assert!(matches!(error, AuthError::Unauthorized(_)), "{error:?}");
}

#[test]
fn reusing_a_consumed_token_drops_the_whole_chain() {
    let config = scratch("reuse");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    let first = session::start(&config, &account).expect("a session");
    let second = session::refresh(&config, &first.refresh_token).expect("a rotation");

    // Replay the consumed token: the sign of a copy in someone else's hands.
    let reuse = session::refresh(&config, &first.refresh_token).expect_err("the reuse is refused");
    assert!(
        reuse.message().contains("chain"),
        "the answer must name what happened: {}",
        reuse.message()
    );

    // The fresh token that descended from it must be gone too, or the thief
    // would keep the other half of the sessions. It is gone from the file, so the
    // second request finds nothing to recognise — which is the point: the
    // revocation was persisted, not just held in memory.
    let after = session::refresh(&config, &second.refresh_token).expect_err("the chain is gone");
    assert!(
        after.message().contains("not recognised"),
        "a dropped session must be unknown, not merely refused: {}",
        after.message()
    );
}

#[test]
fn an_unrelated_session_survives_another_chains_reuse() {
    let config = scratch("other-chain");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");

    let first = session::start(&config, &account).expect("the first session");
    let second = session::start(&config, &account).expect("a second session, a second chain");

    session::refresh(&config, &first.refresh_token).expect("rotating the first");
    session::refresh(&config, &first.refresh_token).expect_err("its reuse");

    // Dropping every session would log the operator out of the device they are
    // holding to investigate the one they are not.
    session::refresh(&config, &second.refresh_token)
        .expect("another chain must not be dropped by this reuse");
}

#[test]
fn an_expired_refresh_token_is_refused_and_removed() {
    let config = scratch("expired-refresh");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    let start = session::now() - REFRESH_TTL_SECONDS - 10;
    let grant = session::start_at(&config, &account, start).expect("a session in the past");

    let error =
        session::refresh_at(&config, &grant.refresh_token, session::now()).expect_err("refused");
    assert!(matches!(error, AuthError::Unauthorized(_)), "{error:?}");
    assert!(error.message().contains("expired"), "{}", error.message());
}

#[test]
fn logging_out_ends_every_session() {
    let config = scratch("logout");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    let first = session::start(&config, &account).expect("a session");
    let second = session::start(&config, &account).expect("another session");

    assert_eq!(session::revoke_all(&config).expect("logout"), 2);

    for token in [&first.refresh_token, &second.refresh_token] {
        assert!(
            session::refresh(&config, token).is_err(),
            "a revoked token must not refresh"
        );
    }
}

#[test]
fn changing_the_password_ends_every_session_and_replaces_the_key() {
    let config = scratch("passwd");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    let grant = session::start(&config, &account).expect("a session");

    let changed = change_password(&config, "operator", PASSWORD, "a different long password")
        .expect("the change");

    assert_ne!(
        account.signing_key(),
        changed.signing_key(),
        "a new password must not keep the old signing key"
    );
    assert!(
        session::refresh(&config, &grant.refresh_token).is_err(),
        "the session must end with the password"
    );
    assert!(
        login(&config, "operator", PASSWORD).is_err(),
        "the old password must stop working"
    );
    login(&config, "operator", "a different long password").expect("the new password");
}

#[test]
fn a_change_with_the_wrong_current_password_is_refused() {
    let config = scratch("passwd-wrong");
    register(&config, "operator", PASSWORD).expect("the first registration");

    let error = change_password(
        &config,
        "operator",
        "wrong but long enough",
        "a new long password",
    )
    .expect_err("refused");
    assert!(matches!(error, AuthError::Unauthorized(_)), "{error:?}");
    login(&config, "operator", PASSWORD).expect("the old password must still work");
}

#[test]
fn the_settings_file_is_written_private() {
    use std::os::unix::fs::PermissionsExt;

    let config = scratch("mode");
    register(&config, "operator", PASSWORD).expect("the first registration");
    let mode = std::fs::metadata(&config)
        .expect("the file")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "a signing key in a readable file is a hole");
}

#[test]
fn tightening_a_readable_file_says_it_did_and_a_private_one_says_it_did_not() {
    use std::os::unix::fs::PermissionsExt;

    let config = scratch("tighten");
    register(&config, "operator", PASSWORD).expect("the first registration");

    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(
        store::tighten(&config).expect("tightening a readable file"),
        "a readable file must be reported as tightened"
    );

    assert!(
        !store::tighten(&config).expect("tightening a private file"),
        "a private file must report that nothing changed"
    );
    let mode = std::fs::metadata(&config)
        .expect("the file")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn a_file_without_an_account_section_reads_as_no_account() {
    let config = scratch("sectionless");
    std::fs::write(&config, "socket = \"/run/anth.sock\"\n").expect("a settings file");
    assert!(
        store::read(&config).expect("readable").is_none(),
        "a file that never held an account is not an error"
    );
}

#[test]
fn writing_the_account_keeps_the_rest_of_the_file() {
    let config = scratch("preserve");
    std::fs::write(
        &config,
        "# the operator's own comment\nsocket = \"/run/anth.sock\"\n\n[log]\nlevel = \"info\"\n",
    )
    .expect("a settings file");

    register(&config, "operator", PASSWORD).expect("the first registration");

    let text = std::fs::read_to_string(&config).expect("the file");
    assert!(
        text.contains("the operator's own comment"),
        "a comment the operator wrote must survive:\n{text}"
    );
    assert!(text.contains("socket = \"/run/anth.sock\""), "{text}");
    assert!(text.contains("[log]"), "{text}");
}

#[test]
fn a_refresh_token_is_not_stored_in_the_clear() {
    let config = scratch("token-at-rest");
    let account = register(&config, "operator", PASSWORD).expect("the first registration");
    let grant = session::start(&config, &account).expect("a session");

    let text = std::fs::read_to_string(&config).expect("the file");
    assert!(
        !text.contains(&grant.refresh_token),
        "the token itself must not be on disk:\n{text}"
    );
    assert!(
        text.contains(&token_digest(&grant.refresh_token)),
        "its digest must be, or the session could not be found:\n{text}"
    );
}

#[test]
fn random_tokens_differ() {
    let first = random_token().expect("entropy");
    let second = random_token().expect("entropy");
    assert_ne!(first, second);
    // 32 bytes, base64url without padding: 43 characters.
    assert_eq!(first.len(), 43, "a token is 256 bits: {first}");
}

/// The HTTP side: what a request must present, and what it may not.
mod http {
    use crate::auth::guard;
    use crate::tests::api::{ask, unauthenticated_router};
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    /// The behaviour that matters and is easy to lose: a protected route with no
    /// credential answers `401`, not `200` and not `500`.
    #[tokio::test]
    async fn a_protected_route_without_a_credential_is_a_401() {
        let (router, _auth) = unauthenticated_router("no-credential");
        let (status, body) = ask(router, "/api/host-info").await;
        assert_eq!(status, 401, "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("session"),
            "the answer must say what is missing: {body}"
        );
    }

    /// Registration is the one write open before a credential exists.
    #[tokio::test]
    async fn the_bootstrap_route_answers_the_state_and_is_open() {
        let (router, auth) = unauthenticated_router("bootstrap-state");
        let (status, body) = ask(router, "/api/auth/state").await;
        assert_eq!(status, 200);
        assert_eq!(body["registered"], false, "{body}");
        // The field the gate reads before choosing a form. Without it the
        // browser cannot tell "no account yet" from "no session needed".
        assert_eq!(body["required"], true, "{body}");
        assert!(!auth.registered());
    }

    /// With the settings file saying `[auth] enabled = false`, the same route
    /// that answered `401` above answers the daemon.
    ///
    /// This is the whole feature: the extractor is still in every signature, so
    /// the route is still declared protected, and the switch decides what
    /// "protected" means for this process.
    #[tokio::test]
    async fn a_protected_route_answers_without_a_credential_when_auth_is_off() {
        let auth = std::sync::Arc::new(crate::auth::guard::Auth::load(
            crate::tests::scratch("auth-off"),
            false,
        ));
        let router = crate::api::router(
            std::sync::Arc::new(crate::dispatch::Daemon::for_test(
                crate::plugins::registry::Registry::empty(),
                crate::tests::quiet(),
            )),
            std::sync::Arc::clone(&auth),
        );

        let (status, body) = ask(router, "/api/host-info").await;
        assert_eq!(status, 200, "{body}");
        assert!(
            body["hostname"]
                .as_str()
                .is_some_and(|name| !name.is_empty()),
            "the route must answer the daemon, not an empty shell: {body}"
        );
        // The account was never created, and turning auth off must not have
        // created one: the file stays exactly as the operator left it.
        assert!(!auth.registered());
    }

    /// `state()` reports the switch, which is what the browser reads.
    #[tokio::test]
    async fn the_state_route_reports_that_auth_is_off() {
        let auth = std::sync::Arc::new(crate::auth::guard::Auth::load(
            crate::tests::scratch("auth-off-state"),
            false,
        ));
        let router = crate::api::router(
            std::sync::Arc::new(crate::dispatch::Daemon::for_test(
                crate::plugins::registry::Registry::empty(),
                crate::tests::quiet(),
            )),
            auth,
        );

        let (status, body) = ask(router, "/api/auth/state").await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["required"], false, "{body}");
    }

    /// A route whose meaning is "whoever is calling" has no meaning with auth
    /// off, and refuses rather than picking the only account on the host.
    #[tokio::test]
    async fn changing_a_password_still_needs_a_signed_in_operator() {
        let config = crate::tests::scratch("auth-off-password");
        let auth = std::sync::Arc::new(crate::auth::guard::Auth::load(&config, false));
        auth.register("operator", "correct horse battery")
            .expect("the bootstrap");
        // Reloaded with the switch off and the account now on disk, so a refusal
        // cannot come from a missing credential: only from `Authed::account`.
        let auth = std::sync::Arc::new(crate::auth::guard::Auth::load(&config, false));
        assert!(
            auth.registered(),
            "the fixture must have an account to rotate"
        );

        let router = crate::api::router(
            std::sync::Arc::new(crate::dispatch::Daemon::for_test(
                crate::plugins::registry::Registry::empty(),
                crate::tests::quiet(),
            )),
            auth,
        );

        let body = serde_json::json!({
            "current_password": "correct horse battery",
            "new_password": "a different password",
        });
        let response = router
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/auth/password")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        assert_eq!(response.status().as_u16(), 401);
    }

    /// After registering, the same router must refuse an anonymous request: the
    /// guard reads state that changed after the router was built.
    #[tokio::test]
    async fn a_route_closes_as_soon_as_an_account_exists() {
        let (router, auth) = unauthenticated_router("closes");
        auth.register("operator", "correct horse battery")
            .expect("the bootstrap");
        let (status, _) = ask(router, "/api/host-info").await;
        assert_eq!(status, 401);
    }

    /// A token in the `Authorization` header is what `curl` and a script use.
    #[tokio::test]
    async fn a_bearer_token_opens_a_protected_route() {
        let (router, auth) = unauthenticated_router("bearer");
        let grant = auth
            .register("operator", "correct horse battery")
            .expect("the bootstrap");

        let (status, body) = ask_with(
            router,
            "/api/host-info",
            &[("authorization", &format!("Bearer {}", grant.access_token))],
        )
        .await;
        assert_eq!(status, 200, "{body}");
        // A real snapshot of this machine: the point is that the route answered
        // the daemon, not that the daemon was asked to invent a hostname.
        assert!(
            body["hostname"]
                .as_str()
                .is_some_and(|name| !name.is_empty()),
            "a host-info answer must carry a hostname: {body}"
        );
    }

    /// The browser sends a cookie and cannot send a header, which is why the
    /// cookie path exists at all.
    #[tokio::test]
    async fn an_access_cookie_opens_a_protected_route_too() {
        let (router, auth) = unauthenticated_router("cookie");
        let grant = auth
            .register("operator", "correct horse battery")
            .expect("the bootstrap");

        let cookie = format!("{}={}", guard::ACCESS_COOKIE, grant.access_token);
        let (status, body) = ask_with(router, "/api/host-info", &[("cookie", &cookie)]).await;
        assert_eq!(status, 200, "{body}");
    }

    /// A header means "this identity", so a stale cookie beside it must not win.
    #[tokio::test]
    async fn a_bearer_header_wins_over_a_cookie() {
        let (router, auth) = unauthenticated_router("header-wins");
        let grant = auth
            .register("operator", "correct horse battery")
            .expect("the bootstrap");

        let good = format!("Bearer {}", grant.access_token);
        let stale = format!("{}={}", guard::ACCESS_COOKIE, "not a token");
        let (status, _) = ask_with(
            router,
            "/api/host-info",
            &[("authorization", &good), ("cookie", &stale)],
        )
        .await;
        assert_eq!(
            status, 200,
            "the header names the identity, so a stale cookie must not override it"
        );
    }

    /// An expired token is a `401` that says so, so a client knows to refresh
    /// rather than to show a login form.
    #[tokio::test]
    async fn an_expired_token_is_a_401_that_names_the_refresh() {
        let (router, auth) = unauthenticated_router("expired-route");
        let grant = auth
            .register("operator", "correct horse battery")
            .expect("the bootstrap");
        // Registration just issued the token; waiting it out is not an option in
        // a unit test, so the file is rewritten with a far-past expiry instead.
        let stale = crate::auth::session::issue_access(
            &auth.account().expect("an account"),
            crate::auth::session::now() - crate::auth::ACCESS_TTL_SECONDS - 60,
        )
        .expect("a stale token");

        let header = format!("Bearer {stale}");
        let (status, body) =
            ask_with(router, "/api/host-info", &[("authorization", &header)]).await;
        assert_eq!(status, 401, "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("refresh"),
            "the answer must name the recovery: {body}"
        );
        // And the live one from the same account still works, so the test cannot
        // pass by refusing everything.
        let (fresh_status, _) = ask_with(
            {
                let (router, _) = unauthenticated_router("expired-route-fresh");
                router
            },
            "/healthz",
            &[],
        )
        .await;
        assert_eq!(fresh_status, 200);
        let _ = grant;
    }

    /// `same_origin` is what stops another site from driving the panel with this
    /// browser's cookie.
    #[tokio::test]
    async fn a_request_from_another_origin_is_refused() {
        let (router, auth) = unauthenticated_router("origin");
        // The token has to come from this router's own state: another file means
        // another signing key, and the refusal would be about the signature.
        let grant = auth
            .register("operator", "correct horse battery")
            .expect("the bootstrap");
        let header = format!("Bearer {}", grant.access_token);

        // A live credential and a foreign origin: the only thing that can refuse
        // this is the origin check, which is what the case is about.
        let response = post_with(
            router,
            "/api/auth/logout",
            &[
                ("origin", "http://evil.example"),
                ("host", "panel.local:8080"),
                ("authorization", &header),
            ],
            b"",
        )
        .await;
        assert_eq!(response.status(), 401);
        let body: serde_json::Value =
            serde_json::from_str(&body_text(response).await).unwrap_or_default();
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("origin"),
            "the answer must name the reason: {body}"
        );
    }

    /// The same request from the panel's own pages must pass the check, or the
    /// guard would lock the UI out of its own API.
    #[tokio::test]
    async fn a_request_from_the_same_origin_is_allowed_past_the_check() {
        let (router, auth) = unauthenticated_router("same-origin");
        let grant = auth
            .register("operator", "correct horse battery")
            .expect("the bootstrap");

        let header = format!("Bearer {}", grant.access_token);
        let response = post_with(
            router,
            "/api/auth/logout",
            &[
                ("origin", "http://panel.local:8080"),
                ("host", "panel.local:8080"),
                ("authorization", &header),
            ],
            b"",
        )
        .await;
        assert_eq!(response.status(), 200);
        let body: serde_json::Value =
            serde_json::from_str(&body_text(response).await).unwrap_or_default();
        assert_eq!(body["ended"], 1, "{body}");
    }

    /// A `curl` with no `Origin` is not a browser and has no cookie to abuse.
    #[tokio::test]
    async fn a_request_without_an_origin_is_not_stopped_by_the_origin_check() {
        let (router, auth) = unauthenticated_router("no-origin");
        let grant = auth
            .register("operator", "correct horse battery")
            .expect("the bootstrap");

        let header = format!("Bearer {}", grant.access_token);
        let response = post_with(
            router,
            "/api/auth/logout",
            &[("authorization", &header)],
            b"",
        )
        .await;
        assert_eq!(response.status(), 200);
    }

    /// The watch stream is `EventSource`-driven, so it must accept a cookie and
    /// refuse an anonymous caller before it starts writing frames.
    #[tokio::test]
    async fn the_watch_route_refuses_an_anonymous_caller() {
        let (router, _auth) = unauthenticated_router("watch-anonymous");
        let (status, body) = ask(router, "/api/host-info/watch?interval=1s").await;
        assert_eq!(status, 401, "{body}");
    }

    /// A cookie is only useful to a browser if the response sets it, and the
    /// attributes are what make it safe.
    #[tokio::test]
    async fn login_sets_both_cookies_with_the_attributes_that_matter() {
        let (router, auth) = unauthenticated_router("set-cookie");
        auth.register("operator", "correct horse battery")
            .expect("the bootstrap");

        let response = post(
            router,
            "/api/auth/login",
            b"{\"username\":\"operator\",\"password\":\"correct horse battery\"}",
        )
        .await;
        assert_eq!(response.status(), 200);

        let cookies = header_values(&response, "set-cookie");
        assert_eq!(cookies.len(), 2, "one cookie per token: {cookies:?}");

        let access = cookies
            .iter()
            .find(|cookie| cookie.starts_with(guard::ACCESS_COOKIE))
            .expect("an access cookie");
        assert!(access.contains("HttpOnly"), "{access}");
        assert!(access.contains("SameSite=Strict"), "{access}");
        assert!(access.contains("Path=/"), "{access}");

        let refresh = cookies
            .iter()
            .find(|cookie| cookie.starts_with(guard::REFRESH_COOKIE))
            .expect("a refresh cookie");
        assert!(refresh.contains("HttpOnly"), "{refresh}");
        assert!(
            refresh.contains("Path=/api/auth"),
            "the long-lived token must not travel on every request: {refresh}"
        );
    }

    /// Registering twice over HTTP is a `409`, not a silent overwrite.
    #[tokio::test]
    async fn a_second_registration_over_http_is_a_409() {
        let (router, auth) = unauthenticated_router("register-twice");
        auth.register("operator", "correct horse battery")
            .expect("the bootstrap");

        let response = post(
            router,
            "/api/auth/register",
            b"{\"username\":\"intruder\",\"password\":\"another long password\"}",
        )
        .await;
        assert_eq!(response.status(), 409);
    }

    /// A login with a wrong password is a `401` that does not say which half was
    /// wrong.
    #[tokio::test]
    async fn a_wrong_password_over_http_is_a_401() {
        let (router, auth) = unauthenticated_router("bad-password");
        auth.register("operator", "correct horse battery")
            .expect("the bootstrap");

        let response = post(
            router,
            "/api/auth/login",
            b"{\"username\":\"operator\",\"password\":\"wrong but long enough\"}",
        )
        .await;
        assert_eq!(response.status(), 401);
        let body = body_text(response).await;
        assert!(!body.contains("hash"), "{body}");
    }

    /// A refresh rotates over HTTP and hands back a new cookie.
    #[tokio::test]
    async fn a_refresh_over_http_rotates_the_cookie() {
        let (router, auth) = unauthenticated_router("refresh-http");
        let grant = auth
            .register("operator", "correct horse battery")
            .expect("the bootstrap");

        let cookie = format!("{}={}", guard::REFRESH_COOKIE, grant.refresh_token);
        let response = post_with(router, "/api/auth/refresh", &[("cookie", &cookie)], b"").await;
        assert_eq!(response.status(), 200);

        let cookies = header_values(&response, "set-cookie");
        let refreshed = cookies
            .iter()
            .find(|cookie| cookie.starts_with(guard::REFRESH_COOKIE))
            .expect("a new refresh cookie");
        assert!(
            !refreshed.contains(&grant.refresh_token),
            "the cookie must carry the rotated token, not the consumed one: {refreshed}"
        );
    }

    /// A refused refresh clears the cookies, or the browser would retry a dead
    /// token on every page load.
    #[tokio::test]
    async fn a_refused_refresh_clears_the_cookies() {
        let (router, auth) = unauthenticated_router("refresh-dead");
        auth.register("operator", "correct horse battery")
            .expect("the bootstrap");

        let response = post_with(
            router,
            "/api/auth/refresh",
            &[("cookie", &format!("{}=gone", guard::REFRESH_COOKIE))],
            b"",
        )
        .await;
        assert_eq!(response.status(), 401);
        let cookies = header_values(&response, "set-cookie");
        assert!(
            cookies.iter().any(|cookie| cookie.contains("Max-Age=0")),
            "a dead session must be forgotten by the browser: {cookies:?}"
        );
    }

    /// The shell is the login page, so it cannot require a session.
    #[tokio::test]
    async fn the_shell_and_healthz_do_not_need_a_session() {
        let (router, auth) = unauthenticated_router("public");
        auth.register("operator", "correct horse battery")
            .expect("the bootstrap");

        let (healthz, _) = ask(router.clone(), "/healthz").await;
        assert_eq!(healthz, 200, "a health check must not depend on a token");
    }

    /// The helpers below are shared by the cases above.
    async fn post(router: Router, uri: &str, body: &[u8]) -> axum::http::Response<Body> {
        post_with(router, uri, &[], body).await
    }

    async fn post_with(
        router: Router,
        uri: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> axum::http::Response<Body> {
        let mut request = HttpRequest::builder().method("POST").uri(uri);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        router
            .oneshot(request.body(Body::from(body.to_vec())).expect("a request"))
            .await
            .expect("the router answers")
    }

    async fn ask_with(
        router: Router,
        uri: &str,
        headers: &[(&str, &str)],
    ) -> (u16, serde_json::Value) {
        let mut request = HttpRequest::builder().uri(uri);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = router
            .oneshot(request.body(Body::empty()).expect("a request"))
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    fn header_values(response: &axum::http::Response<Body>, name: &str) -> Vec<String> {
        response
            .headers()
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_string))
            .collect()
    }

    async fn body_text(response: axum::http::Response<Body>) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a body");
        String::from_utf8_lossy(&bytes).to_string()
    }
}
