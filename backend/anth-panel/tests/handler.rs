//! The handler chain: ordering, blocking classification and the request no
//! handler accepts.
//!
//! The chain is what a plugin will plug into, so these tests pin the contract
//! rather than the concrete handler list.

use std::sync::atomic::{AtomicUsize, Ordering};

use anth_panel::config::FilesConfig;
use anth_panel::daemon::Policy;
use anth_panel::files::FileService;
use anth_panel::handler::{self, CoreHandler, Dispatch, Handler};
use anth_proto::{Request, Response};

fn files() -> FileService {
    FileService::new(&FilesConfig::default()).unwrap()
}

#[test]
fn the_core_handler_answers_echo() {
    let files = files();
    let core = CoreHandler;
    let response = handler::dispatch(
        &[&core],
        &Request::Echo {
            message: "hi".to_string(),
        },
        &Dispatch {
            policy: &Policy::default(),
            files: &files,
        },
    );

    match response {
        Response::Ok { message } => assert_eq!(message, "hi"),
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn the_first_handler_that_answers_wins() {
    struct Always;
    impl Handler for Always {
        fn handle(&self, _: &Request, _: &Dispatch<'_>) -> Option<Response> {
            Some(Response::Ok {
                message: "first".to_string(),
            })
        }
    }

    struct Never;
    impl Handler for Never {
        fn handle(&self, _: &Request, _: &Dispatch<'_>) -> Option<Response> {
            None
        }
    }

    let files = files();
    let first = Always;
    let never = Never;
    let core = CoreHandler;
    let response = handler::dispatch(
        &[&first, &never, &core],
        &Request::Echo {
            message: "ignored".to_string(),
        },
        &Dispatch {
            policy: &Policy::default(),
            files: &files,
        },
    );

    match response {
        Response::Ok { message } => assert_eq!(message, "first"),
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn a_request_no_handler_accepts_is_reported() {
    struct Never;
    impl Handler for Never {
        fn handle(&self, _: &Request, _: &Dispatch<'_>) -> Option<Response> {
            None
        }
    }

    let files = files();
    let never = Never;
    let response = handler::dispatch(
        &[&never],
        &Request::HostInfo,
        &Dispatch {
            policy: &Policy::default(),
            files: &files,
        },
    );

    match response {
        Response::Error { detail } => {
            assert!(detail.contains("no handler"), "unexpected detail: {detail}");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn a_handler_is_asked_about_every_request_once() {
    struct Counting(AtomicUsize);
    impl Handler for Counting {
        fn handle(&self, _: &Request, _: &Dispatch<'_>) -> Option<Response> {
            self.0.fetch_add(1, Ordering::SeqCst);
            None
        }
    }

    let files = files();
    let counting = Counting(AtomicUsize::new(0));
    let core = CoreHandler;
    handler::dispatch(
        &[&counting, &core],
        &Request::HostInfo,
        &Dispatch {
            policy: &Policy::default(),
            files: &files,
        },
    );

    assert_eq!(counting.0.load(Ordering::SeqCst), 1);
}

#[test]
fn stateless_requests_stay_off_the_blocking_worker() {
    let core = CoreHandler;
    assert!(!handler::blocks(
        &[&core],
        &Request::Echo {
            message: "hi".to_string()
        }
    ));
}

#[test]
fn filesystem_and_proc_reads_run_on_a_blocking_worker() {
    let core = CoreHandler;
    for request in [
        Request::HostInfo,
        Request::ProcessList,
        Request::FileList {
            path: "/".to_string(),
        },
        Request::AccountList,
    ] {
        assert!(
            handler::blocks(&[&core], &request),
            "expected {request:?} to be blocking"
        );
    }
}

#[test]
fn a_chain_with_a_blocking_handler_marks_the_request_blocking() {
    struct Slow;
    impl Handler for Slow {
        fn handle(&self, _: &Request, _: &Dispatch<'_>) -> Option<Response> {
            None
        }
        fn blocks(&self, _: &Request) -> bool {
            true
        }
    }

    let slow = Slow;
    let core = CoreHandler;
    assert!(handler::blocks(
        &[&core, &slow],
        &Request::Echo {
            message: "hi".to_string()
        }
    ));
}
