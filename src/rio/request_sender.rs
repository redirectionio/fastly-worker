use fastly::http::request::SendError;
use fastly::{Request, Response};

/// This trait is used to provide a way to override how requests are sent to Fastly backends.
///
/// The application may implement this trait and extend it with further logic such as header
/// manipulation, or further caching logic.
pub trait RequestSender {
    #[allow(unused_mut)]
    fn send(&self, req: Request, backend: String) -> Result<Response, SendError> {
        return req.send(backend);
    }
}

/// Default implementation for verbatim sending request to Fastly.
pub struct DirectRequestSender;
impl RequestSender for DirectRequestSender {}
