//! How a failed POST to quipu is classified.

use crate::errors::Error;

/// quipu never answering, or answering "not now" (408 or 5xx), is an outage:
/// `Unavailable`, which a caller may count and allow. Any other status is quipu
/// refusing this request, and stays a loud `Projection` error.
pub(super) fn classify(url: &str, e: &ureq::Error) -> Error {
    match e {
        ureq::Error::Transport(_) => Error::Unavailable(format!("POST {url} failed: {e}")),
        ureq::Error::Status(code, _) if *code == 408 || *code >= 500 => {
            Error::Unavailable(format!("POST {url} failed: {e}"))
        }
        ureq::Error::Status(..) => Error::Projection(format!("POST {url} failed: {e}")),
    }
}
